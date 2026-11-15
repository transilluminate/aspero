//! Searching the built-in sources through the engine, answered by a client
//! that records each request: what `search_supports` says is what a
//! search does.
#![cfg(all(
    feature = "openalex",
    feature = "crossref",
    feature = "datacite",
    feature = "europepmc",
    feature = "openlibrary"
))]

use std::sync::{Arc, Mutex};

use aspero_core::testing::response;
use aspero_core::{
    Error, HttpClient, HttpRequest, HttpResponse, Provider, QueryNode, ResiliencePolicy,
    SearchQuery, Sort, TermField,
};
use aspero_engine::{Engine, EngineError};
use async_trait::async_trait;

/// Answers every request with an empty page, as each source's JSON reads
/// it, and keeps the URLs asked.
#[derive(Default)]
struct Recorder {
    urls: Mutex<Vec<String>>,
}

impl Recorder {
    fn urls(&self) -> Vec<String> {
        self.urls.lock().unwrap().clone()
    }
}

#[async_trait]
impl HttpClient for Recorder {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, Error> {
        self.urls.lock().unwrap().push(request.url);
        Ok(response(200, &[], r#"{"message": {"items": []}}"#))
    }
}

/// The searching sources, at their standard priorities, behind one
/// recorder.
fn engine() -> (Engine, Arc<Recorder>) {
    let http = Arc::new(Recorder::default());
    let client = || -> Arc<dyn HttpClient> { http.clone() };
    let providers: [(Arc<dyn Provider>, i32); 5] = [
        (
            Arc::new(aspero_openalex::OpenAlex::new(Default::default(), client())),
            10,
        ),
        (
            Arc::new(aspero_crossref::Crossref::new(Default::default(), client())),
            20,
        ),
        (
            Arc::new(aspero_datacite::Datacite::new(Default::default(), client())),
            25,
        ),
        (
            Arc::new(aspero_europepmc::EuropePmc::new(
                Default::default(),
                client(),
            )),
            35,
        ),
        (
            Arc::new(aspero_openlibrary::OpenLibrary::new(
                Default::default(),
                client(),
            )),
            40,
        ),
    ];
    let mut policy = ResiliencePolicy::default();
    policy.max_retries = 0;
    let engine = providers
        .into_iter()
        .fold(Engine::builder(), |b, (p, priority)| {
            b.provider(p, policy.clone(), priority)
        })
        .clock(|| "2026-11-13T12:00:00Z".parse().unwrap())
        .build();
    (engine, http)
}

fn with_tree(tree: QueryNode) -> SearchQuery {
    let mut q = SearchQuery::new("");
    q.tree = Some(tree);
    q
}

/// Newest first caps a tree at this year, which puts a years filter
/// beside it: that mustn't make searchable what `search_supports` (which
/// knows no sort) refuses.
#[tokio::test(start_paused = true)]
async fn newest_first_doesnt_change_what_a_source_can_search() {
    let (engine, http) = engine();
    let left_out = QueryNode::None(vec![QueryNode::term(TermField::Title, "x")]);
    let why = "Open Library can only leave out terms beside something to find, in an all";
    assert_eq!(
        engine
            .search_supports(Some("openlibrary"), &left_out)
            .as_deref(),
        Some(why)
    );
    let mut q = with_tree(left_out);
    q.sort = Sort::Newest;
    match engine.search(Some("openlibrary"), &q).await {
        Err(EngineError::Provider {
            error: Error::UnsupportedQuery(r),
            ..
        }) => assert_eq!(r, why),
        other => panic!("{:?}", other.map(|r| r.provider)),
    }
    assert!(http.urls().is_empty());
}
