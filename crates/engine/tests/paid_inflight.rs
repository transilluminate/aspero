//! A paid request once sent is finished and counted, even if its caller
//! stops waiting; one not yet sent never is (#139).
#![cfg(feature = "openalex")]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use aspero_core::testing::{Captured, Events, FIXTURE_TIME, fixture, response};
use aspero_core::{
    Error, ErrorKind, HttpClient, HttpRequest, HttpResponse, ProviderId, ResiliencePolicy,
    SearchQuery,
};
use aspero_engine::transport::{HttpTape, Paced, TapeClient, TapeMode};
use aspero_engine::{
    Engine, OpenAlexConfig, PrepaidDays, SecretString, Served, SqliteCache, SqliteTape,
};
use aspero_openalex::OpenAlex;
use async_trait::async_trait;
use tokio::time::Instant;
use tracing::subscriber::DefaultGuard;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../providers/openalex/tests/fixtures"
);

/// The budget day of [`FIXTURE_TIME`].
const DAY: &str = "2026-11-10";

/// OpenAlex answering after `delay`: a search's page charged $0.001 of
/// credit (the allowance gone), or (`status`) a failure; its budget, free
/// and at once. Records each request as it's sent.
struct Slow {
    delay: Duration,
    status: u16,
    sent: Mutex<Vec<String>>,
}

impl Slow {
    fn new(delay: Duration, status: u16) -> Arc<Self> {
        Arc::new(Slow {
            delay,
            status,
            sent: Mutex::default(),
        })
    }

    fn sent(&self) -> Vec<String> {
        self.sent.lock().unwrap().clone()
    }

    fn searches(&self) -> usize {
        self.sent().iter().filter(|u| u.contains("/works?")).count()
    }
}

#[async_trait]
impl HttpClient for Slow {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, Error> {
        self.sent.lock().unwrap().push(request.url.clone());
        if request.url.contains("/rate-limit") {
            let body = r#"{"rate_limit": {"daily_budget_usd": 1, "daily_used_usd": 1,
                "daily_remaining_usd": 0, "prepaid_remaining_usd": 5,
                "resets_in_seconds": 3600}}"#;
            return Ok(response(200, &[], body));
        }
        tokio::time::sleep(self.delay).await;
        let page = fixture(FIXTURES, "works_search");
        let headers = [
            ("content-type", "application/json"),
            ("x-ratelimit-cost-usd", "0.001"),
            ("x-ratelimit-limit-usd", "1"),
            ("x-ratelimit-remaining-usd", "0"),
            ("x-ratelimit-prepaid-remaining-usd", "4.999"),
            ("x-ratelimit-reset", "3600"),
        ]
        .map(|(k, v)| (k.to_owned(), v.to_owned()));
        let body = if self.status == 200 {
            page.body
        } else {
            Vec::new()
        };
        Ok(HttpResponse::new(
            self.status,
            headers.to_vec(),
            body,
            FIXTURE_TIME,
        ))
    }
}

struct Setup {
    engine: Engine,
    http: Arc<Slow>,
    store: Arc<SqliteCache>,
    tape: HttpTape,
    /// The test's events. Every test captures its own: a thread with no
    /// subscriber can leave a callsite cached as unwanted by another's.
    events: Events,
    _events: DefaultGuard,
}

/// A keyed OpenAlex over `http`, paced `interval` apart, behind a tape,
/// with an answer cache and the credit spent here kept in one store; a
/// credit cap of `credit_cap`, if any, and `retries`.
fn setup(http: Arc<Slow>, interval: Duration, credit_cap: Option<f64>, retries: u32) -> Setup {
    let (events, guard) = Events::install();
    let tape = HttpTape::new(
        Arc::new(SqliteTape::open_in_memory().unwrap()),
        TapeMode::Record,
    );
    let paced = Arc::new(Paced::new("openalex", http.clone(), interval));
    let taped = Arc::new(TapeClient::new("openalex", paced, &tape));
    let mut config = OpenAlexConfig::default();
    config.api_key = Some(SecretString::from("oa-test-key-0123456789".to_owned()));
    let mut policy = ResiliencePolicy::default();
    policy.max_retries = retries;
    let store = Arc::new(SqliteCache::open_in_memory().unwrap());
    let mut builder = Engine::builder()
        .provider(Arc::new(OpenAlex::new(config, taped)), policy, 10)
        .cache(store.clone())
        .prepaid_days(store.clone())
        .clock(|| FIXTURE_TIME);
    if let Some(cap) = credit_cap {
        builder = builder.daily_credit_cap(ProviderId::from_static("openalex"), cap);
    }
    Setup {
        engine: builder.build(),
        http,
        store,
        tape,
        events,
        _events: guard,
    }
}

fn query(text: &str) -> SearchQuery {
    SearchQuery::new(text)
}

#[tokio::test(start_paused = true)]
async fn a_search_sent_then_dropped_is_finished_counted_cached_and_taped() {
    let s = setup(
        Slow::new(Duration::from_secs(1), 200),
        Duration::ZERO,
        Some(0.001),
        0,
    );
    // The caller stops waiting mid-request.
    let q = query("deep learning");
    let search = s.engine.search(Some("openalex"), &q);
    let waited = tokio::time::timeout(Duration::from_millis(50), search).await;
    assert!(waited.is_err(), "dropped before the answer");
    assert_eq!(s.http.searches(), 1, "it was on the wire");
    assert_eq!(s.tape.tape.entries().unwrap(), 0, "not answered yet");

    // It finishes in the background; settling waits for it.
    let start = Instant::now();
    let usage = s.engine.settle(Duration::from_secs(10)).await;
    assert!(
        start.elapsed() <= Duration::from_secs(1),
        "not to the bound"
    );
    let [(provider, spent)] = usage.as_slice() else {
        panic!("one provider's usage: {usage:?}");
    };
    assert_eq!(provider.as_str(), "openalex");
    assert_eq!(spent.cost, Some(0.001));
    assert!(s.engine.settle(Duration::ZERO).await.is_empty(), "drained");

    // Counted as an awaited request is: the budget, the credit spent
    // here (in the store, which the credit cap counts).
    let budget = s.engine.budgets()[0].1;
    assert_eq!(budget.remaining, Some(0.0));
    assert_eq!(budget.credit_spent_today_here, Some(0.001));
    assert_eq!(s.store.credit_here("openalex", DAY).unwrap(), 0.001);
    // Taped, and cached: asked again, it's the cache's answer.
    assert_eq!(s.tape.tape.entries().unwrap(), 1);
    let again = s
        .engine
        .search(Some("openalex"), &query("deep learning"))
        .await
        .unwrap();
    assert_eq!(again.served, Served::Cache);
    assert_eq!(s.http.searches(), 1);
    // And the credit cap stops the next paid search, unsent.
    let capped = s
        .engine
        .search(Some("openalex"), &query("something else"))
        .await
        .unwrap_err();
    assert_eq!(capped.kind(), ErrorKind::BudgetCapped);
    assert_eq!(s.http.searches(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_search_still_waiting_for_its_slot_is_never_sent_once_dropped() {
    let s = setup(
        Slow::new(Duration::from_millis(100), 200),
        Duration::from_secs(1),
        None,
        0,
    );
    s.engine
        .search(Some("openalex"), &query("first"))
        .await
        .unwrap();
    // The next slot is a second after the first: dropped before it.
    let q = query("second");
    let second = s.engine.search(Some("openalex"), &q);
    assert!(
        tokio::time::timeout(Duration::from_millis(200), second)
            .await
            .is_err()
    );
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(s.engine.settle(Duration::from_secs(10)).await.is_empty());
    assert_eq!(s.http.searches(), 1, "{:?}", s.http.sent());
    assert_eq!(s.tape.tape.entries().unwrap(), 1);
    assert_eq!(s.store.credit_here("openalex", DAY).unwrap(), 0.001);
}

#[tokio::test(start_paused = true)]
async fn a_request_left_finishing_sends_no_retry() {
    // A server error, after a second: retried, if anyone still waited.
    let s = setup(
        Slow::new(Duration::from_secs(1), 503),
        Duration::ZERO,
        None,
        2,
    );
    let q = query("deep learning");
    let search = s.engine.search(Some("openalex"), &q);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), search)
            .await
            .is_err()
    );
    let start = Instant::now();
    assert!(s.engine.settle(Duration::from_secs(10)).await.is_empty());
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "the task ends at its retry"
    );
    tokio::time::sleep(Duration::from_secs(60)).await;
    assert_eq!(s.http.searches(), 1, "{:?}", s.http.sent());
    assert_eq!(s.tape.tape.entries().unwrap(), 0);
}

#[tokio::test(start_paused = true)]
async fn an_awaited_search_is_answered_as_before() {
    let s = setup(
        Slow::new(Duration::from_millis(100), 200),
        Duration::ZERO,
        None,
        0,
    );
    let report = s
        .engine
        .search(Some("openalex"), &query("deep learning"))
        .await
        .unwrap();
    assert_eq!(report.served, Served::Live);
    assert_eq!(report.usage.and_then(|u| u.cost), Some(0.001));
    assert!(!report.page.hits.is_empty());
    // Its usage is the report's, not left for settling.
    assert!(s.engine.settle(Duration::from_secs(1)).await.is_empty());
    assert_eq!(s.store.credit_here("openalex", DAY).unwrap(), 0.001);
}

/// DESIGN "Diagnostics events": a paid request finished after its caller
/// left says `detached=true`, and its caller's call was cancelled; one
/// awaited says nothing of it. Each keeps its call's id.
#[tokio::test(start_paused = true)]
async fn a_request_finished_after_its_caller_left_says_detached() {
    let s = setup(
        Slow::new(Duration::from_secs(1), 200),
        Duration::ZERO,
        None,
        0,
    );
    let left = tokio::time::timeout(
        Duration::from_millis(50),
        s.engine.search(Some("openalex"), &query("deep learning")),
    )
    .await;
    assert!(left.is_err());
    s.engine.settle(Duration::from_secs(10)).await;
    s.engine
        .search(Some("openalex"), &query("graph neural networks"))
        .await
        .unwrap();

    let searches: Vec<_> = s
        .events
        .named("aspero::net", "request")
        .into_iter()
        .filter(|e| e.field("op") == Some("search"))
        .collect();
    let detached: Vec<_> = searches.iter().map(|e| e.field("detached")).collect();
    assert_eq!(detached, [Some("true"), None]);
    let calls = s.events.named("aspero::engine", "call");
    let outcomes: Vec<_> = calls.iter().map(|e| e.field("outcome")).collect();
    assert_eq!(outcomes, [Some("cancelled"), Some("ok")]);
    // Finished in a task of its own, after its call's event: its call's id.
    let ids = |events: &[Captured]| -> Vec<Option<String>> {
        events
            .iter()
            .map(|e| e.field("call").map(str::to_owned))
            .collect()
    };
    let called = ids(&calls);
    assert!(called.iter().all(Option::is_some));
    assert_ne!(called[0], called[1]);
    assert_eq!(ids(&searches), called);
}
