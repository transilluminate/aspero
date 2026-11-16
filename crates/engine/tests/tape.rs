//! The HTTP tape under a whole engine: a second run asks the network
//! nothing, costs nothing and moves no budget; replay-only fails a miss.
#![cfg(feature = "openalex")]

use std::sync::Arc;

use aspero_core::testing::{FIXTURE_TIME, ReplayClient, fixture};
use aspero_core::{FieldSet, HttpResponse, ResiliencePolicy, SearchQuery, WorkId};
use aspero_engine::transport::{HttpTape, TapeClient, TapeMode};
use aspero_engine::{Engine, OpenAlexConfig, Outcome, SecretString, SqliteTape};
use aspero_openalex::OpenAlex;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../providers/openalex/tests/fixtures"
);

const KEY: &str = "oa-test-key-0123456789";

fn recorded(name: &str) -> HttpResponse {
    fixture(FIXTURES, name)
}

/// A keyed OpenAlex behind the tape, no answer cache: everything it
/// learns comes through the tape.
fn engine(tape: &HttpTape, responses: Vec<HttpResponse>) -> (Engine, Arc<ReplayClient>) {
    let http = Arc::new(ReplayClient::new(responses));
    let taped = Arc::new(TapeClient::new("openalex", http.clone(), tape));
    let mut config = OpenAlexConfig::default();
    config.api_key = Some(SecretString::from(KEY.to_owned()));
    let mut policy = ResiliencePolicy::default();
    policy.max_retries = 0;
    let engine = Engine::builder()
        .provider(Arc::new(OpenAlex::new(config, taped)), policy, 10)
        .clock(|| FIXTURE_TIME)
        .build();
    (engine, http)
}

fn ids() -> [WorkId; 2] {
    [
        "10.1038/nature14539".parse().unwrap(),
        "10.9999/not-a-work".parse().unwrap(),
    ]
}

/// One run: a found lookup, a "not found", and a search; their outcomes
/// and the search's cost.
async fn run(engine: &Engine) -> (Vec<String>, Vec<String>, Option<f64>) {
    let mut outcomes = Vec::new();
    for id in ids() {
        let report = engine.lookup(&[id], FieldSet::DEFAULT).await;
        outcomes.push(match &report.results[0].attempts[0].outcome {
            Outcome::Found => "found".to_owned(),
            Outcome::NotFound => "not found".to_owned(),
            other => format!("{other:?}"),
        });
    }
    let search = engine
        .search(Some("openalex"), &SearchQuery::new("deep learning"))
        .await
        .unwrap();
    let titles = search
        .page
        .hits
        .iter()
        .map(|h| h.record.work.title.clone().unwrap_or_default())
        .collect();
    (outcomes, titles, search.usage.and_then(|u| u.cost))
}

#[tokio::test]
async fn a_second_run_is_replayed_free() {
    let tape = HttpTape::new(
        Arc::new(SqliteTape::open_in_memory().unwrap()),
        TapeMode::Record,
    );

    let (first, http) = engine(
        &tape,
        vec![
            recorded("work_by_doi"),
            recorded("work_not_found"),
            recorded("works_search"),
        ],
    );
    let (outcomes, titles, cost) = run(&first).await;
    assert_eq!(outcomes, ["found", "not found"]);
    assert!(!titles.is_empty());
    assert_eq!(cost, Some(0.001), "a recorded miss costs as usual");
    assert_eq!(http.requests().len(), 3);
    assert!(!first.budgets().is_empty(), "the live budget is noted");
    assert_eq!(tape.tape.entries().unwrap(), 3);

    // A new engine on the same tape: nothing queued, so any request sent
    // would panic.
    let (second, http) = engine(&tape, Vec::new());
    let replayed = run(&second).await;
    assert_eq!(http.requests().len(), 0, "zero inner calls");
    assert_eq!(
        replayed,
        (outcomes, titles, Some(0.0)),
        "same answers, free"
    );
    assert!(second.budgets().is_empty(), "a replay reports no budget");

    // Replay-only: what's on the tape is answered, a miss fails unsent.
    let offline = HttpTape::new(tape.tape.clone(), TapeMode::ReplayOnly);
    let (third, http) = engine(&offline, Vec::new());
    assert_eq!(run(&third).await.0, ["found", "not found"]);
    let other: WorkId = "10.1000/elsewhere".parse().unwrap();
    let report = third.lookup(&[other], FieldSet::DEFAULT).await;
    assert!(
        matches!(&report.results[0].attempts[0].outcome, Outcome::Failed(e) if e.kind() == aspero_core::ErrorKind::NotCached),
        "{:?}",
        report.results[0].attempts[0].outcome
    );
    assert_eq!(http.requests().len(), 0);
}
