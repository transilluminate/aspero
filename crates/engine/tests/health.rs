//! A long-lived engine remembers providers that can't answer: a spent
//! quota until it resets, rejected credentials for good, and repeated
//! failures for a cooldown (the circuit breaker).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aspero_core::{
    Capabilities, Error, FieldSet, IdKind, Lookup, Provider, ProviderId, ResiliencePolicy,
    Response, Result, WorkId,
};
use aspero_engine::{Engine, Outcome};
use async_trait::async_trait;
use jiff::Timestamp;

/// Fails with each scripted error in turn, then answers "not found".
struct Failing {
    caps: Capabilities,
    errors: Mutex<VecDeque<Error>>,
    calls: AtomicUsize,
}

impl Failing {
    fn new(errors: Vec<Error>) -> Arc<Self> {
        let mut caps = Capabilities::default();
        caps.lookup_kinds = vec![IdKind::Doi];
        caps.max_batch = 10;
        Arc::new(Failing {
            caps,
            errors: Mutex::new(errors.into()),
            calls: AtomicUsize::new(0),
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Provider for Failing {
    fn id(&self) -> ProviderId {
        ProviderId::from_static("failing")
    }

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    async fn lookup(&self, ids: &[WorkId], _: FieldSet) -> Result<Response<Vec<Lookup>>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(e) = self.errors.lock().unwrap().pop_front() {
            return Err(e);
        }
        let misses = ids
            .iter()
            .map(|id| Lookup {
                id: id.clone(),
                record: None,
            })
            .collect();
        Ok(Response::new(misses))
    }
}

fn engine(provider: Arc<Failing>) -> Engine {
    let mut policy = ResiliencePolicy::default();
    policy.max_retries = 0;
    policy.breaker_threshold = 2;
    policy.breaker_cooldown = Duration::from_secs(60);
    Engine::builder()
        .provider(provider, policy, 0)
        .clock(|| Timestamp::UNIX_EPOCH)
        .build()
}

async fn outcome(engine: &Engine) -> Outcome {
    let id: WorkId = "doi:10.1234/x".parse().unwrap();
    let report = engine.lookup(&[id], FieldSet::DEFAULT).await;
    report.results[0].attempts[0].outcome.clone()
}

fn server_error() -> Error {
    Error::Server {
        status: 503,
        message: None,
    }
}

#[tokio::test(start_paused = true)]
async fn a_rejected_key_is_not_tried_again() {
    let failing = Failing::new(vec![Error::AuthRejected { message: None }]);
    let engine = engine(failing.clone());
    for _ in 0..3 {
        assert!(matches!(
            outcome(&engine).await,
            Outcome::Failed(Error::AuthRejected { .. })
        ));
    }
    assert_eq!(failing.calls(), 1);
    assert!(matches!(
        engine.unavailable()[..],
        [(_, Error::AuthRejected { .. })]
    ));
}

#[tokio::test(start_paused = true)]
async fn a_spent_quota_is_waited_out_until_it_resets() {
    let resets_at = Timestamp::UNIX_EPOCH + jiff::SignedDuration::from_secs(600);
    let failing = Failing::new(vec![Error::QuotaExhausted {
        resets_at: Some(resets_at),
    }]);
    let engine = engine(failing.clone());
    outcome(&engine).await;
    assert!(matches!(
        outcome(&engine).await,
        Outcome::Failed(Error::QuotaExhausted { .. })
    ));
    assert_eq!(failing.calls(), 1);
    tokio::time::advance(Duration::from_secs(601)).await;
    assert!(matches!(outcome(&engine).await, Outcome::NotFound));
    assert!(engine.unavailable().is_empty());
}

#[tokio::test(start_paused = true)]
async fn repeated_failures_open_the_breaker_until_a_probe_succeeds() {
    let failing = Failing::new(vec![server_error(), server_error(), server_error()]);
    let engine = engine(failing.clone());
    outcome(&engine).await;
    outcome(&engine).await;
    // Open: answered without asking.
    assert!(matches!(
        outcome(&engine).await,
        Outcome::Failed(Error::CircuitOpen { until: Some(_) })
    ));
    assert_eq!(failing.calls(), 2);

    // After the cooldown one probe goes through; it fails, so wait again.
    tokio::time::advance(Duration::from_secs(61)).await;
    outcome(&engine).await;
    assert_eq!(failing.calls(), 3);
    assert!(matches!(
        outcome(&engine).await,
        Outcome::Failed(Error::CircuitOpen { .. })
    ));

    // The next probe succeeds and closes it.
    tokio::time::advance(Duration::from_secs(61)).await;
    assert!(matches!(outcome(&engine).await, Outcome::NotFound));
    assert!(matches!(outcome(&engine).await, Outcome::NotFound));
    assert_eq!(failing.calls(), 5);
}

#[tokio::test(start_paused = true)]
async fn views_share_what_the_engine_remembers() {
    let failing = Failing::new(vec![Error::AuthRejected { message: None }]);
    let engine = engine(failing.clone());
    outcome(&engine).await;
    let interactive = engine.with_options(aspero_engine::CallOptions::interactive());
    outcome(&interactive).await;
    assert_eq!(failing.calls(), 1);
}
