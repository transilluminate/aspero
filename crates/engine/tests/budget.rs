//! Budgets: what the last responses said, and a free refresh with a key.
#![cfg(feature = "openalex")]

use std::sync::Arc;

use aspero_core::testing::{FIXTURE_TIME, ReplayClient, fixture};
use aspero_core::{FieldSet, HttpResponse, ResiliencePolicy, WorkId};
use aspero_engine::{Engine, OpenAlexConfig, SecretString};
use aspero_openalex::OpenAlex;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../providers/openalex/tests/fixtures"
);

fn recorded(name: &str) -> HttpResponse {
    fixture(FIXTURES, name)
}

fn engine(key: Option<&str>, responses: Vec<HttpResponse>) -> (Engine, Arc<ReplayClient>) {
    let http = Arc::new(ReplayClient::new(responses));
    let mut config = OpenAlexConfig::default();
    config.api_key = key.map(|k| SecretString::from(k.to_owned()));
    let mut policy = ResiliencePolicy::default();
    policy.max_retries = 0;
    let engine = Engine::builder()
        .provider(Arc::new(OpenAlex::new(config, http.clone())), policy, 10)
        .clock(|| FIXTURE_TIME)
        .build();
    (engine, http)
}

#[tokio::test]
async fn with_a_key_refreshing_asks_rate_limit_for_free() {
    let (engine, http) = engine(Some("test-key"), vec![recorded("rate_limit")]);
    assert!(engine.budgets().is_empty(), "nothing said yet");

    let budgets = engine.refresh_budgets().await;
    let requests = http.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].url, "https://api.openalex.org/rate-limit");
    let [(provider, budget)] = budgets.as_slice() else {
        panic!("one budget: {budgets:?}");
    };
    assert_eq!(provider.as_str(), "openalex");
    assert_eq!(budget.limit, Some(1.0));
    assert_eq!(budget.remaining, Some(0.9502));
    assert_eq!(budget.prepaid_remaining, Some(0.0));
    assert!((budget.used().unwrap() - 0.0498).abs() < 1e-9);
    assert_eq!(
        budget.resets_at,
        Some("2026-11-15T00:00:00Z".parse().unwrap())
    );
    assert_eq!(budget.seen_at, FIXTURE_TIME);
    assert_eq!(engine.budgets(), budgets, "remembered");
}

#[tokio::test]
async fn without_a_key_refreshing_sends_nothing() {
    let (engine, http) = engine(None, vec![recorded("work_by_doi")]);
    assert!(engine.refresh_budgets().await.is_empty());
    assert!(http.requests().is_empty(), "no request without a key");

    // A response's headers say the budget; refreshing keeps it as they
    // left it.
    let id: WorkId = "10.1038/nature14539".parse().unwrap();
    engine.lookup(&[id], FieldSet::DEFAULT).await;
    let budgets = engine.refresh_budgets().await;
    assert_eq!(http.requests().len(), 1, "only the lookup");
    let budget = budgets[0].1;
    assert_eq!(budget.limit, Some(1.0));
    assert_eq!(budget.remaining, Some(0.9989));
    assert_eq!(budget.prepaid_remaining, Some(0.0));
}

#[tokio::test]
async fn offline_refreshing_sends_nothing() {
    let (engine, http) = engine(Some("test-key"), vec![recorded("rate_limit")]);
    let offline = engine.with_options(aspero_engine::CallOptions::offline());
    assert!(offline.refresh_budgets().await.is_empty());
    assert!(http.requests().is_empty());
}
