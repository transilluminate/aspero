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
    Error, FilterValue, HttpClient, HttpRequest, HttpResponse, Provider, QueryNode, Relation,
    ResiliencePolicy, SearchQuery, Sort, Term, TermField, WorkType, YearRange,
};
use aspero_engine::{Engine, EngineError};
use async_trait::async_trait;
use proptest::prelude::*;

/// Answers every request with an empty page, as each source's JSON reads
/// it (but OpenAlex's journals, one source, so its works are asked too),
/// and keeps the URLs asked.
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
        let source = r#"{"results": [{"id": "https://openalex.org/S1", "display_name": "S"}]}"#;
        let body = if request.url.contains("/sources?") {
            source
        } else {
            r#"{"message": {"items": []}}"#
        };
        self.urls.lock().unwrap().push(request.url);
        Ok(response(200, &[], body))
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

/// Text that is syntax somewhere: brackets, wildcards, filter separators,
/// escapes, ranges, operators; and words, unicode and blanks.
fn text() -> impl Strategy<Value = String> {
    let token = prop::sample::select(vec![
        "(", ")", "*", ",", "|", "!", ":", "\\", "<", ">", "=", "\"", "AND", "OR", "NOT", "sepsis",
        "lactate", "é", "細胞", " ", "",
    ]);
    prop::collection::vec(token, 0..5).prop_map(|t| t.join(" "))
}

fn term() -> impl Strategy<Value = QueryNode> {
    let words = prop::sample::select(vec![
        TermField::Keywords,
        TermField::Title,
        TermField::Author,
        TermField::Container,
        TermField::Abstract,
    ]);
    let author_ids = prop_oneof![
        3 => prop::sample::select(vec!["0000-0002-1825-0097", "A5023888391"]).prop_map(String::from),
        1 => text(),
    ];
    let ids = prop_oneof![
        3 => prop::sample::select(vec![
            "10.1038/nature14539",
            "pmid:26017442",
            "PMC7254481",
            "arXiv:1706.03762",
            "W2919115771",
            "978-0-262-03561-3",
        ])
        .prop_map(String::from),
        1 => text(),
    ];
    let field_value = prop_oneof![
        3 => (words, text()),
        1 => author_ids.prop_map(|v| (TermField::AuthorId, v)),
        1 => ids.prop_map(|v| (TermField::Identifier, v)),
    ];
    (field_value, any::<bool>()).prop_map(|((field, value), phrase)| {
        if phrase {
            QueryNode::phrase(field, value)
        } else {
            QueryNode::term(field, value)
        }
    })
}

fn filter() -> impl Strategy<Value = QueryNode> {
    let year = prop::sample::select(vec![None, Some(1990), Some(2020), Some(2030)]);
    let work_type = prop::sample::select(vec![
        WorkType::JournalArticle,
        WorkType::Book,
        WorkType::Dataset,
        WorkType::Software,
        WorkType::Review,
        WorkType::from_csl("peer-review"),
        WorkType::from_csl("book,is_oa:true"),
    ]);
    let language = prop::sample::select(vec!["en", "de", "xx", " "]);
    prop_oneof![
        (year.clone(), year).prop_map(|(from, to)| FilterValue::Years(YearRange { from, to })),
        work_type.prop_map(FilterValue::WorkType),
        any::<bool>().prop_map(FilterValue::OpenAccess),
        Just(FilterValue::NotRetracted),
        language.prop_map(|l| FilterValue::Language(l.to_owned())),
        Just(FilterValue::HasAbstract),
    ]
    .prop_map(QueryNode::filter)
}

/// OpenAlex work ids only: others would be looked up first.
fn relation() -> impl Strategy<Value = QueryNode> {
    (
        prop::sample::select(Relation::ALL.to_vec()),
        prop::sample::select(vec!["W2919115771", "https://openalex.org/W1"]),
    )
        .prop_map(|(relation, id)| QueryNode::relation(relation, id))
}

/// Trees up to three groups deep.
fn tree() -> impl Strategy<Value = QueryNode> {
    let leaf = prop_oneof![4 => term(), 2 => filter(), 1 => relation()];
    leaf.prop_recursive(3, 24, 4, |inner| {
        let nodes = prop::collection::vec(inner, 0..4);
        prop_oneof![
            nodes.clone().prop_map(QueryNode::All),
            nodes.clone().prop_map(QueryNode::Any),
            nodes.prop_map(QueryNode::None),
        ]
    })
}

/// Every term searched as words or a phrase in some field (not ids).
fn text_terms<'a>(node: &'a mut QueryNode, out: &mut Vec<&'a mut Term>) {
    match node {
        QueryNode::All(nodes) | QueryNode::Any(nodes) | QueryNode::None(nodes) => {
            nodes.iter_mut().for_each(|n| text_terms(n, out));
        }
        QueryNode::Term(t) if !matches!(t.field, TermField::AuthorId | TermField::Identifier) => {
            out.push(t);
        }
        _ => {}
    }
}

const SOURCES: [&str; 5] = [
    "openalex",
    "crossref",
    "europepmc",
    "datacite",
    "openlibrary",
];

/// Why a search failed, as `search_supports` would say it.
fn reason(error: &EngineError) -> String {
    match error {
        EngineError::Provider {
            error: Error::UnsupportedQuery(r),
            ..
        } => r.clone(),
        other => other.to_string(),
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Each source searches exactly the trees `search_supports` says it
    /// can, failing on the others with its reason before sending
    /// anything; without a source, the first in search order that can.
    #[test]
    fn search_supports_says_what_each_source_searches(tree in tree()) {
        let (engine, http) = engine();
        let rt = runtime();
        let order: Vec<String> = engine
            .search_providers()
            .map(|p| p.id().to_string())
            .collect();
        prop_assert_eq!(&order, &SOURCES);
        let mut first_that_can = None;
        for source in SOURCES {
            let supports = engine.search_supports(Some(source), &tree);
            let before = http.urls().len();
            let searched = rt.block_on(engine.search(Some(source), &with_tree(tree.clone())));
            let sent = http.urls().len() - before;
            match (&supports, &searched) {
                (None, Ok(_)) => {
                    prop_assert!(sent > 0, "{source} sent nothing for {tree:?}");
                    first_that_can.get_or_insert(source);
                }
                (Some(why), Err(e)) => {
                    prop_assert_eq!(why, &reason(e), "{} on {:?}", source, tree);
                    prop_assert_eq!(sent, 0);
                }
                (supports, searched) => prop_assert!(
                    false,
                    "{source}: supports {supports:?}, search {:?}, for {tree:?}",
                    searched.as_ref().map(|r| &r.provider).map_err(reason)
                ),
            }
        }
        let supports = engine.search_supports(None, &tree);
        let searched = rt.block_on(engine.search(None, &with_tree(tree.clone())));
        match (first_that_can, supports, searched) {
            (Some(first), None, Ok(report)) => prop_assert_eq!(report.provider.as_str(), first),
            (None, Some(why), Err(e)) => prop_assert_eq!(why, reason(&e)),
            (first, supports, searched) => prop_assert!(
                false,
                "first {first:?}, supports {supports:?}, search {:?}, for {tree:?}",
                searched.map(|r| r.provider).map_err(|e| reason(&e))
            ),
        }
    }

    /// Whatever a source searches by, it sends: a term's words, made a
    /// sentinel, reach the request (no term is dropped or swallowed).
    #[test]
    fn every_searched_term_is_sent(mut tree in tree(), pick in any::<prop::sample::Index>()) {
        let mut terms = Vec::new();
        text_terms(&mut tree, &mut terms);
        if terms.is_empty() {
            return Ok(());
        }
        let i = pick.index(terms.len());
        terms[i].value = "zq7x".into();
        let (engine, http) = engine();
        let rt = runtime();
        for source in SOURCES {
            if engine.search_supports(Some(source), &tree).is_some() {
                continue;
            }
            let before = http.urls().len();
            rt.block_on(engine.search(Some(source), &with_tree(tree.clone()))).unwrap();
            let urls = &http.urls()[before..];
            prop_assert!(
                urls.iter().any(|u| u.contains("zq7x")),
                "{source} dropped the term from {tree:?}: {urls:?}"
            );
        }
    }
}
