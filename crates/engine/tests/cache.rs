//! Engine caching: fresh hits skip providers, misses are remembered, expiry
//! refetches, failures fall back to stale answers, and calls past the cache
//! refresh it.

mod common;

use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use aspero_core::{
    Capabilities, Error, FieldSet, Hit, IdKind, Lookup, Page, Provider, ProviderId, Raw,
    ResiliencePolicy, Response, Result, SearchQuery, SourceRecord, Work, WorkId,
};
use aspero_engine::{CallOptions, Engine, Outcome, Served, SqliteCache};
use async_trait::async_trait;
use jiff::Timestamp;

thread_local! {
    /// Seconds since the test epoch; tokio's current-thread runtime runs
    /// tests on their own thread, so each test has its own clock.
    static NOW: Cell<i64> = const { Cell::new(0) };
}

fn clock() -> Timestamp {
    Timestamp::from_second(1_794_312_000 + NOW.with(Cell::get)).unwrap()
}

fn advance(seconds: i64) {
    NOW.with(|n| n.set(n.get() + seconds));
}

const DAY: i64 = 86_400;

/// Knows one work (10.1000/known), and another (10.1000/late) once
/// registered; counts calls; can be switched off.
struct Counting {
    caps: Capabilities,
    calls: AtomicUsize,
    down: AtomicBool,
    /// The known work's title has since been corrected.
    corrected: AtomicBool,
    registered: AtomicBool,
    /// Fields its records cover.
    covers: FieldSet,
}

impl Counting {
    fn new(covers: FieldSet) -> Self {
        let mut caps = Capabilities::default();
        caps.lookup_kinds = vec![IdKind::Doi];
        caps.max_batch = 10;
        caps.search = true;
        Counting {
            caps,
            calls: AtomicUsize::new(0),
            down: AtomicBool::new(false),
            corrected: AtomicBool::new(false),
            registered: AtomicBool::new(false),
            covers,
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn known(&self) -> SourceRecord {
        let title = match self.corrected.load(Ordering::SeqCst) {
            true => "Known (corrected)",
            false => "Known",
        };
        self.record("10.1000/known", title)
    }

    fn record(&self, doi: &str, title: &str) -> SourceRecord {
        let mut work = Work::default();
        work.title = Some(title.into());
        SourceRecord::new(
            ProviderId::from_static("counting"),
            vec![doi.parse().unwrap()],
            work,
            self.covers,
            Raw {
                media_type: "application/json".into(),
                body: "{}".into(),
            },
            clock(),
        )
    }

    fn call(&self) -> Result<()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.down.load(Ordering::SeqCst) {
            return Err(Error::AuthRejected { message: None });
        }
        Ok(())
    }
}

#[async_trait]
impl Provider for Counting {
    fn id(&self) -> ProviderId {
        ProviderId::from_static("counting")
    }

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    async fn lookup(&self, ids: &[WorkId], _: FieldSet) -> Result<Response<Vec<Lookup>>> {
        self.call()?;
        let mut works = vec![self.known()];
        if self.registered.load(Ordering::SeqCst) {
            works.push(self.record("10.1000/late", "Late"));
        }
        let lookups = ids
            .iter()
            .map(|id| Lookup {
                id: id.clone(),
                record: works.iter().find(|w| w.ids.contains(id)).cloned(),
            })
            .collect();
        Ok(Response::new(lookups))
    }

    async fn search(&self, _: &SearchQuery) -> Result<Response<Page>> {
        self.call()?;
        let hit = Hit::new(self.known(), Some(1.0));
        Ok(Response::new(Page::new(vec![hit], Some(1), None)))
    }
}

fn setup() -> (Engine, Arc<Counting>) {
    setup_covering(FieldSet::ALL)
}

fn setup_covering(covers: FieldSet) -> (Engine, Arc<Counting>) {
    NOW.with(|n| n.set(0));
    let provider = Arc::new(Counting::new(covers));
    let mut policy = ResiliencePolicy::default();
    policy.max_retries = 0;
    let engine = Engine::builder()
        .provider(provider.clone(), policy, 0)
        .cache(Arc::new(SqliteCache::open_in_memory().unwrap()))
        .clock(clock)
        .build();
    (engine, provider)
}

fn ids(list: &[&str]) -> Vec<WorkId> {
    list.iter().map(|s| s.parse().unwrap()).collect()
}

fn served(report: &aspero_engine::LookupReport) -> Vec<(bool, Served)> {
    report
        .results
        .iter()
        .map(|r| {
            let a = r.attempts.last().unwrap();
            (matches!(a.outcome, Outcome::Found), a.served)
        })
        .collect()
}

#[tokio::test]
async fn second_lookup_is_served_from_cache_including_misses() {
    let (engine, provider) = setup();
    let wanted = ids(&["10.1000/known", "10.1000/invented"]);

    let first = engine.lookup(&wanted, FieldSet::DEFAULT).await;
    assert_eq!(
        served(&first),
        [(true, Served::Live), (false, Served::Live)]
    );

    let second = engine.lookup(&wanted, FieldSet::DEFAULT).await;
    assert_eq!(
        served(&second),
        [(true, Served::Cache), (false, Served::Cache)]
    );
    assert_eq!(provider.calls(), 1, "nothing refetched");
}

#[tokio::test]
async fn misses_expire_sooner_than_records() {
    let (engine, provider) = setup();
    let wanted = ids(&["10.1000/known", "10.1000/invented"]);
    engine.lookup(&wanted, FieldSet::DEFAULT).await;

    advance(2 * DAY);
    let report = engine.lookup(&wanted, FieldSet::DEFAULT).await;
    assert_eq!(
        served(&report),
        [(true, Served::Cache), (false, Served::Live)]
    );
    assert_eq!(provider.calls(), 2, "only the miss was re-asked");

    advance(8 * DAY);
    engine.lookup(&wanted, FieldSet::DEFAULT).await;
    assert_eq!(provider.calls(), 3, "both expired: one batched call");
}

#[tokio::test]
async fn the_trust_in_cached_answers_is_configurable() {
    NOW.with(|n| n.set(0));
    let provider = Arc::new(Counting::new(FieldSet::ALL));
    let mut policy = ResiliencePolicy::default();
    policy.max_retries = 0;
    // Found records for 3 days, "not found" not at all.
    let mut trust = aspero_engine::CachePolicy::default();
    trust.records = std::time::Duration::from_secs(3 * DAY as u64);
    trust.misses = std::time::Duration::ZERO;
    let engine = Engine::builder()
        .provider(provider.clone(), policy, 0)
        .cache(Arc::new(SqliteCache::open_in_memory().unwrap()))
        .cache_policy(trust)
        .clock(clock)
        .build();
    let known = ids(&["10.1000/known"]);
    let invented = ids(&["10.1000/invented"]);
    engine.lookup(&known, FieldSet::DEFAULT).await;
    engine.lookup(&invented, FieldSet::DEFAULT).await;
    assert_eq!(provider.calls(), 2);

    advance(1);
    let again = engine.lookup(&invented, FieldSet::DEFAULT).await;
    assert_eq!(
        served(&again),
        [(false, Served::Live)],
        "misses aren't trusted"
    );
    advance(2 * DAY);
    let cached = engine.lookup(&known, FieldSet::DEFAULT).await;
    assert_eq!(served(&cached), [(true, Served::Cache)]);
    advance(2 * DAY);
    let expired = engine.lookup(&known, FieldSet::DEFAULT).await;
    assert_eq!(served(&expired), [(true, Served::Live)], "past 3 days");
    assert_eq!(provider.calls(), 4);
}

#[tokio::test]
async fn a_failing_provider_falls_back_to_stale_records() {
    let (engine, provider) = setup();
    engine
        .lookup(&ids(&["10.1000/known"]), FieldSet::DEFAULT)
        .await;

    advance(30 * DAY);
    provider.down.store(true, Ordering::SeqCst);
    let report = engine
        .lookup(
            &ids(&["10.1000/known", "10.1000/never-seen"]),
            FieldSet::DEFAULT,
        )
        .await;

    assert_eq!(
        served(&report),
        [(true, Served::Stale), (false, Served::Live)]
    );
    assert!(matches!(
        report.results[1].attempts[0].outcome,
        Outcome::Failed(Error::AuthRejected { .. })
    ));
}

#[tokio::test]
async fn searches_are_cached_by_exact_query_and_served_stale_on_error() {
    let (engine, provider) = setup();
    let query = SearchQuery::new("known");

    assert_eq!(
        engine.search(None, &query).await.unwrap().served,
        Served::Live
    );
    assert_eq!(
        engine.search(None, &query).await.unwrap().served,
        Served::Cache
    );
    assert_eq!(provider.calls(), 1);

    advance(2 * DAY);
    provider.down.store(true, Ordering::SeqCst);
    let report = engine.search(None, &query).await.unwrap();
    assert_eq!(report.served, Served::Stale);
    assert_eq!(report.page.hits.len(), 1);

    assert!(
        engine
            .search(None, &SearchQuery::new("never asked"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_cached_record_lacking_requested_fields_is_refetched() {
    let (engine, provider) = setup_covering(FieldSet::DEFAULT);
    let wanted = ids(&["10.1000/known"]);
    engine.lookup(&wanted, FieldSet::DEFAULT).await;
    engine.lookup(&wanted, FieldSet::DEFAULT).await;
    assert_eq!(provider.calls(), 1, "covered: served from cache");

    let report = engine.lookup(&wanted, FieldSet::ALL).await;
    assert_eq!(provider.calls(), 2, "abstract etc. not cached: refetched");
    assert_eq!(served(&report), [(true, Served::Live)]);
}

fn title(report: &aspero_engine::LookupReport) -> Option<&str> {
    report.results[0].record.as_ref()?.work.title.as_deref()
}

#[tokio::test]
async fn a_fresh_lookup_asks_past_the_cache_and_replaces_its_answer() {
    let (engine, provider) = setup();
    let fresh = engine.with_options(CallOptions::fresh());
    let wanted = ids(&["10.1000/known"]);
    engine.lookup(&wanted, FieldSet::DEFAULT).await;
    provider.corrected.store(true, Ordering::SeqCst);

    let cached = engine.lookup(&wanted, FieldSet::DEFAULT).await;
    assert_eq!(served(&cached), [(true, Served::Cache)]);
    assert_eq!(title(&cached), Some("Known"), "the cache can't know");
    assert_eq!(provider.calls(), 1);

    let report = fresh.lookup(&wanted, FieldSet::DEFAULT).await;
    assert_eq!(served(&report), [(true, Served::Live)]);
    assert_eq!(title(&report), Some("Known (corrected)"));
    assert_eq!(provider.calls(), 2, "asked though the cache was fresh");

    // Its answer replaced the cached one.
    let later = engine.lookup(&wanted, FieldSet::DEFAULT).await;
    assert_eq!(served(&later), [(true, Served::Cache)]);
    assert_eq!(title(&later), Some("Known (corrected)"));
    assert_eq!(provider.calls(), 2);
}

#[tokio::test]
async fn a_fresh_lookup_asks_again_after_not_found_and_a_found_work_replaces_the_miss() {
    let (engine, provider) = setup();
    let wanted = ids(&["10.1000/late"]);
    engine.lookup(&wanted, FieldSet::DEFAULT).await;
    provider.registered.store(true, Ordering::SeqCst);
    let cached = engine.lookup(&wanted, FieldSet::DEFAULT).await;
    assert_eq!(served(&cached), [(false, Served::Cache)]);

    let fresh = engine.with_options(CallOptions::fresh());
    let report = fresh.lookup(&wanted, FieldSet::DEFAULT).await;
    assert_eq!(served(&report), [(true, Served::Live)]);
    assert_eq!(provider.calls(), 2);

    let later = engine.lookup(&wanted, FieldSet::DEFAULT).await;
    assert_eq!(served(&later), [(true, Served::Cache)]);
    assert_eq!(title(&later), Some("Late"));
    assert_eq!(provider.calls(), 2);
}

#[tokio::test]
async fn a_fresh_search_asks_past_the_cache_and_replaces_its_page() {
    let (engine, provider) = setup();
    let query = SearchQuery::new("known");
    engine.search(None, &query).await.unwrap();
    provider.corrected.store(true, Ordering::SeqCst);

    let fresh = engine.with_options(CallOptions::fresh());
    let report = fresh.search(None, &query).await.unwrap();
    assert_eq!(report.served, Served::Live);
    assert_eq!(provider.calls(), 2);

    let later = engine.search(None, &query).await.unwrap();
    assert_eq!(later.served, Served::Cache);
    let title = later.page.hits[0].record.work.title.as_deref();
    assert_eq!(title, Some("Known (corrected)"));
    assert_eq!(provider.calls(), 2);
}

/// Past the cache, a provider failing is no worse than asking normally:
/// the cached answer, though within its lifetime, is served marked stale
/// (it is not what was asked for: an answer from now).
#[tokio::test]
async fn a_fresh_call_whose_provider_fails_serves_the_cached_answer_as_stale() {
    let (engine, provider) = setup();
    let wanted = ids(&["10.1000/known", "10.1000/invented"]);
    engine.lookup(&wanted, FieldSet::DEFAULT).await;
    let query = SearchQuery::new("known");
    engine.search(None, &query).await.unwrap();

    provider.down.store(true, Ordering::SeqCst);
    let fresh = engine.with_options(CallOptions::fresh());
    let report = fresh
        .lookup(
            &ids(&["10.1000/known", "10.1000/invented", "10.1000/never-seen"]),
            FieldSet::DEFAULT,
        )
        .await;
    assert_eq!(
        served(&report),
        [
            (true, Served::Stale),
            (false, Served::Stale),
            (false, Served::Live)
        ]
    );
    assert!(report.results[1].is_not_found());
    assert!(matches!(
        report.results[2].attempts[0].outcome,
        Outcome::Failed(Error::AuthRejected { .. })
    ));
    assert_eq!(provider.calls(), 1 + 1 + 1, "asked once more, not served");

    let search = fresh.search(None, &query).await.unwrap();
    assert_eq!(search.served, Served::Stale);
    assert_eq!(search.page.hits.len(), 1);
}

#[tokio::test]
async fn offline_wins_over_fresh() {
    let (engine, provider) = setup();
    let wanted = ids(&["10.1000/known"]);
    engine.lookup(&wanted, FieldSet::DEFAULT).await;
    let mut options = CallOptions::offline();
    options.fresh = true;
    let report = engine
        .with_options(options)
        .lookup(&wanted, FieldSet::DEFAULT)
        .await;
    assert_eq!(served(&report), [(true, Served::Cache)]);
    assert_eq!(provider.calls(), 1);
}

#[tokio::test]
async fn a_fresh_verify_runs_its_searches_again() {
    use aspero_core::Claim;
    use common::{Scripted, deep_learning};

    let openalex = Arc::new(
        Scripted::new("openalex")
            .keyword()
            .with(vec![deep_learning()]),
    );
    let crossref = Arc::new(Scripted::new("crossref").matcher(vec![deep_learning()]));
    let engine = Engine::builder()
        .provider(openalex.clone(), ResiliencePolicy::default(), 0)
        .provider(crossref.clone(), ResiliencePolicy::default(), 1)
        .cache(Arc::new(SqliteCache::open_in_memory().unwrap()))
        .build();
    let claim = Claim::from_text("LeCun Y, Bengio Y, Hinton G. Deep learning. 2015.");
    let searches = || openalex.queries().len() + crossref.queries().len();

    let first = engine.verify(&claim, 5).await;
    let asked = searches();
    assert!(asked > 0);
    let again = engine.verify(&claim, 5).await;
    assert_eq!(searches(), asked, "searches served from cache");
    assert_eq!(again.verdict, first.verdict);

    let fresh = engine
        .with_options(CallOptions::fresh())
        .verify(&claim, 5)
        .await;
    assert_eq!(searches(), 2 * asked, "every search asked again");
    assert_eq!(fresh.verdict, first.verdict);
}
