//! Engine routing, fallback and reporting, against scriptable providers.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aspero_core::{
    Capabilities, Error, FieldSet, Filter, Hit, IdKind, Lookup, Page, Provider, ProviderId, Raw,
    ResiliencePolicy, Response, Result, SearchMode, SearchQuery, SourceRecord, Usage, Work, WorkId,
    YearRange,
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
                record: self.known.get(id).map(|t| self.record(id, t)),
            })
            .collect();
        Ok(self.respond(lookups))
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
