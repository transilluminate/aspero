//! Independent work runs concurrently: title searches across providers,
//! and claims in a batch. Measured on paused tokio time, where a provider
//! that takes "one second" makes sequential work take N seconds.

use std::sync::Arc;
use std::time::Duration;

use aspero_core::{
    Capabilities, Claim, Page, Provider, ProviderId, ResiliencePolicy, Response, Result,
    SearchQuery,
};
use aspero_engine::Engine;
use async_trait::async_trait;
use tokio::time::Instant;

/// A search provider that answers (with nothing) after one second.
struct Slow {
    id: &'static str,
    caps: Capabilities,
}

impl Slow {
    fn new(id: &'static str, citation: bool) -> Arc<Self> {
        let mut caps = Capabilities::default();
        caps.search = true;
        caps.citation_search = citation;
        Arc::new(Slow { id, caps })
    }
}

#[async_trait]
impl Provider for Slow {
    fn id(&self) -> ProviderId {
        ProviderId::parse(self.id).unwrap()
    }

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    async fn search(&self, _: &SearchQuery) -> Result<Response<Page>> {
        tokio::time::sleep(Duration::from_secs(1)).await;
        Ok(Response::new(Page::default()))
    }
}

fn engine(providers: Vec<Arc<Slow>>) -> Engine {
    providers
        .into_iter()
        .enumerate()
        .fold(Engine::builder(), |b, (i, p)| {
            b.provider(p, ResiliencePolicy::default(), i as i32)
        })
        .build()
}

fn titled(title: &str) -> Claim {
    let mut c = Claim::default();
    c.title = Some(title.into());
    c
}

#[tokio::test(start_paused = true)]
async fn title_searches_across_providers_overlap() {
    let engine = engine(vec![
        Slow::new("a", false),
        Slow::new("b", false),
        Slow::new("c", false),
    ]);
    let start = Instant::now();
    engine.resolve(&titled("Some title"), 5).await;
    assert_eq!(
        start.elapsed(),
        Duration::from_secs(1),
        "three providers, one second"
    );
}

#[tokio::test(start_paused = true)]
async fn matcher_stage_precedes_title_stage() {
    // Titles come from matcher hits, so the stages are sequential: 1s + 1s.
    let engine = engine(vec![
        Slow::new("matcher", true),
        Slow::new("keywords", false),
    ]);
    let start = Instant::now();
    engine
        .resolve(&Claim::from_text("Some reference, 2020"), 5)
        .await;
    assert_eq!(start.elapsed(), Duration::from_secs(2));
}

#[tokio::test(start_paused = true)]
async fn claims_in_a_batch_are_checked_several_at_a_time() {
    let engine = engine(vec![Slow::new("keywords", false)]);
    let claims: Vec<Claim> = (0..8).map(|i| titled(&format!("Title {i}"))).collect();
    let start = Instant::now();
    let mut order = Vec::new();
    let checked = engine.check(&claims, 5, |i| order.push(i)).await;
    assert_eq!(checked.len(), 8);
    assert_eq!(
        order,
        (0..8).collect::<Vec<_>>(),
        "results stay in claim order"
    );
    assert_eq!(
        start.elapsed(),
        Duration::from_secs(2),
        "8 claims, 4 at a time"
    );
}
