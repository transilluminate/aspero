//! Engine routing, fallback and reporting, against scriptable providers.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aspero_core::{
    BooleanSupport, Capabilities, Error, ErrorKind, FieldSet, Filter, FilterValue, Hit, IdKind,
    Lookup, Page, Provider, ProviderId, QueryNode, Raw, Relation, ResiliencePolicy, Response,
    Result, SearchMode, SearchQuery, Sort, SourceRecord, TermField, Usage, Work, WorkId, YearRange,
};
use aspero_engine::{Engine, EngineError, Outcome};
use async_trait::async_trait;
use jiff::Timestamp;

/// A provider that knows some works, can be scripted to fail, and records
/// every call it receives.
struct Mock {
    id: ProviderId,
    caps: Capabilities,
    known: HashMap<WorkId, String>,
    /// More identifiers a known work's record carries.
    aliases: HashMap<WorkId, Vec<WorkId>>,
    failures: Mutex<VecDeque<Error>>,
    lookups: Mutex<Vec<Vec<WorkId>>>,
    searches: Mutex<Vec<SearchQuery>>,
    cost: Option<f64>,
}

impl Mock {
    fn new(id: &'static str, kinds: &[IdKind]) -> Self {
        let mut caps = Capabilities::default();
        caps.lookup_kinds = kinds.to_vec();
        caps.max_batch = 10;
        Mock {
            id: ProviderId::from_static(id),
            caps,
            known: HashMap::new(),
            aliases: HashMap::new(),
            failures: Mutex::default(),
            lookups: Mutex::default(),
            searches: Mutex::default(),
            cost: None,
        }
    }

    fn knows(mut self, id: &str, title: &str) -> Self {
        self.known.insert(id.parse().unwrap(), title.to_owned());
        self
    }

    /// Knows `id`, and its record carries `alias` too.
    fn knows_also(mut self, id: &str, alias: &str) -> Self {
        let id: WorkId = id.parse().unwrap();
        self.known.insert(id.clone(), "aliased".into());
        self.aliases.insert(id, vec![alias.parse().unwrap()]);
        self
    }

    fn failing(self, errors: impl IntoIterator<Item = Error>) -> Self {
        self.failures.lock().unwrap().extend(errors);
        self
    }

    fn searchable(mut self, citation: bool, filters: &[Filter]) -> Self {
        self.caps.search = true;
        self.caps.citation_search = citation;
        self.caps.search_filters = filters.to_vec();
        self
    }

    fn record(&self, id: &WorkId, title: &str) -> SourceRecord {
        let mut work = Work::default();
        work.title = Some(title.to_owned());
        let raw = Raw {
            media_type: "application/json".into(),
            body: "{}".into(),
        };
        SourceRecord::new(
            self.id.clone(),
            vec![id.clone()],
            work,
            FieldSet::ALL,
            raw,
            Timestamp::UNIX_EPOCH,
        )
    }

    fn respond<T>(&self, value: T) -> Response<T> {
        match self.cost {
            Some(cost) => {
                let mut usage = Usage::default();
                usage.cost = Some(cost);
                usage.remaining = Some(1.0);
                Response::with_usage(value, usage)
            }
            None => Response::new(value),
        }
    }

    fn lookup_calls(&self) -> Vec<Vec<String>> {
        let calls = self.lookups.lock().unwrap();
        calls
            .iter()
            .map(|b| b.iter().map(ToString::to_string).collect())
            .collect()
    }
}

#[async_trait]
impl Provider for Mock {
    fn id(&self) -> ProviderId {
        self.id.clone()
    }

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    async fn lookup(&self, ids: &[WorkId], _: FieldSet) -> Result<Response<Vec<Lookup>>> {
        self.lookups.lock().unwrap().push(ids.to_vec());
        if let Some(e) = self.failures.lock().unwrap().pop_front() {
            return Err(e);
        }
        let lookups = ids
            .iter()
            .map(|id| Lookup {
                id: id.clone(),
                record: self.known.get(id).map(|t| {
                    let mut record = self.record(id, t);
                    record
                        .ids
                        .extend(self.aliases.get(id).into_iter().flatten().cloned());
                    record
                }),
            })
            .collect();
        Ok(self.respond(lookups))
    }

    fn express(&self, tree: &QueryNode) -> std::result::Result<(), String> {
        aspero_core::query::check_supported(tree, &self.caps, "Mock")
    }

    async fn search(&self, query: &SearchQuery) -> Result<Response<Page>> {
        self.searches.lock().unwrap().push(query.clone());
        if let Some(e) = self.failures.lock().unwrap().pop_front() {
            return Err(e);
        }
        let id: WorkId = "10.1234/hit".parse().unwrap();
        let hit = Hit::new(self.record(&id, &query.text), Some(1.0));
        Ok(self.respond(Page::new(vec![hit], Some(1), None)))
    }
}

fn policy() -> ResiliencePolicy {
    let mut p = ResiliencePolicy::default();
    p.max_retries = 1;
    p.backoff_base = Duration::from_millis(10);
    p
}

fn engine(providers: Vec<(Arc<Mock>, i32)>) -> Engine {
    providers
        .into_iter()
        .fold(Engine::builder(), |b, (p, priority)| {
            b.provider(p, policy(), priority)
        })
        .jitter(|| 1.0)
        .build()
}

fn ids(list: &[&str]) -> Vec<WorkId> {
    list.iter().map(|s| s.parse().unwrap()).collect()
}

fn outcomes(attempts: &[aspero_engine::Attempt]) -> Vec<String> {
    attempts
        .iter()
        .map(|a| {
            let what = match &a.outcome {
                Outcome::Found => "found".to_owned(),
                Outcome::NotFound => "not-found".to_owned(),
                Outcome::Failed(e) => format!("failed: {e}"),
                _ => "other".to_owned(),
            };
            format!("{} {what}", a.provider)
        })
        .collect()
}

const DOI_A: &str = "10.1000/a";
const DOI_B: &str = "10.1000/b";

#[tokio::test(start_paused = true)]
async fn falls_through_to_the_next_provider_when_one_does_not_know_an_id() {
    let first = Arc::new(Mock::new("first", &[IdKind::Doi]).knows(DOI_A, "A"));
    let second = Arc::new(Mock::new("second", &[IdKind::Doi]).knows(DOI_B, "B"));
    let engine = engine(vec![(second.clone(), 20), (first.clone(), 10)]);

    let report = engine
        .lookup(&ids(&[DOI_A, DOI_B]), FieldSet::DEFAULT)
        .await;

    assert_eq!(first.lookup_calls(), [["doi:10.1000/a", "doi:10.1000/b"]]);
    assert_eq!(
        second.lookup_calls(),
        [["doi:10.1000/b"]],
        "only what's still missing"
    );
    let [a, b] = &report.results[..] else {
        panic!()
    };
    assert_eq!(a.record.as_ref().unwrap().source.as_str(), "first");
    assert_eq!(outcomes(&b.attempts), ["first not-found", "second found"]);
    assert_eq!(b.record.as_ref().unwrap().work.title.as_deref(), Some("B"));
}

#[tokio::test(start_paused = true)]
async fn a_final_not_found_ends_the_search() {
    let mut registry = Mock::new("registry", &[IdKind::Doi]);
    registry.caps.final_misses = vec![IdKind::Doi];
    let registry = Arc::new(registry.knows(DOI_A, "A"));
    let slow = Arc::new(Mock::new("slow", &[IdKind::Doi]));
    let engine = engine(vec![(registry.clone(), 10), (slow.clone(), 20)]);

    let report = engine
        .lookup(&ids(&[DOI_A, DOI_B]), FieldSet::DEFAULT)
        .await;

    assert!(report.results[1].is_not_found());
    // Nothing left to ask the next provider about.
    assert!(slow.lookup_calls().is_empty());
}

#[tokio::test(start_paused = true)]
async fn routes_ids_only_to_providers_accepting_their_kind() {
    let dois_only = Arc::new(Mock::new("dois", &[IdKind::Doi]).knows(DOI_A, "A"));
    let pmids = Arc::new(Mock::new("pmids", &[IdKind::Pmid]).knows("pmid:42", "P"));
    let engine = engine(vec![(dois_only.clone(), 10), (pmids.clone(), 20)]);

    let report = engine
        .lookup(&ids(&["pmid:42", DOI_A, "CorpusId:7"]), FieldSet::DEFAULT)
        .await;

    assert_eq!(
        dois_only.lookup_calls(),
        [[DOI_A.replace("10.", "doi:10.")]]
    );
    assert_eq!(pmids.lookup_calls(), [["pmid:42"]]);
    assert!(report.results[0].record.is_some() && report.results[1].record.is_some());
    let corpus = &report.results[2];
    assert!(corpus.record.is_none() && corpus.attempts.is_empty() && !corpus.is_not_found());
}

#[tokio::test(start_paused = true)]
async fn splits_batches_at_max_batch() {
    let mut mock = Mock::new("small", &[IdKind::Doi]);
    mock.caps.max_batch = 2;
    let mock = Arc::new(mock);
    let engine = engine(vec![(mock.clone(), 10)]);

    let requested: Vec<String> = (0..5).map(|i| format!("10.1000/{i}")).collect();
    let requested: Vec<&str> = requested.iter().map(String::as_str).collect();
    engine.lookup(&ids(&requested), FieldSet::DEFAULT).await;

    let sizes: Vec<usize> = mock.lookup_calls().iter().map(Vec::len).collect();
    assert_eq!(sizes, [2, 2, 1]);
}

#[tokio::test(start_paused = true)]
async fn retries_then_falls_back_when_a_provider_keeps_failing() {
    let down = Error::Server {
        status: 503,
        message: None,
    };
    let flaky = Arc::new(Mock::new("flaky", &[IdKind::Doi]).failing([down.clone(), down]));
    let backup = Arc::new(Mock::new("backup", &[IdKind::Doi]).knows(DOI_A, "A"));
    let engine = engine(vec![(flaky.clone(), 10), (backup.clone(), 20)]);

    let report = engine.lookup(&ids(&[DOI_A]), FieldSet::DEFAULT).await;

    assert_eq!(flaky.lookup_calls().len(), 2, "one attempt plus one retry");
    let result = &report.results[0];
    assert_eq!(
        outcomes(&result.attempts),
        ["flaky failed: server error (HTTP 503)", "backup found"]
    );
}

#[tokio::test(start_paused = true)]
async fn spent_quota_stops_further_calls_to_that_provider() {
    let mut broke =
        Mock::new("broke", &[IdKind::Doi]).failing([Error::QuotaExhausted { resets_at: None }]);
    broke.caps.max_batch = 1;
    let broke = Arc::new(broke);
    let engine = engine(vec![(broke.clone(), 10)]);

    let report = engine
        .lookup(&ids(&[DOI_A, DOI_B]), FieldSet::DEFAULT)
        .await;

    assert_eq!(broke.lookup_calls().len(), 1);
    assert!(report.results.iter().all(|r| matches!(
        r.attempts[..],
        [aspero_engine::Attempt {
            outcome: Outcome::Failed(Error::QuotaExhausted { .. }),
            ..
        }]
    )));
}

#[tokio::test(start_paused = true)]
async fn usage_is_combined_per_provider() {
    let mut paid = Mock::new("paid", &[IdKind::Doi]);
    paid.caps.max_batch = 1;
    paid.cost = Some(0.25);
    let paid = Arc::new(paid);
    let engine = engine(vec![(paid, 10)]);

    let report = engine
        .lookup(&ids(&[DOI_A, DOI_B]), FieldSet::DEFAULT)
        .await;

    let [(provider, usage)] = &report.usage[..] else {
        panic!("{:?}", report.usage)
    };
    assert_eq!(provider.as_str(), "paid");
    assert_eq!(usage.cost, Some(0.5));
    // What's left is remembered, to show without spending.
    assert_eq!(engine.budgets()[0].1.remaining, Some(1.0));
}

#[tokio::test(start_paused = true)]
async fn citation_search_defaults_to_the_first_capable_provider() {
    let keywords = Arc::new(Mock::new("keywords", &[]).searchable(false, &[]));
    let matcher = Arc::new(Mock::new("matcher", &[]).searchable(true, &[]));
    let engine = engine(vec![(keywords.clone(), 10), (matcher.clone(), 20)]);

    let report = engine
        .search(None, &SearchQuery::citation("LeCun 2015"))
        .await
        .unwrap();
    assert_eq!(report.provider.as_str(), "matcher");

    let report = engine
        .search(None, &SearchQuery::new("deep learning"))
        .await
        .unwrap();
    assert_eq!(report.provider.as_str(), "keywords");
    assert_eq!(
        matcher.searches.lock().unwrap()[0].mode,
        SearchMode::Citation
    );
}

#[tokio::test(start_paused = true)]
async fn undeclared_filters_are_stripped_and_reported() {
    let only_years = Arc::new(Mock::new("years", &[]).searchable(false, &[Filter::Years]));
    let engine = engine(vec![(only_years.clone(), 10)]);
    let mut q = SearchQuery::new("x");
    q.years = Some(YearRange {
        from: Some(2017),
        to: None,
    });
    q.open_access = Some(true);

    let report = engine.search(Some("years"), &q).await.unwrap();

    assert_eq!(report.ignored_filters, [Filter::OpenAccess]);
    let sent = &only_years.searches.lock().unwrap()[0];
    assert!(sent.years.is_some() && sent.open_access.is_none());
}

#[tokio::test(start_paused = true)]
async fn missing_fields_and_sorts_fail_rather_than_widen_the_search() {
    let mut mock = Mock::new("fields", &[]).searchable(false, &[Filter::Author]);
    mock.caps.sorts = vec![Sort::MostCited];
    let mock = Arc::new(mock);
    let engine = engine(vec![(mock.clone(), 10)]);
    let unsupported = |r: std::result::Result<_, EngineError>| match r {
        Err(EngineError::Provider {
            error: Error::Unsupported(what),
            ..
        }) => what,
        other => panic!("{:?}", other.map(|_: aspero_engine::SearchReport| ())),
    };

    let mut q = SearchQuery::new("");
    q.author = Some("LeCun".into());
    q.title = Some("Deep learning".into());
    assert_eq!(
        unsupported(engine.search(None, &q).await),
        "search by title"
    );
    q.title = None;
    q.sort = Sort::Newest;
    assert_eq!(
        unsupported(engine.search(None, &q).await),
        "sorting newest first"
    );
    assert!(mock.searches.lock().unwrap().is_empty());

    // Fields alone, by relevance or a declared sort, are searched.
    for sort in [Sort::Relevance, Sort::MostCited] {
        q.sort = sort;
        engine.search(None, &q).await.unwrap();
    }
    let sent = mock.searches.lock().unwrap();
    assert_eq!(sent[0].author.as_deref(), Some("LeCun"));
    assert_eq!(sent[1].sort, Sort::MostCited);
}

#[tokio::test(start_paused = true)]
async fn search_providers_come_in_the_order_search_picks_from() {
    let lookups_only = Arc::new(Mock::new("lookups", &[IdKind::Doi]));
    let second = Arc::new(Mock::new("second", &[]).searchable(false, &[]));
    let first = Arc::new(Mock::new("first", &[]).searchable(true, &[]));
    let engine = engine(vec![(lookups_only, 5), (second, 20), (first, 10)]);

    let names: Vec<String> = engine
        .search_providers()
        .map(|p| p.id().to_string())
        .collect();
    assert_eq!(names, ["first", "second"]);
    let report = engine.search(None, &SearchQuery::new("x")).await.unwrap();
    assert_eq!(report.provider.as_str(), names[0]);
}

#[tokio::test(start_paused = true)]
async fn search_errors_name_the_provider() {
    let mock = Arc::new(
        Mock::new("named", &[])
            .searchable(false, &[])
            .failing([Error::BadRequest("nope".into())]),
    );
    let engine = engine(vec![(mock, 10)]);

    let err = engine
        .search(None, &SearchQuery::new("x"))
        .await
        .unwrap_err();
    assert_eq!(err.to_string(), "named: bad request: nope");

    let err = engine
        .search(Some("missing"), &SearchQuery::new("x"))
        .await
        .unwrap_err();
    assert!(matches!(err, EngineError::UnknownProvider(_)));
    let err = engine
        .search(None, &SearchQuery::citation("x"))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        EngineError::NoCapableProvider("citation search")
    ));
}

#[tokio::test(start_paused = true)]
async fn since_picks_the_first_source_that_can_and_is_never_ignored() {
    let first = Arc::new(Mock::new("first", &[]).searchable(false, &[Filter::Keywords]));
    let dated =
        Arc::new(Mock::new("dated", &[]).searchable(false, &[Filter::Keywords, Filter::Since]));
    let engine = engine(vec![(first.clone(), 10), (dated.clone(), 20)]);
    let mut q = SearchQuery::new("sepsis");
    q.since = Some(jiff::civil::date(2026, 11, 1));

    // Without a source: the first that can, as search_supports says.
    let report = engine.search(None, &q).await.unwrap();
    assert_eq!(report.provider.as_str(), "dated");
    assert!(report.ignored_filters.is_empty());
    assert_eq!(dated.searches.lock().unwrap()[0].since, q.since);
    let keywords = QueryNode::term(TermField::Keywords, "sepsis");
    assert_eq!(engine.search_supports_since(None, &keywords, q.since), None);
    // Without since, the first as ever.
    let plain = engine
        .search(None, &SearchQuery::new("sepsis"))
        .await
        .unwrap();
    assert_eq!(plain.provider.as_str(), "first");

    // Named, a source that can't fails and says why, unasked.
    let reason = "first can't search by date added";
    match engine.search(Some("first"), &q).await {
        Err(EngineError::Provider {
            provider,
            error: Error::UnsupportedQuery(r),
        }) => assert_eq!((provider.as_str(), r.as_str()), ("first", reason)),
        other => panic!("{:?}", other.map(|_| ())),
    }
    assert_eq!(
        engine
            .search_supports_since(Some("first"), &keywords, q.since)
            .as_deref(),
        Some(reason)
    );
    assert_eq!(engine.search_supports(Some("first"), &keywords), None);
    assert_eq!(first.searches.lock().unwrap().len(), 1);

    // None can: a clear refusal.
    let engine = self::engine(vec![(first, 10)]);
    let err = engine.search(None, &q).await.unwrap_err();
    assert!(matches!(
        err,
        EngineError::NoCapableProvider("search by date added")
    ));
    assert_eq!(err.kind(), ErrorKind::Unsupported);
    assert_eq!(
        engine
            .search_supports_since(None, &keywords, q.since)
            .as_deref(),
        Some("no enabled provider supports search by date added")
    );
}

#[tokio::test(start_paused = true)]
async fn a_search_for_nothing_is_refused() {
    let mock = Arc::new(Mock::new("any", &[]).searchable(false, &[]));
    let engine = engine(vec![(mock.clone(), 10)]);
    let r = engine.search(None, &SearchQuery::new("  ")).await;
    assert!(matches!(r, Err(EngineError::NothingToSearch)), "{r:?}");
    assert!(mock.searches.lock().unwrap().is_empty());

    // An expert's filter alone is a search.
    let mut q = SearchQuery::new("");
    q.extra.insert("filter".into(), "is_oa:true".into());
    engine.search(None, &q).await.unwrap();
    assert_eq!(mock.searches.lock().unwrap().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn newest_first_stops_at_this_year_unless_told_otherwise() {
    let mut mock = Mock::new("dated", &[]).searchable(false, &[Filter::Years]);
    mock.caps.sorts = vec![Sort::Newest];
    let mock = Arc::new(mock);
    let engine = Engine::builder()
        .provider(mock.clone(), policy(), 10)
        .jitter(|| 1.0)
        .clock(|| "2026-11-13T12:00:00Z".parse().unwrap())
        .build();

    let mut q = SearchQuery::new("deep learning");
    q.sort = Sort::Newest;
    engine.search(None, &q).await.unwrap();
    q.years = Some(YearRange {
        from: Some(2015),
        to: None,
    });
    engine.search(None, &q).await.unwrap();
    q.years = Some(YearRange {
        from: None,
        to: Some(2030),
    });
    engine.search(None, &q).await.unwrap();

    let sent = mock.searches.lock().unwrap();
    let years: Vec<_> = sent
        .iter()
        .map(|q| q.years.map(|y| (y.from, y.to)))
        .collect();
    assert_eq!(
        years,
        [
            Some((None, Some(2026))),
            Some((Some(2015), Some(2026))),
            Some((None, Some(2030))),
        ]
    );
}

fn title(words: &str) -> QueryNode {
    QueryNode::term(TermField::Title, words)
}

fn years(from: Option<i16>, to: Option<i16>) -> QueryNode {
    QueryNode::filter(FilterValue::Years(YearRange { from, to }))
}

/// Searches every tree of keywords and titles, with years and open
/// access; any and none anywhere, nested.
fn tree_mock(id: &'static str, kinds: &[IdKind]) -> Mock {
    let mut mock = Mock::new(id, kinds).searchable(
        false,
        &[
            Filter::Keywords,
            Filter::Title,
            Filter::Years,
            Filter::OpenAccess,
        ],
    );
    mock.caps.search_any = BooleanSupport::Anywhere;
    mock.caps.search_none = BooleanSupport::Anywhere;
    mock.caps.search_nesting = true;
    mock.caps.sorts = vec![Sort::Relevance, Sort::Newest];
    mock
}

fn with_tree(tree: QueryNode) -> SearchQuery {
    let mut q = SearchQuery::new("");
    q.tree = Some(tree);
    q
}

#[tokio::test(start_paused = true)]
async fn trees_are_normalised_and_take_the_requests_filters() {
    let mock = Arc::new(tree_mock("tree", &[]));
    let engine = engine(vec![(mock.clone(), 10)]);
    let mut q = with_tree(QueryNode::All(vec![
        QueryNode::All(vec![title("a")]),
        QueryNode::Any(vec![]),
        QueryNode::Any(vec![QueryNode::Any(vec![title("b"), title("c")])]),
    ]));
    q.years = Some(YearRange {
        from: Some(2017),
        to: None,
    });
    q.open_access = Some(true);
    // The mock can't filter by type: ignored, as without a tree.
    q.work_type = Some(aspero_core::WorkType::Book);
    let report = engine.search(None, &q).await.unwrap();
    assert_eq!(report.ignored_filters, [Filter::WorkType]);
    let sent = mock.searches.lock().unwrap()[0].clone();
    assert_eq!(
        sent.tree,
        Some(QueryNode::All(vec![
            title("a"),
            QueryNode::Any(vec![title("b"), title("c")]),
            years(Some(2017), None),
            QueryNode::filter(FilterValue::OpenAccess(true)),
        ]))
    );
    assert_eq!(
        (sent.years, sent.open_access, sent.work_type),
        (None, None, None)
    );
}

#[tokio::test(start_paused = true)]
async fn a_tree_replaces_text_and_fields_and_must_hold_something() {
    let mock = Arc::new(tree_mock("tree", &[]));
    let engine = engine(vec![(mock.clone(), 10)]);
    let invalid = |r: std::result::Result<_, EngineError>| match r {
        Err(EngineError::InvalidQuery(reason)) => reason,
        other => panic!("{other:?}"),
    };
    let mut q = with_tree(title("a"));
    q.text = "keywords".into();
    assert!(invalid(engine.search(None, &q).await).starts_with("a query replaces"));
    let mut q = with_tree(title("a"));
    q.author = Some("LeCun".into());
    assert!(invalid(engine.search(None, &q).await).starts_with("a query replaces"));

    let empty = with_tree(QueryNode::All(vec![QueryNode::None(vec![title(" ")])]));
    let r = engine.search(None, &empty).await;
    assert!(matches!(r, Err(EngineError::NothingToSearch)), "{r:?}");
    assert_eq!(
        engine.search_supports(None, empty.tree.as_ref().unwrap()),
        Some(EngineError::NothingToSearch.to_string())
    );
    assert!(mock.searches.lock().unwrap().is_empty());
    // As without a tree, an expert's own parameters are a search.
    let mut q = empty.clone();
    q.extra.insert("filter".into(), "is_oa:true".into());
    engine.search(None, &q).await.unwrap();
    assert_eq!(mock.searches.lock().unwrap()[0].tree, None);
}

#[tokio::test(start_paused = true)]
async fn trees_a_source_cant_say_fail_as_search_supports_says() {
    let mut mock = tree_mock("flat", &[]);
    mock.caps.search_any = BooleanSupport::No;
    let mock = Arc::new(mock);
    let engine = engine(vec![(mock.clone(), 10)]);
    let tree = QueryNode::Any(vec![title("a"), title("b")]);
    let reason = "Mock can't combine terms with any";
    assert_eq!(engine.search_supports(None, &tree).as_deref(), Some(reason));
    assert_eq!(engine.search_supports(Some("flat"), &title("a")), None);
    match engine.search(None, &with_tree(tree)).await {
        Err(EngineError::Provider {
            error: Error::UnsupportedQuery(r),
            ..
        }) => assert_eq!(r, reason),
        other => panic!("{other:?}"),
    }
    let abstracts = QueryNode::term(TermField::Abstract, "x");
    assert_eq!(
        engine.search_supports(None, &abstracts).as_deref(),
        Some("Mock can't search by abstract")
    );
    // Invalid values are the caller's to fix.
    let language = QueryNode::filter(FilterValue::Language("english".into()));
    assert_eq!(
        engine.search_supports(None, &language).as_deref(),
        Some("\"english\" is not an ISO 639-1 language code, like \"en\"")
    );
    assert_eq!(
        engine
            .search_supports(Some("nobody"), &title("a"))
            .as_deref(),
        Some("no enabled provider is named \"nobody\"")
    );
    assert!(mock.searches.lock().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn newest_trees_stop_at_this_year_unless_their_years_say_otherwise() {
    let mock = Arc::new(tree_mock("dated", &[]));
    let engine = Engine::builder()
        .provider(mock.clone(), policy(), 10)
        .jitter(|| 1.0)
        .clock(|| "2026-11-13T12:00:00Z".parse().unwrap())
        .build();
    let newest = |tree| {
        let mut q = with_tree(tree);
        q.sort = Sort::Newest;
        q
    };
    engine.search(None, &newest(title("a"))).await.unwrap();
    let mut from = newest(title("a"));
    from.years = Some(YearRange {
        from: Some(2015),
        to: None,
    });
    engine.search(None, &from).await.unwrap();
    let later = QueryNode::All(vec![title("a"), years(None, Some(2030))]);
    engine.search(None, &newest(later.clone())).await.unwrap();
    // Years under none don't bound every hit.
    let excluded = QueryNode::All(vec![
        title("a"),
        QueryNode::None(vec![years(None, Some(2000))]),
    ]);
    engine.search(None, &newest(excluded)).await.unwrap();

    let sent: Vec<_> = mock
        .searches
        .lock()
        .unwrap()
        .iter()
        .map(|q| q.tree.clone().unwrap())
        .collect();
    assert_eq!(
        sent,
        [
            QueryNode::All(vec![title("a"), years(None, Some(2026))]),
            QueryNode::All(vec![title("a"), years(Some(2015), Some(2026))]),
            later,
            QueryNode::All(vec![
                title("a"),
                QueryNode::None(vec![years(None, Some(2000))]),
                years(None, Some(2026)),
            ]),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn relations_are_searched_by_their_openalex_work() {
    // OpenAlex knows a DOI; Europe PMC knows a PMCID by its DOI, which
    // OpenAlex then knows.
    let mut openalex = tree_mock("openalex", &[IdKind::Doi, IdKind::Pmid, IdKind::OpenAlex])
        .knows_also(DOI_A, "W1")
        .knows_also(DOI_B, "W2");
    openalex.caps.relations = vec![Relation::Cites, Relation::CitedBy];
    let openalex = Arc::new(openalex);
    let europepmc = Arc::new(Mock::new("europepmc", &[IdKind::Pmcid]).knows_also("PMC7", DOI_B));
    let engine = engine(vec![(openalex.clone(), 10), (europepmc.clone(), 35)]);

    let tree = QueryNode::All(vec![
        QueryNode::relation(Relation::Cites, DOI_A),
        QueryNode::None(vec![QueryNode::relation(Relation::CitedBy, "PMC7")]),
        QueryNode::relation(Relation::Cites, "https://openalex.org/W3"),
    ]);
    engine.search(None, &with_tree(tree)).await.unwrap();
    assert_eq!(
        openalex.searches.lock().unwrap()[0].tree,
        Some(QueryNode::All(vec![
            QueryNode::relation(Relation::Cites, "W1"),
            QueryNode::None(vec![QueryNode::relation(Relation::CitedBy, "W2")]),
            QueryNode::relation(Relation::Cites, "W3"),
        ]))
    );
    // An OpenAlex id is used as given, without a lookup.
    assert_eq!(
        *openalex.lookups.lock().unwrap(),
        [ids(&[DOI_A]), ids(&[DOI_B])]
    );
    assert_eq!(*europepmc.lookups.lock().unwrap(), [ids(&["PMC7"])]);

    // Nowhere to be found: the search fails, and says which.
    let missing = QueryNode::relation(Relation::Cites, "10.1000/missing");
    match engine.search(None, &with_tree(missing)).await {
        Err(e @ EngineError::Unresolved { .. }) => {
            assert_eq!(e.kind(), ErrorKind::NotFound);
            assert_eq!(
                e.to_string(),
                "can't search by citations of 10.1000/missing: no such work found"
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(openalex.searches.lock().unwrap().len(), 1);
    // Only sources with relations can search by them.
    let crossref = Arc::new(tree_mock("crossref", &[]));
    let engine = engine_of(crossref);
    assert_eq!(
        engine
            .search_supports(None, &QueryNode::relation(Relation::CitedBy, DOI_A))
            .as_deref(),
        Some("Mock can't find the works a work cites")
    );
}

fn engine_of(mock: Arc<Mock>) -> Engine {
    engine(vec![(mock, 10)])
}
