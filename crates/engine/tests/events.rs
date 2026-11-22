//! The diagnostics events (DESIGN "Diagnostics events"): their names and
//! fields, the contract an embedder's summariser parses, and that nothing
//! asked reaches an info-level event.

mod common;

use std::sync::Arc;
use std::time::Duration;

use aspero_core::Claim;
use aspero_core::testing::{Captured, Events};
use aspero_engine::CallId;
use common::{Scripted, engine};
use futures::StreamExt;
use tracing::Level;

/// Whether an event says `text`, in its name or any field.
fn says(e: &Captured, text: &str) -> bool {
    e.name.contains(text) || e.fields.iter().any(|(_, v)| v.contains(text))
}

/// aspero's events at info and above, and at debug.
fn by_level(events: &Events) -> (Vec<Captured>, Vec<Captured>) {
    events
        .all()
        .into_iter()
        .filter(|e| e.target.starts_with("aspero"))
        .partition(|e| e.level <= Level::INFO)
}

fn fields<'a>(e: &'a Captured, names: &[&str]) -> Vec<&'a str> {
    names.iter().map(|n| e.field(n).unwrap_or("-")).collect()
}

/// Why a request failed or has no code, as `error` says (DESIGN
/// "Diagnostics events"), and the error kinds a test here meets.
const ERRORS: [&str; 13] = [
    "timeout",
    "dns",
    "refused",
    "reset",
    "connect",
    "body",
    "network",
    "cancelled",
    "offline",
    "server",
    "not_found",
    "rate_limited",
    "not_cached",
];

/// A contract event's schema (`v=2`): `v` first, `status` an HTTP code
/// or absent, `error` a kind or absent.
fn contract(e: &Captured) {
    assert_eq!(e.names().first(), Some(&"v"), "{e:?}");
    assert_eq!(e.field("v"), Some("2"), "{e:?}");
    if let Some(status) = e.field("status") {
        assert!(status.parse::<u16>().is_ok(), "status {status}: {e:?}");
    }
    if let Some(error) = e.field("error") {
        assert!(ERRORS.contains(&error), "error {error}: {e:?}");
    }
}

/// Every contract event captured, checked ([`contract`]).
fn all_contract(events: &Events) {
    let all = events.all();
    let contract_events = all.iter().filter(|e| {
        matches!(
            (e.target.as_str(), e.name.as_str()),
            ("aspero::net", "request" | "retry")
                | ("aspero::cache", "request")
                | ("aspero::engine", "call")
        )
    });
    for e in contract_events {
        contract(e);
    }
}

const CALL: [&str; 9] = [
    "v",
    "op",
    "items",
    "patience",
    "fresh",
    "providers",
    "ms",
    "outcome",
    "call",
];

/// The `call` (id) of each event.
fn ids(events: &[Captured]) -> Vec<u64> {
    events
        .iter()
        .map(|e| e.field("call").expect("a call id").parse().unwrap())
        .collect()
}

#[tokio::test]
async fn an_engine_call_says_its_op_items_providers_and_outcome_but_not_its_claim() {
    let (events, _guard) = Events::install();
    let crossref = Arc::new(Scripted::new("crossref").keyword());
    let engine = engine(vec![crossref]);
    let claim = Claim::from_text("Lactate clearance in septic shock. Crit Care. 2019.");
    engine.verify(&claim, 5).await;

    let calls = events.named("aspero::engine", "call");
    assert_eq!(calls.len(), 1, "one event, its parts not counted again");
    let call = &calls[0];
    assert_eq!(call.names(), CALL);
    all_contract(&events);
    assert_eq!(
        fields(
            call,
            &["op", "items", "patience", "fresh", "providers", "outcome"]
        ),
        ["verify", "1", "batch", "false", "crossref", "ok"]
    );
    let (info, debug) = by_level(&events);
    for text in ["Lactate", "septic"] {
        assert!(!info.iter().any(|e| says(e, text)), "{text} at info");
    }
    assert!(
        debug
            .iter()
            .any(|e| e.name == "call_input" && says(e, "Lactate"))
    );
}

/// A caller can choose a call's id beforehand ([`CallId`]), so it can
/// log the same number; calls made outside one take new ids.
#[tokio::test]
async fn a_call_given_an_id_says_it() {
    let (events, _guard) = Events::install();
    let crossref = Arc::new(Scripted::new("crossref").keyword());
    let engine = engine(vec![crossref]);
    let claim = Claim::from_text("Lactate clearance in septic shock. Crit Care. 2019.");
    let id = CallId::new();
    assert!(!id.taken());
    id.scope(engine.verify(&claim, 5)).await;
    assert!(id.taken());
    engine.verify(&claim, 5).await;

    let calls = events.named("aspero::engine", "call");
    let said = ids(&calls);
    assert_eq!(said[0], id.get());
    assert!(said[1] > id.get(), "a new id: {said:?}");
    assert!(calls.iter().all(|e| e.field("parent").is_none()));
    let inputs: Vec<_> = events.named("aspero::engine", "call_input");
    assert_eq!(ids(&inputs), said, "the debug companion says it too");
}

/// Each claim of a batch is a call, and so is its prefetch: each with an
/// id of its own and the batch's as its `parent`, so a batch reads as a
/// tree. One claim is the call itself; a check's verifies are its parts.
#[tokio::test]
async fn a_batch_reads_as_a_tree() {
    let (events, _guard) = Events::install();
    let (engine, _, _) = common::live_like();
    let claims = [
        Claim::from_text("Attention is all you need. NeurIPS. 2017. arXiv:1706.03762"),
        Claim::from_text("LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015."),
    ];
    let batch = CallId::new();
    let done: Vec<_> = batch
        .scope(async { engine.verify_batch(&claims, 5).collect::<Vec<_>>().await })
        .await;
    assert_eq!(done.len(), 2);

    let calls = events.named("aspero::engine", "call");
    let ops: Vec<_> = calls.iter().map(|e| e.field("op").unwrap()).collect();
    assert_eq!(ops, ["lookup", "verify", "verify"], "the prefetch first");
    let parent = batch.get().to_string();
    assert!(calls.iter().all(|e| e.field("parent") == Some(&parent)));
    let mut own = ids(&calls);
    own.sort_unstable();
    own.dedup();
    assert_eq!(own.len(), 3, "an id each");
    assert!(!own.contains(&batch.get()), "not the batch's");
    for e in &calls {
        assert_eq!(e.names(), [&CALL[..], &["parent"]].concat());
    }

    // A batch of one is its claim's call.
    let one = CallId::new();
    one.scope(async { engine.verify_batch(&claims[..1], 5).count().await })
        .await;
    let last = events.named("aspero::engine", "call").pop().unwrap();
    assert_eq!(ids(std::slice::from_ref(&last)), [one.get()]);
    assert_eq!(last.field("parent"), None);

    // A check's verifies are its parts: one event, no parent.
    let before = events.named("aspero::engine", "call").len();
    engine.check(&claims, 5, |_| {}).await;
    let checked = &events.named("aspero::engine", "call")[before..];
    assert_eq!(checked.len(), 1, "{checked:?}");
    assert_eq!(checked[0].field("op"), Some("check"));
    assert_eq!(checked[0].field("parent"), None);
}

#[tokio::test(start_paused = true)]
async fn a_call_dropped_before_it_ends_is_cancelled() {
    let (events, _guard) = Events::install();
    let slow = Arc::new(
        Scripted::new("crossref")
            .keyword()
            .slow(Duration::from_secs(60)),
    );
    let engine = engine(vec![slow]).with_options(aspero_engine::CallOptions::interactive());
    let query = aspero_core::SearchQuery::new("lactate");
    let stopped = tokio::time::timeout(
        Duration::from_secs(1),
        engine.search(Some("crossref"), &query),
    )
    .await;
    assert!(stopped.is_err());
    let calls = events.named("aspero::engine", "call");
    assert_eq!(
        fields(&calls[0], &["op", "patience", "providers", "ms", "outcome"]),
        ["search", "interactive", "crossref", "1000", "cancelled"]
    );
    assert_eq!(calls[0].field("error"), None, "cancelled is no error");
    all_contract(&events);
}

#[cfg(feature = "openalex")]
mod openalex {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use aspero_core::testing::{FIXTURE_TIME, fixture};
    use aspero_core::{
        Error, FieldSet, HttpClient, HttpRequest, HttpResponse, ResiliencePolicy, SearchQuery,
        WorkId,
    };
    use aspero_engine::transport::Paced;
    use aspero_engine::{Engine, OpenAlexConfig, SecretString, SqliteCache};
    use aspero_openalex::OpenAlex;
    use async_trait::async_trait;

    use super::*;

    const FIXTURES: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../providers/openalex/tests/fixtures"
    );

    /// Answers in turn: a timeout, then recorded responses.
    struct Flaky(Mutex<VecDeque<Result<HttpResponse, Error>>>);

    #[async_trait]
    impl HttpClient for Flaky {
        async fn send(&self, _: HttpRequest) -> Result<HttpResponse, Error> {
            self.0
                .lock()
                .unwrap()
                .pop_front()
                .expect("an answer queued")
        }
    }

    fn openalex() -> Engine {
        openalex_answering([
            Err(Error::Timeout),
            Ok(fixture(FIXTURES, "works_search")),
            Ok(fixture(FIXTURES, "work_by_doi")),
        ])
    }

    fn openalex_answering(
        answers: impl IntoIterator<Item = Result<HttpResponse, Error>>,
    ) -> Engine {
        let http = Arc::new(Flaky(Mutex::new(answers.into_iter().collect())));
        let paced = Arc::new(Paced::new("openalex", http, Duration::ZERO));
        let mut config = OpenAlexConfig::default();
        config.api_key = Some(SecretString::from("oa-test-key-0123456789".to_owned()));
        let mut policy = ResiliencePolicy::default();
        policy.max_retries = 1;
        Engine::builder()
            .provider(Arc::new(OpenAlex::new(config, paced)), policy, 10)
            .cache(Arc::new(SqliteCache::open_in_memory().unwrap()))
            .clock(|| FIXTURE_TIME)
            .build()
    }

    const REQUEST: [&str; 10] = [
        "v",
        "provider",
        "op",
        "status",
        "error",
        "ms",
        "waited_ms",
        "attempt",
        "cache",
        "cost_usd",
    ];

    #[tokio::test(start_paused = true)]
    async fn requests_retries_and_cache_hits_are_logged_without_what_was_asked() {
        let (events, _guard) = Events::install();
        let engine = openalex();
        let query = SearchQuery::new("deep learning");
        engine.search(Some("openalex"), &query).await.unwrap();
        engine.search(Some("openalex"), &query).await.unwrap();
        let doi: WorkId = "10.1038/nature14539".parse().unwrap();
        engine.lookup(&[doi], FieldSet::DEFAULT).await;

        let live = events.named("aspero::net", "request");
        let said: Vec<Vec<&str>> = live.iter().map(|e| fields(e, &REQUEST)).collect();
        assert_eq!(
            said,
            [
                [
                    "2", "openalex", "search", "-", "timeout", "0", "0", "1", "miss", "-"
                ],
                [
                    "2", "openalex", "search", "200", "-", "0", "0", "2", "miss", "0.001000"
                ],
                [
                    "2", "openalex", "lookup", "200", "-", "0", "0", "1", "miss", "0.000000"
                ],
            ]
        );
        assert!(live.iter().all(|e| e.field("detached").is_none()));
        let retry = events.named("aspero::net", "retry");
        assert_eq!(
            fields(
                &retry[0],
                &["v", "provider", "op", "attempt", "status", "error"]
            ),
            ["2", "openalex", "search", "1", "-", "timeout"]
        );

        let hits = events.named("aspero::cache", "request");
        assert_eq!(hits.len(), 1);
        assert_eq!(
            fields(&hits[0], &[&REQUEST[..9], &["items"]].concat()),
            [
                "2", "openalex", "search", "-", "-", "0", "0", "0", "hit", "1"
            ]
        );
        assert_eq!(
            hits[0].names(),
            [
                "v",
                "provider",
                "op",
                "ms",
                "waited_ms",
                "attempt",
                "cache",
                "items",
                "call"
            ],
            "no status or error for a hit"
        );
        all_contract(&events);

        let calls = events.named("aspero::engine", "call");
        let said: Vec<Vec<&str>> = calls
            .iter()
            .map(|e| fields(e, &["op", "items", "providers", "outcome"]))
            .collect();
        assert_eq!(
            said,
            [
                ["search", "1", "openalex", "ok"],
                ["search", "1", "none", "ok"],
                ["lookup", "1", "openalex", "ok"],
            ]
        );

        let (info, debug) = by_level(&events);
        for text in ["deep", "nature14539", "10.1038"] {
            assert!(!info.iter().any(|e| says(e, text)), "{text} at info");
        }
        let urls: Vec<&str> = debug
            .iter()
            .filter(|e| e.name == "request_url")
            .filter_map(|e| e.field("url"))
            .collect();
        assert_eq!(urls.len(), 3);
        assert!(urls[1].contains("deep"), "{urls:?}");
    }

    /// Everything done for a call carries its id, last: its requests (a
    /// failed attempt, the retry, the one answered), the cache's answers
    /// and the debug companions.
    #[tokio::test(start_paused = true)]
    async fn every_event_of_a_call_carries_its_id() {
        let (events, _guard) = Events::install();
        let engine = openalex();
        let query = SearchQuery::new("deep learning");
        engine.search(Some("openalex"), &query).await.unwrap();
        engine.search(Some("openalex"), &query).await.unwrap();
        let doi: WorkId = "10.1038/nature14539".parse().unwrap();
        engine.lookup(&[doi], FieldSet::DEFAULT).await;

        let calls = ids(&events.named("aspero::engine", "call"));
        let [searched, cached, looked] = calls[..] else {
            panic!("three calls: {calls:?}");
        };
        assert!(searched < cached && cached < looked, "{calls:?}");
        let live = events.named("aspero::net", "request");
        assert_eq!(ids(&live), [searched, searched, looked]);
        for e in &live {
            assert_eq!(e.names().last(), Some(&"call"), "last: {:?}", e.names());
        }
        assert_eq!(ids(&events.named("aspero::net", "retry")), [searched]);
        let hits = events.named("aspero::cache", "request");
        assert_eq!(ids(&hits), [cached]);
        assert_eq!(hits[0].names().last(), Some(&"call"));
        let urls = events.named("aspero::net", "request_url");
        assert_eq!(ids(&urls), [searched, searched, looked]);
        let inputs = events.named("aspero::engine", "call_input");
        assert_eq!(ids(&inputs), [searched, cached, looked]);
    }

    /// An HTTP error response says its code and its kind, both: the
    /// `request` the code's (core's `status_error`), the `retry` the
    /// provider's reading of it. `error` follows `status`.
    #[tokio::test(start_paused = true)]
    async fn an_http_error_says_its_status_and_its_kind() {
        let (events, _guard) = Events::install();
        let engine = openalex_answering([
            Ok(aspero_core::testing::response(503, &[], "busy")),
            Ok(fixture(FIXTURES, "works_search")),
        ]);
        let query = SearchQuery::new("deep learning");
        engine.search(Some("openalex"), &query).await.unwrap();

        let live = events.named("aspero::net", "request");
        assert_eq!(
            fields(
                &live[0],
                &["v", "provider", "op", "status", "error", "attempt"]
            ),
            ["2", "openalex", "search", "503", "server", "1"]
        );
        let names = live[0].names();
        let at = |n| names.iter().position(|x| *x == n).unwrap();
        assert_eq!(at("error"), at("status") + 1, "{names:?}");
        assert_eq!(live[1].field("error"), None, "none on success");
        let retry = events.named("aspero::net", "retry");
        assert_eq!(
            retry[0].names(),
            [
                "v", "provider", "op", "attempt", "status", "error", "delay_ms", "call"
            ]
        );
        assert_eq!(fields(&retry[0], &["status", "error"]), ["503", "server"]);
        all_contract(&events);
    }
}

/// A request dropped in flight is `error=cancelled`, no `status`, and says
/// why, last
/// (#156): its deadline passed, its call was done with it (a verify found
/// its verdict), or its caller stopped waiting.
#[cfg(feature = "crossref")]
mod cancelled {
    use aspero_core::{Error, HttpClient, HttpRequest, HttpResponse, ResiliencePolicy};
    use aspero_crossref::{Crossref, CrossrefConfig};
    use aspero_engine::Engine;
    use aspero_engine::transport::Paced;
    use async_trait::async_trait;

    use super::*;

    /// Never answers.
    struct Hang;

    #[async_trait]
    impl HttpClient for Hang {
        async fn send(&self, _: HttpRequest) -> Result<HttpResponse, Error> {
            std::future::pending().await
        }
    }

    /// Crossref (free, so its calls run in place), never answering, after
    /// `first`.
    fn engine(first: Option<Arc<Scripted>>, deadline: Duration) -> Engine {
        let paced = Arc::new(Paced::new("crossref", Arc::new(Hang), Duration::ZERO));
        let crossref = Arc::new(Crossref::new(CrossrefConfig::default(), paced));
        let mut policy = ResiliencePolicy::default();
        policy.deadline = deadline;
        policy.max_retries = 0;
        let mut builder = Engine::builder();
        if let Some(first) = first {
            builder = builder.provider(first, ResiliencePolicy::default(), 0);
        }
        builder.provider(crossref, policy, 1).build()
    }

    /// The cancelled requests' `why`, checking it's said last.
    fn whys(events: &Events) -> Vec<String> {
        events
            .named("aspero::net", "request")
            .iter()
            .filter(|e| e.field("error") == Some("cancelled"))
            .map(|e| {
                contract(e);
                assert_eq!(e.field("status"), None, "no code: {e:?}");
                assert_eq!(e.names().last(), Some(&"why"), "{:?}", e.names());
                e.field("why").unwrap().to_owned()
            })
            .collect()
    }

    #[tokio::test(start_paused = true)]
    async fn a_request_past_its_deadline_says_deadline() {
        let (events, _guard) = Events::install();
        let engine = engine(None, Duration::from_secs(2));
        let query = aspero_core::SearchQuery::new("lactate");
        let result = engine.search(Some("crossref"), &query).await;
        assert!(result.is_err());
        assert_eq!(whys(&events), ["deadline"]);
        // The call failed: `outcome=error`, its kind in `error`, after it.
        let calls = events.named("aspero::engine", "call");
        assert_eq!(
            fields(&calls[0], &["outcome", "error"]),
            ["error", "timeout"]
        );
        let names = calls[0].names();
        let at = |n| names.iter().position(|x| *x == n).unwrap();
        assert_eq!(at("error"), at("outcome") + 1, "{names:?}");
        all_contract(&events);
    }

    #[tokio::test(start_paused = true)]
    async fn a_request_its_caller_stops_waiting_for_says_caller() {
        let (events, _guard) = Events::install();
        let engine = engine(None, Duration::from_secs(60));
        let query = aspero_core::SearchQuery::new("lactate");
        let stopped = tokio::time::timeout(
            Duration::from_secs(1),
            engine.search(Some("crossref"), &query),
        )
        .await;
        assert!(stopped.is_err());
        assert_eq!(whys(&events), ["caller"]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_request_a_verdict_makes_needless_says_done() {
        let (events, _guard) = Events::install();
        // The DOI's lookup verifies the claim, while Crossref's citation
        // matcher is still out.
        let lookup = Arc::new(
            Scripted::new("openalex")
                .with(vec![common::deep_learning()])
                .slow_lookups(Duration::from_millis(100)),
        );
        let engine = engine(Some(lookup), Duration::from_secs(60));
        let claim = Claim::from_text(
            "LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015;521:436-444. \
             doi:10.1038/nature14539",
        );
        let v = engine.verify(&claim, 5).await;
        assert_eq!(v.verdict.as_str(), "verified", "{v:?}");
        assert!(!whys(&events).is_empty());
        assert!(
            whys(&events).iter().all(|w| w == "done"),
            "{:?}",
            whys(&events)
        );
        // A request that completes says no why.
        assert!(
            events
                .named("aspero::net", "request")
                .iter()
                .filter(|e| e.field("error") != Some("cancelled"))
                .all(|e| e.field("why").is_none())
        );
    }
}
