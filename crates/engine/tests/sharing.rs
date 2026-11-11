//! Callers sharing one engine: requests in flight are shared, a cancelled
//! caller doesn't strand the others, and each caller sets its own patience.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aspero_core::{
    Capabilities, Error, FieldSet, IdKind, Lookup, Provider, ProviderId, Raw, ResiliencePolicy,
    Response, Result, SourceRecord, Work, WorkId,
};
use aspero_engine::{CallOptions, Engine, Outcome, Served, SqliteCache};
use async_trait::async_trait;
use jiff::Timestamp;
use tokio::time::Instant;

/// Knows every DOI; answers after `delay`, recording each batch.
struct Slow {
    caps: Capabilities,
    delay: Duration,
    batches: Mutex<Vec<Vec<WorkId>>>,
    calls: AtomicUsize,
}

impl Slow {
    fn new(delay: Duration) -> Arc<Self> {
        let mut caps = Capabilities::default();
        caps.lookup_kinds = vec![IdKind::Doi];
        caps.max_batch = 10;
        Arc::new(Slow {
            caps,
            delay,
            batches: Mutex::default(),
            calls: AtomicUsize::new(0),
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Provider for Slow {
    fn id(&self) -> ProviderId {
        ProviderId::from_static("slow")
    }

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    async fn lookup(&self, ids: &[WorkId], _: FieldSet) -> Result<Response<Vec<Lookup>>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.batches.lock().unwrap().push(ids.to_vec());
        tokio::time::sleep(self.delay).await;
        let lookups = ids
            .iter()
            .map(|id| Lookup {
                id: id.clone(),
                record: Some(record(id)),
            })
            .collect();
        Ok(Response::new(lookups))
    }
}

fn record(id: &WorkId) -> SourceRecord {
    let mut work = Work::default();
    work.title = Some(format!("Work {id}"));
    SourceRecord::new(
        ProviderId::from_static("slow"),
        vec![id.clone()],
        work,
        FieldSet::ALL,
        Raw {
            media_type: "application/json".into(),
            body: "{}".into(),
        },
        Timestamp::UNIX_EPOCH,
    )
}

fn engine(provider: Arc<Slow>) -> Engine {
    Engine::builder()
        .provider(provider, ResiliencePolicy::default(), 0)
        .cache(Arc::new(SqliteCache::open_in_memory().unwrap()))
        .build()
}

fn ids(list: &[&str]) -> Vec<WorkId> {
    list.iter().map(|s| s.parse().unwrap()).collect()
}

#[tokio::test(start_paused = true)]
async fn concurrent_lookups_of_one_work_share_a_request() {
    let slow = Slow::new(Duration::from_secs(1));
    let engine = engine(slow.clone());
    let one = ids(&["10.1234/a"]);
    let both = ids(&["10.1234/a", "10.1234/b"]);
    let (first, second) = tokio::join!(
        engine.lookup(&one, FieldSet::DEFAULT),
        engine.lookup(&both, FieldSet::DEFAULT),
    );
    assert!(first.results[0].record.is_some());
    assert!(second.results.iter().all(|r| r.record.is_some()));
    let batches = slow.batches.lock().unwrap().clone();
    assert_eq!(batches, vec![ids(&["10.1234/a"]), ids(&["10.1234/b"])]);
}

#[tokio::test(start_paused = true)]
async fn a_repeated_identifier_is_asked_about_once() {
    let slow = Slow::new(Duration::from_secs(1));
    let engine = engine(slow.clone());
    let report = engine
        .lookup(&ids(&["10.1234/a", "10.1234/a"]), FieldSet::DEFAULT)
        .await;
    assert!(report.results.iter().all(|r| r.record.is_some()));
    assert_eq!(*slow.batches.lock().unwrap(), vec![ids(&["10.1234/a"])]);
}

#[tokio::test(start_paused = true)]
async fn a_cancelled_caller_does_not_strand_those_waiting_on_it() {
    let slow = Slow::new(Duration::from_secs(1));
    let engine = engine(slow.clone());
    let id = ids(&["10.1234/a"]);
    let leader = engine.lookup(&id, FieldSet::DEFAULT);
    let joiner = async {
        // Join once the leader's request is under way.
        tokio::time::sleep(Duration::from_millis(100)).await;
        engine.lookup(&id, FieldSet::DEFAULT).await
    };
    let (_, joined) = tokio::join!(
        tokio::time::timeout(Duration::from_millis(500), leader),
        joiner
    );
    assert!(joined.results[0].record.is_some());
    assert_eq!(slow.calls(), 2, "the joiner asked again itself");
}

#[tokio::test(start_paused = true)]
async fn interactive_callers_give_up_sooner() {
    let slow = Slow::new(Duration::from_secs(15));
    let engine = engine(slow.clone());
    let start = Instant::now();
    let report = engine
        .with_options(CallOptions::interactive())
        .lookup(&ids(&["10.1234/a"]), FieldSet::DEFAULT)
        .await;
    assert!(matches!(
        report.results[0].attempts[0].outcome,
        Outcome::Failed(Error::Timeout)
    ));
    assert!(start.elapsed() <= Duration::from_secs(8));
    // A batch caller waits it out.
    assert!(
        engine
            .lookup(&ids(&["10.1234/a"]), FieldSet::DEFAULT)
            .await
            .results[0]
            .record
            .is_some()
    );
}

#[tokio::test(start_paused = true)]
async fn offline_callers_answer_from_the_cache_only() {
    let slow = Slow::new(Duration::from_secs(1));
    let engine = engine(slow.clone());
    engine.lookup(&ids(&["10.1234/a"]), FieldSet::DEFAULT).await;
    let offline = engine.with_options(CallOptions::offline());
    let report = offline
        .lookup(&ids(&["10.1234/a", "10.1234/b"]), FieldSet::DEFAULT)
        .await;
    assert_eq!(report.results[0].attempts[0].served, Served::Cache);
    assert!(report.results[0].record.is_some());
    assert!(matches!(
        report.results[1].attempts[0].outcome,
        Outcome::Failed(Error::Offline)
    ));
    assert_eq!(slow.calls(), 1, "nothing asked while offline");
}
