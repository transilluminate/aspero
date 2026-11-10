//! Engine caching: fresh hits skip providers, misses are remembered, expiry
//! refetches, and failures fall back to stale answers.

use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use aspero_core::{
    Capabilities, Error, FieldSet, Hit, IdKind, Lookup, Page, Provider, ProviderId, Raw,
    ResiliencePolicy, Response, Result, SearchQuery, SourceRecord, Work, WorkId,
};
use aspero_engine::{Engine, Outcome, Served, SqliteCache};
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

/// Knows one work (10.1000/known); counts calls; can be switched off.
struct Counting {
    caps: Capabilities,
    calls: AtomicUsize,
    down: AtomicBool,
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
            covers,
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn known(&self) -> SourceRecord {
        let mut work = Work::default();
        work.title = Some("Known".into());
        SourceRecord::new(
            ProviderId::from_static("counting"),
            vec!["10.1000/known".parse().unwrap()],
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
        let known = self.known();
        let lookups = ids
            .iter()
            .map(|id| Lookup {
                id: id.clone(),
                record: known.ids.contains(id).then(|| known.clone()),
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
