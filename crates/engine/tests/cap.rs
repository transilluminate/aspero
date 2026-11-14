//! A daily cap on a provider's spend: paid calls stop once the day's use
//! (as the provider reports it) reaches it, free ones go on, checks fall
//! back to other sources, and it lifts when the budget resets.

mod common;

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use aspero_core::{
    Capabilities, Claim, Error, ErrorKind, FieldSet, Hit, IdKind, Lookup, Page, Provider,
    ProviderId, ResiliencePolicy, Response, Result, SearchQuery, SourceRecord, Usage, WorkId,
};
use aspero_engine::{Engine, EngineError, Outcome, Verdict};
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
            calls: Mutex::default(),
        })
    }

    fn spend(&self, call: &'static str, cost: f64) -> Usage {
        self.calls.lock().unwrap().push(call);
        let mut remaining = self.remaining.lock().unwrap();
        *remaining -= cost;
        let mut usage = Usage::default();
        usage.cost = Some(cost);
        usage.limit = Some(1.0);
        usage.remaining = Some(*remaining);
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
        builder = builder.provider(p, policy.clone(), 20 + i as i32);
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
                },
            ..
        } => (*cap, *used, *resets_at),
        other => panic!("not capped: {other:?}"),
    }
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
    assert_ne!(v.verdict, Verdict::Unverifiable, "{}", v.reason);
    let best = &v.report.candidates[v.best.unwrap()];
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
