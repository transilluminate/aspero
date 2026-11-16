//! A daily cap on a provider's spend: paid calls stop once the day's use
//! (as the provider reports it) reaches it, free ones go on, checks fall
//! back to other sources, and it lifts when the budget resets.

// Exact constants: scores and spends rounded or summed from binary-exact values.
#![allow(clippy::float_cmp)]

mod common;

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use aspero_core::{
    CapKind, Capabilities, Claim, Error, ErrorKind, FieldSet, Hit, IdKind, Lookup, Page, Provider,
    ProviderId, ResiliencePolicy, Response, Result, SearchQuery, SourceRecord, Usage, WorkId,
};
use aspero_engine::{Engine, EngineError, Outcome, PrepaidDays, SqliteCache, Verdict};
use async_trait::async_trait;
use common::*;
use jiff::Timestamp;

/// 2026-11-14T12:00:00Z; the budget resets at midnight.
const NOON: i64 = 1_794_657_600;
const MIDNIGHT: i64 = NOON + 12 * 3600;

/// OpenAlex's pricing: searches $0.001, batch lookups $0.0001, single
/// lookups free, a free budget status with a key; out of a $1 day.
struct Metered {
    caps: Capabilities,
    works: Vec<SourceRecord>,
    remaining: Mutex<f64>,
    /// Prepaid credit, spent once the allowance is used up; without any,
    /// the allowance's remaining goes below 0.
    prepaid: Mutex<Option<f64>>,
    /// Credit bought, as `/rate-limit` says.
    bought: Mutex<Option<f64>>,
    /// How much lower than the truth responses' headers say the balance
    /// (OpenAlex's servers disagree); `/rate-limit` says the truth.
    skew: Mutex<f64>,
    calls: Mutex<Vec<&'static str>>,
}

impl Metered {
    fn new(used: f64, keyed: bool, works: Vec<SourceRecord>) -> Arc<Self> {
        let mut caps = Capabilities::default();
        caps.lookup_kinds = vec![IdKind::Doi];
        caps.max_batch = 100;
        caps.max_free_batch = 1;
        caps.fields = FieldSet::ALL;
        caps.search = true;
        caps.budget_status = keyed;
        Arc::new(Metered {
            caps,
            works,
            remaining: Mutex::new(1.0 - used),
            prepaid: Mutex::default(),
            bought: Mutex::default(),
            skew: Mutex::default(),
            calls: Mutex::default(),
        })
    }

    /// The same, with `usd` of prepaid credit.
    fn with_credit(used: f64, keyed: bool, usd: f64) -> Arc<Self> {
        let metered = Metered::new(used, keyed, vec![openalex_work("10.1234/a", "Alpha")]);
        *metered.prepaid.lock().unwrap() = Some(usd);
        *metered.bought.lock().unwrap() = Some(usd);
        metered
    }

    fn spend(&self, call: &'static str, cost: f64) -> Usage {
        self.calls.lock().unwrap().push(call);
        let mut remaining = self.remaining.lock().unwrap();
        let mut prepaid = self.prepaid.lock().unwrap();
        match prepaid.as_mut() {
            // The allowance first, then credit, as OpenAlex spends them.
            Some(credit) => {
                let from_allowance = cost.min(remaining.max(0.0));
                *remaining -= from_allowance;
                *credit -= cost - from_allowance;
            }
            None => *remaining -= cost,
        }
        let mut usage = Usage::default();
        usage.cost = Some(cost);
        usage.limit = Some(1.0);
        usage.remaining = Some(*remaining);
        usage.prepaid_remaining = *prepaid;
        if call == "budget" {
            usage.prepaid_bought = *self.bought.lock().unwrap();
        } else {
            let skew = *self.skew.lock().unwrap();
            usage.prepaid_remaining = prepaid.map(|p| p - skew);
        }
        usage.resets_at = Some(Timestamp::from_second(MIDNIGHT).unwrap());
        usage
    }

    fn calls(&self) -> Vec<&'static str> {
        self.calls.lock().unwrap().clone()
    }

    fn count(&self, call: &str) -> usize {
        self.calls().iter().filter(|c| **c == call).count()
    }
}

#[async_trait]
impl Provider for Metered {
    fn id(&self) -> ProviderId {
        ProviderId::from_static("openalex")
    }

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    async fn lookup(&self, ids: &[WorkId], _: FieldSet) -> Result<Response<Vec<Lookup>>> {
        let usage = match ids.len() {
            1 => self.spend("single", 0.0),
            _ => self.spend("batch", 0.0001),
        };
        let lookups = ids
            .iter()
            .map(|id| Lookup {
                id: id.clone(),
                record: self.works.iter().find(|w| w.ids.contains(id)).cloned(),
            })
            .collect();
        Ok(Response::with_usage(lookups, usage))
    }

    async fn search(&self, query: &SearchQuery) -> Result<Response<Page>> {
        let usage = self.spend("search", 0.001);
        let hits = self
            .works
            .iter()
            .filter(|w| {
                w.work.title.as_deref().map(str::to_lowercase) == Some(query.text.to_lowercase())
            })
            .map(|w| Hit::new(w.clone(), Some(1.0)))
            .collect();
        Ok(Response::with_usage(Page::new(hits, None, None), usage))
    }

    async fn budget(&self) -> Result<Usage> {
        Ok(self.spend("budget", 0.0))
    }
}

fn openalex_work(doi: &str, title: &str) -> SourceRecord {
    work("openalex", &[doi], title, &LECUN, 2015)
}

static CLOCK: AtomicI64 = AtomicI64::new(NOON);

fn engine_with(openalex: Arc<Metered>, others: Vec<Arc<Scripted>>, cap: f64) -> Engine {
    let mut policy = ResiliencePolicy::default();
    policy.max_retries = 0;
    let mut builder = Engine::builder()
        .provider(openalex, policy.clone(), 10)
        .daily_cap(ProviderId::from_static("openalex"), cap)
        .clock(|| Timestamp::from_second(NOON).unwrap());
    for (i, p) in others.into_iter().enumerate() {
        builder = builder.provider(p, policy.clone(), 20 + i32::try_from(i).unwrap());
    }
    builder.build()
}

fn capped(e: &EngineError) -> (f64, f64, Option<Timestamp>) {
    match e {
        EngineError::Provider {
            error:
                Error::BudgetCapped {
                    cap,
                    used,
                    resets_at,
                    which: CapKind::Allowance,
                },
            ..
        } => (*cap, *used, *resets_at),
        other => panic!("not capped on the allowance: {other:?}"),
    }
}

/// The credit cap reached, and the credit spent today, rounded to a
/// millionth.
fn credit_capped(e: &EngineError) -> (f64, f64) {
    match e {
        EngineError::Provider {
            error:
                Error::BudgetCapped {
                    cap,
                    used,
                    which: CapKind::Credit,
                    ..
                },
            ..
        } => (*cap, (used * 1e6).round() / 1e6),
        other => panic!("not capped on credit: {other:?}"),
    }
}

/// An engine with OpenAlex's caps on its allowance and its credit, sharing
/// the day's prepaid balances through `days` if given.
fn credit_engine(
    openalex: Arc<Metered>,
    allowance: Option<f64>,
    credit: Option<f64>,
    days: Option<Arc<dyn PrepaidDays>>,
) -> Engine {
    let mut policy = ResiliencePolicy::default();
    policy.max_retries = 0;
    let id = ProviderId::from_static("openalex");
    let mut builder = Engine::builder()
        .provider(openalex, policy, 10)
        .clock(|| Timestamp::from_second(NOON).unwrap());
    if let Some(cap) = allowance {
        builder = builder.daily_cap(id.clone(), cap);
    }
    if let Some(cap) = credit {
        builder = builder.daily_credit_cap(id, cap);
    }
    if let Some(days) = days {
        builder = builder.prepaid_days(days);
    }
    builder.build()
}

async fn search(engine: &Engine) -> std::result::Result<(), EngineError> {
    engine
        .search(None, &SearchQuery::new("alpha"))
        .await
        .map(|_| ())
}

/// The allowance used up by something else: credit is spent, whatever
/// the allowance's cap, until today's credit reaches its own cap.
#[tokio::test]
async fn once_the_allowance_is_used_up_credit_is_spent_to_its_own_cap() {
    let openalex = Metered::with_credit(1.0, true, 10.0);
    let engine = credit_engine(openalex.clone(), Some(0.5), Some(0.0025), None);
    // The refresh sees the day's first balance; three searches spend
    // $0.003 of credit, past the cap, so the fourth isn't sent.
    for _ in 0..3 {
        search(&engine).await.unwrap();
    }
    let error = search(&engine).await.unwrap_err();
    assert_eq!(credit_capped(&error), (0.0025, 0.003));
    assert_eq!(error.kind(), ErrorKind::BudgetCapped);
    assert_eq!(openalex.calls(), ["budget", "search", "search", "search"]);
    let budget = engine.budgets()[0].1;
    assert_eq!(budget.prepaid_spent_today, Some(0.003));
    assert_eq!(budget.credit_spent_today_here, Some(0.003));
    assert!(error.to_string().contains("today's credit cap of $0.0025"));
}

/// A day running into the credit: the allowance first, then credit to
/// its cap.
#[tokio::test]
async fn the_allowance_runs_out_then_credit_runs_to_its_cap() {
    let openalex = Metered::with_credit(0.999, true, 10.0);
    let engine = credit_engine(openalex.clone(), None, Some(0.001), None);
    // The last of the allowance, then $0.001 of credit.
    search(&engine).await.unwrap();
    search(&engine).await.unwrap();
    let error = search(&engine).await.unwrap_err();
    assert_eq!(credit_capped(&error), (0.001, 0.001));
    assert_eq!(openalex.count("search"), 2);
}

#[tokio::test]
async fn a_credit_cap_of_nothing_never_spends_credit() {
    let openalex = Metered::with_credit(1.0, true, 10.0);
    let engine = credit_engine(openalex.clone(), None, Some(0.0), None);
    let error = search(&engine).await.unwrap_err();
    assert_eq!(credit_capped(&error), (0.0, 0.0));
    assert_eq!(openalex.calls(), ["budget"]);
}

/// With some of the allowance left, its cap still holds the rest back,
/// credit cap or not.
#[tokio::test]
async fn past_the_allowance_cap_with_allowance_left_stops_there() {
    let openalex = Metered::with_credit(0.6, true, 10.0);
    let engine = credit_engine(openalex.clone(), Some(0.5), Some(5.0), None);
    let (cap, used, _) = capped(&search(&engine).await.unwrap_err());
    assert_eq!((cap, used), (0.5, 0.6));
    assert_eq!(openalex.calls(), ["budget"]);
}

/// No credit cap: as before it had one. The day's use tops out at the
/// allowance, so a cap over it never stops (credit is spent freely), and
/// one at it stops with no credit spent.
#[tokio::test]
async fn without_a_credit_cap_the_allowance_cap_decides_as_before() {
    let openalex = Metered::with_credit(1.0, true, 10.0);
    let engine = credit_engine(openalex.clone(), Some(2.0), None, None);
    for _ in 0..3 {
        search(&engine).await.unwrap();
    }
    assert_eq!(engine.budgets()[0].1.prepaid_spent_today, Some(0.003));

    let openalex = Metered::with_credit(1.0, true, 10.0);
    let engine = credit_engine(openalex.clone(), Some(1.0), None, None);
    let (cap, used, _) = capped(&search(&engine).await.unwrap_err());
    assert_eq!((cap, used), (1.0, 1.0));
}

/// Credit spent today counts everything using the key from the day's
/// first balance seen by any process sharing the store: a second engine
/// (another process, a restart) starts from the first one's.
#[tokio::test]
async fn credit_spent_today_is_shared_through_the_store() {
    let days: Arc<dyn PrepaidDays> = Arc::new(SqliteCache::open_in_memory().unwrap());
    let openalex = Metered::with_credit(1.0, true, 10.0);
    let first = credit_engine(openalex.clone(), None, Some(0.0025), Some(days.clone()));
    search(&first).await.unwrap();
    search(&first).await.unwrap();

    // The same key, another engine: it sees $0.002 spent already.
    let second = credit_engine(openalex.clone(), None, Some(0.0025), Some(days));
    search(&second).await.unwrap();
    let error = search(&second).await.unwrap_err();
    assert_eq!(credit_capped(&error), (0.0025, 0.003));
    assert_eq!(second.budgets()[0].1.prepaid_spent_today, Some(0.003));
}

#[tokio::test]
async fn paid_calls_stop_at_the_cap_and_free_lookups_go_on() {
    let works = vec![
        openalex_work("10.1234/a", "Alpha"),
        openalex_work("10.1234/b", "Beta"),
        openalex_work("10.1234/c", "Gamma"),
    ];
    // Without a key, the first search's own headers tell the day's use.
    let openalex = Metered::new(0.0005, false, works);
    let engine = engine_with(openalex.clone(), vec![], 0.002);

    // Searches until the day's use reaches the cap: .0005 → .0015 → .0025.
    for _ in 0..2 {
        engine
            .search(None, &SearchQuery::new("alpha"))
            .await
            .unwrap();
    }
    let error = engine
        .search(None, &SearchQuery::new("beta"))
        .await
        .unwrap_err();
    let (cap, used, resets_at) = capped(&error);
    assert_eq!(cap, 0.002);
    assert!((used - 0.0025).abs() < 1e-9, "{used}");
    assert_eq!(resets_at, Some(Timestamp::from_second(MIDNIGHT).unwrap()));
    assert_eq!(error.kind(), ErrorKind::BudgetCapped);
    assert_eq!(openalex.count("search"), 2, "the third was never sent");

    // A lookup of several IDs, a paid batch, goes one free lookup at a time.
    let ids: Vec<WorkId> = ["10.1234/a", "10.1234/b", "10.1234/c"]
        .iter()
        .map(|i| i.parse().unwrap())
        .collect();
    let report = engine.lookup(&ids, FieldSet::DEFAULT).await;
    assert!(report.results.iter().all(|r| r.record.is_some()));
    assert_eq!(openalex.count("batch"), 0);
    assert_eq!(openalex.count("single"), 3);
}

#[tokio::test]
async fn under_the_cap_batches_stay_batches() {
    let works = vec![
        openalex_work("10.1234/a", "Alpha"),
        openalex_work("10.1234/b", "Beta"),
    ];
    let openalex = Metered::new(0.0, false, works);
    let engine = engine_with(openalex.clone(), vec![], 0.5);
    let ids: Vec<WorkId> = ["10.1234/a", "10.1234/b"]
        .iter()
        .map(|i| i.parse().unwrap())
        .collect();
    engine.lookup(&ids, FieldSet::DEFAULT).await;
    assert_eq!(openalex.calls(), ["batch"]);
}

#[tokio::test]
async fn with_a_key_a_day_already_past_the_cap_stops_at_once() {
    let openalex = Metered::new(0.05, true, vec![openalex_work("10.1234/a", "Alpha")]);
    let engine = engine_with(openalex.clone(), vec![], 0.01);
    let error = engine
        .search(None, &SearchQuery::new("alpha"))
        .await
        .unwrap_err();
    let (cap, used, _) = capped(&error);
    assert_eq!(cap, 0.01);
    assert!((used - 0.05).abs() < 1e-9);
    // One free refresh, and no search.
    assert_eq!(openalex.calls(), ["budget"]);

    // Known now: no second refresh.
    engine
        .search(None, &SearchQuery::new("alpha"))
        .await
        .unwrap_err();
    assert_eq!(openalex.calls(), ["budget"]);
}

#[tokio::test]
async fn without_a_key_unknown_use_counts_as_under_the_cap() {
    let openalex = Metered::new(0.05, false, vec![openalex_work("10.1234/a", "Alpha")]);
    let engine = engine_with(openalex.clone(), vec![], 0.01);
    // Nothing is asked first (there's no free way without a key): the
    // search goes, and its headers say the day is past the cap.
    engine
        .search(None, &SearchQuery::new("alpha"))
        .await
        .unwrap();
    engine
        .search(None, &SearchQuery::new("alpha"))
        .await
        .unwrap_err();
    assert_eq!(openalex.calls(), ["search"]);
}

#[tokio::test]
async fn a_cap_of_nothing_sends_no_paid_call() {
    let openalex = Metered::new(0.0, true, vec![]);
    let engine = engine_with(openalex.clone(), vec![], 0.0);
    let error = engine
        .search(None, &SearchQuery::new("alpha"))
        .await
        .unwrap_err();
    assert_eq!(capped(&error).0, 0.0);
    assert_eq!(openalex.calls(), ["budget"]);
}

#[tokio::test]
async fn checks_fall_back_to_the_free_sources_past_the_cap() {
    let openalex = Metered::new(0.05, true, vec![]);
    let crossref = Arc::new(
        Scripted::new("crossref")
            .matcher(vec![deep_learning()])
            .with(vec![deep_learning()]),
    );
    let engine = engine_with(openalex.clone(), vec![crossref.clone()], 0.01);
    let v = engine
        .verify(
            &Claim::from_text("LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015"),
            5,
        )
        .await;
    assert_ne!(v.verdict, Verdict::Unverifiable, "{}", v.reason_text);
    let best = &v.report.candidates[v.best_match.unwrap()];
    assert_eq!(best.record.source.as_str(), "crossref");
    assert_eq!(openalex.count("search"), 0, "{:?}", openalex.calls());
    assert!(
        v.report
            .failures()
            .iter()
            .any(|e| e.kind() == ErrorKind::BudgetCapped)
    );
}

#[tokio::test]
async fn the_cap_lifts_when_the_budget_resets() {
    let openalex = Metered::new(0.05, false, vec![openalex_work("10.1234/a", "Alpha")]);
    let mut policy = ResiliencePolicy::default();
    policy.max_retries = 0;
    let engine = Engine::builder()
        .provider(openalex.clone(), policy, 10)
        .daily_cap(ProviderId::from_static("openalex"), 0.01)
        .clock(|| Timestamp::from_second(CLOCK.load(Ordering::SeqCst)).unwrap())
        .build();
    let query = SearchQuery::new("alpha");
    let search = || engine.search(None, &query);
    search().await.unwrap();
    search().await.unwrap_err();

    // Past midnight the day's use is unknown again: under the cap.
    CLOCK.store(MIDNIGHT + 1, Ordering::SeqCst);
    search().await.unwrap();
    assert_eq!(openalex.count("search"), 2);
}

#[tokio::test]
async fn single_lookups_are_never_refused() {
    let openalex = Metered::new(0.9, true, vec![openalex_work("10.1234/a", "Alpha")]);
    let engine = engine_with(openalex.clone(), vec![], 0.01);
    let id: WorkId = "10.1234/a".parse().unwrap();
    let report = engine.lookup(&[id], FieldSet::DEFAULT).await;
    assert!(matches!(
        report.results[0].attempts[0].outcome,
        Outcome::Found
    ));
    // Free, so no refresh was needed to decide.
    assert_eq!(openalex.calls(), ["single"]);
}

/// Responses' headers can say a stale balance (OpenAlex's servers
/// disagree), which skews the credit spent on the key, but not the credit
/// spent here, which each response's cost tells: the cap counts that, and
/// no `/rate-limit` is asked to check a stop.
#[tokio::test]
async fn stale_balances_dont_move_the_credit_cap() {
    let openalex = Metered::with_credit(1.0, true, 10.0);
    // Headers say $0.01 less than the truth.
    *openalex.skew.lock().unwrap() = 0.01;
    let engine = credit_engine(openalex.clone(), None, Some(0.0025), None);
    for _ in 0..3 {
        search(&engine).await.unwrap();
    }
    let error = search(&engine).await.unwrap_err();
    assert_eq!(credit_capped(&error), (0.0025, 0.003));
    assert_eq!(openalex.calls(), ["budget", "search", "search", "search"]);
    let budget = engine.budgets()[0].1;
    assert_eq!(budget.credit_spent_today_here, Some(0.003));
    assert_eq!(budget.prepaid_spent_today, Some(0.013));
}

/// Other tools using the key (a researcher's scripts, another app) have
/// spent far past this caller's credit cap today; it has spent next to
/// nothing, so its calls go on, to its cap, and what's spendable is what's
/// left of the cap.
#[tokio::test]
async fn the_credit_cap_counts_only_credit_spent_here() {
    let id = ProviderId::from_static("openalex");
    let openalex = Metered::with_credit(1.0, true, 10.0);
    let engine = credit_engine(openalex.clone(), None, Some(0.0025), None);
    search(&engine).await.unwrap();
    // Something else spends $5 of credit on the key.
    *openalex.prepaid.lock().unwrap() = Some(10.0 - 0.001 - 5.0);
    search(&engine).await.unwrap();
    let budget = engine.budgets()[0].1;
    assert_eq!(budget.prepaid_spent_today, Some(5.002));
    assert_eq!(budget.credit_spent_today_here, Some(0.002));
    assert_eq!(engine.spendable_today(&id, &budget), Some(0.0005));
    search(&engine).await.unwrap();
    let error = search(&engine).await.unwrap_err();
    assert_eq!(credit_capped(&error), (0.0025, 0.003));
    assert!(
        error
            .to_string()
            .contains("($0.0030 of credit spent today through aspero here)"),
        "{error}"
    );
    let budget = engine.budgets()[0].1;
    assert_eq!(engine.spendable_today(&id, &budget), Some(0.0));
}

/// Two engines sharing a store (two processes on the cache file, or a
/// restart) count one credit spent here: each sees the other's at once,
/// before its own next response.
#[tokio::test]
async fn engines_sharing_a_store_share_the_credit_spent_here() {
    let days: Arc<dyn PrepaidDays> = Arc::new(SqliteCache::open_in_memory().unwrap());
    let openalex = Metered::with_credit(1.0, true, 10.0);
    let first = credit_engine(openalex.clone(), None, Some(0.0025), Some(days.clone()));
    let second = credit_engine(openalex.clone(), None, Some(0.0025), Some(days));
    search(&second).await.unwrap();
    assert_eq!(second.budgets()[0].1.credit_spent_today_here, Some(0.001));
    search(&first).await.unwrap();
    search(&first).await.unwrap();
    assert_eq!(first.budgets()[0].1.credit_spent_today_here, Some(0.003));
    // The second's last response said $0.001; the store says $0.003.
    let error = search(&second).await.unwrap_err();
    assert_eq!(credit_capped(&error), (0.0025, 0.003));
}
