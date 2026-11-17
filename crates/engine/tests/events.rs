//! The diagnostics events (DESIGN "Diagnostics events"): their names and
//! fields, the contract an embedder's summariser parses, and that nothing
//! asked reaches an info-level event.

mod common;

use std::sync::Arc;
use std::time::Duration;

use aspero_core::Claim;
use aspero_core::testing::{Captured, Events};
use common::{Scripted, engine};
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

const CALL: [&str; 7] = [
    "op",
    "items",
    "patience",
    "fresh",
    "providers",
    "ms",
    "outcome",
];

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
        let answers = [
            Err(Error::Timeout),
            Ok(fixture(FIXTURES, "works_search")),
            Ok(fixture(FIXTURES, "work_by_doi")),
        ];
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

    const REQUEST: [&str; 8] = [
        "provider",
        "op",
        "status",
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
                ["openalex", "search", "timeout", "0", "0", "1", "miss", "-"],
                [
                    "openalex", "search", "200", "0", "0", "2", "miss", "0.001000"
                ],
                [
                    "openalex", "lookup", "200", "0", "0", "1", "miss", "0.000000"
                ],
            ]
        );
        assert!(live.iter().all(|e| e.field("detached").is_none()));
        let retry = events.named("aspero::net", "retry");
        assert_eq!(
            fields(&retry[0], &["provider", "op", "attempt", "status"]),
            ["openalex", "search", "1", "timeout"]
        );

        let hits = events.named("aspero::cache", "request");
        assert_eq!(hits.len(), 1);
        assert_eq!(
            fields(&hits[0], &[&REQUEST[..7], &["items"]].concat()),
            ["openalex", "search", "none", "0", "0", "0", "hit", "1"]
        );

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
}
