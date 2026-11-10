//! OpenAlex provider against responses recorded by `scripts/record-openalex.sh`.

use std::sync::Arc;
use std::time::Duration;

use aspero_core::testing::{FIXTURE_TIME, ReplayClient, block_on, fixture, response};
use aspero_core::{
    Error, FieldSet, HttpResponse, Page, Provider, SearchQuery, SourceRecord, WorkId, WorkType,
    YearRange,
};
use aspero_openalex::{OpenAlex, OpenAlexConfig};
use secrecy::SecretString;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn recorded(name: &str) -> HttpResponse {
    fixture(FIXTURES, name)
}

fn provider(responses: impl IntoIterator<Item = HttpResponse>) -> (OpenAlex, Arc<ReplayClient>) {
    provider_with(Some("test-key"), responses)
}

fn provider_with(
    key: Option<&str>,
    responses: impl IntoIterator<Item = HttpResponse>,
) -> (OpenAlex, Arc<ReplayClient>) {
    let http = Arc::new(ReplayClient::new(responses));
    let mut config = OpenAlexConfig::default();
    config.api_key = key.map(|k| SecretString::from(k.to_owned()));
    (OpenAlex::new(config, http.clone()), http)
}

fn id(s: &str) -> WorkId {
    s.parse().unwrap()
}

fn ids(record: &SourceRecord) -> Vec<String> {
    record.ids.iter().map(ToString::to_string).collect()
}

/// Decodes the query string of the n-th request.
fn query(http: &ReplayClient, n: usize) -> Vec<(String, String)> {
    let url = url::Url::parse(&http.requests()[n].url).unwrap();
    url.query_pairs().into_owned().collect()
}

fn after(secs: f64) -> Option<jiff::Timestamp> {
    Some(
        FIXTURE_TIME
            .checked_add(Duration::from_secs_f64(secs))
            .unwrap(),
    )
}

#[test]
fn single_doi_lookup_uses_free_endpoint_and_maps_the_work() {
    let fixture = recorded("work_by_doi");
    // Budget headers change with every recording, so expectations come from them.
    let header = |name| fixture.header(name).unwrap().parse::<f64>().unwrap();
    let (remaining, reset) = (
        header("x-ratelimit-remaining-usd"),
        header("x-ratelimit-reset"),
    );
    let (oa, http) = provider([fixture]);
    let resp = block_on(oa.lookup(&[id("10.1038/nature14539")], FieldSet::DEFAULT)).unwrap();

    let req = &http.requests()[0];
    assert_eq!(
        req.url,
        "https://api.openalex.org/works/doi:10.1038/nature14539"
    );
    assert!(
        req.headers
            .contains(&("authorization".into(), "Bearer test-key".into()))
    );

    let record = resp.value[0].record.as_ref().unwrap();
    assert_eq!(
        ids(record),
        [
            "openalex:W2919115771",
            "doi:10.1038/nature14539",
            "pmid:26017442"
        ]
    );
    let w = &record.work;
    assert_eq!(w.title.as_deref(), Some("Deep learning"));
    assert_eq!(w.issued.unwrap().to_string(), "2015-05-26");
    assert_eq!(w.work_type, Some(WorkType::JournalArticle));
    assert_eq!(w.container_title.as_deref(), Some("Nature"));
    assert_eq!(w.volume.as_deref(), Some("521"));
    assert_eq!(w.page.as_deref(), Some("436-444"));
    let first = &w.authors[0];
    assert_eq!(first.display_name().as_deref(), Some("Yann LeCun"));
    assert_eq!(first.orcid.as_deref(), Some("0000-0002-1992-2684"));
    assert!(w.cited_by_count.unwrap() > 80_000);
    assert_eq!(record.fields, FieldSet::ALL);
    assert_eq!(record.fetched_at, FIXTURE_TIME);
    assert!(record.raw.body.contains("https://openalex.org/W2919115771"));

    let usage = resp.usage.unwrap();
    assert_eq!(usage.cost, Some(0.0));
    assert_eq!(usage.remaining, Some(remaining));
    assert_eq!(usage.resets_at, after(reset));
}

#[test]
fn pmid_and_openalex_singletons_use_their_path_forms() {
    let (oa, http) = provider([recorded("work_by_pmid"), recorded("work_by_openalex")]);
    block_on(oa.lookup(&[id("pmid:26017442")], FieldSet::DEFAULT)).unwrap();
    block_on(oa.lookup(&[id("W2919115771")], FieldSet::DEFAULT)).unwrap();
    let urls: Vec<_> = http.requests().into_iter().map(|r| r.url).collect();
    assert_eq!(
        urls,
        [
            "https://api.openalex.org/works/pmid:26017442",
            "https://api.openalex.org/works/W2919115771",
        ]
    );
}

#[test]
fn arxiv_ids_are_looked_up_by_datacite_doi() {
    let (oa, http) = provider([recorded("work_by_arxiv_doi")]);
    let resp = block_on(oa.lookup(&[id("arXiv:1706.03762v5")], FieldSet::DEFAULT)).unwrap();

    assert_eq!(
        http.requests()[0].url,
        "https://api.openalex.org/works/doi:10.48550/arxiv.1706.03762"
    );
    let record = resp.value[0].record.as_ref().unwrap();
    assert!(ids(record).contains(&"arxiv:1706.03762".to_owned()));
    assert_eq!(record.work.work_type, Some(WorkType::Preprint));
    assert_eq!(
        record.work.title.as_deref(),
        Some("Attention Is All You Need")
    );
}

#[test]
fn unknown_id_is_an_absent_record_not_an_error() {
    let (oa, _) = provider([recorded("work_not_found")]);
    let resp =
        block_on(oa.lookup(&[id("10.5555/aspero-does-not-exist")], FieldSet::DEFAULT)).unwrap();
    assert_eq!(resp.value.len(), 1);
    assert!(resp.value[0].record.is_none());
}

#[test]
fn batch_lookup_answers_every_id_in_request_order() {
    let (oa, http) = provider([recorded("works_batch_doi")]);
    let requested = [
        id("10.48550/arXiv.1706.03762"),
        id("10.5555/aspero-does-not-exist"),
        id("https://doi.org/10.1038/NATURE14539"),
    ];
    let resp = block_on(oa.lookup(&requested, FieldSet::DEFAULT)).unwrap();

    assert_eq!(http.requests().len(), 1);
    assert!(query(&http, 0).contains(&(
        "filter".into(),
        "doi:10.48550/arxiv.1706.03762|10.5555/aspero-does-not-exist|10.1038/nature14539".into()
    )));

    let found: Vec<_> = resp
        .value
        .iter()
        .map(|l| {
            (
                l.id.to_string(),
                l.record.as_ref().and_then(|r| r.work.title.clone()),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            (
                "doi:10.48550/arxiv.1706.03762".into(),
                Some("Attention Is All You Need".into())
            ),
            ("doi:10.5555/aspero-does-not-exist".into(), None),
            (
                "doi:10.1038/nature14539".into(),
                Some("Deep learning".into())
            ),
        ]
    );
    assert_eq!(resp.usage.unwrap().cost, Some(0.0001));
}

#[test]
fn mixed_batch_makes_one_call_per_kind_and_sums_cost() {
    let (oa, http) = provider([
        recorded("works_batch_openalex"),
        recorded("works_batch_pmid"),
    ]);
    let requested = [id("W2919115771"), id("pmid:26017442"), id("W2626778328")];
    let resp = block_on(oa.lookup(&requested, FieldSet::DEFAULT)).unwrap();

    assert_eq!(
        query(&http, 0)[0],
        ("filter".into(), "openalex:W2919115771|W2626778328".into())
    );
    assert_eq!(
        query(&http, 1)[0],
        ("filter".into(), "pmid:26017442".into())
    );
    assert!(resp.value.iter().all(|l| l.record.is_some()));
    assert_eq!(
        resp.value[1].record.as_ref().unwrap().work.title.as_deref(),
        Some("Deep learning")
    );
    assert_eq!(resp.usage.unwrap().cost, Some(0.0002));
}

#[test]
fn search_keeps_rank_order_and_reports_cost() {
    let (oa, http) = provider([recorded("works_search")]);
    let mut q = SearchQuery::new("attention is all you need");
    q.page.limit = 5;
    let resp = block_on(oa.search(&q)).unwrap();

    let params = query(&http, 0);
    assert!(params.contains(&("search".into(), "attention is all you need".into())));
    assert!(params.contains(&("per-page".into(), "5".into())));
    assert!(params.contains(&("cursor".into(), "*".into())));

    let Page { works, total, .. } = resp.value;
    assert_eq!(works.len(), 5);
    assert_eq!(
        works[0].work.title.as_deref(),
        Some("Attention Is All You Need")
    );
    assert!(total.unwrap() > 1_000_000);
    assert_eq!(resp.usage.unwrap().cost, Some(0.001));
}

#[test]
fn search_applies_every_declared_filter_and_pages_by_cursor() {
    let (oa, http) = provider([recorded("works_search_filtered")]);
    let mut q = SearchQuery::new("transformer");
    q.years = Some(YearRange {
        from: Some(2017),
        to: Some(2018),
    });
    q.open_access = Some(true);
    q.work_type = Some(WorkType::Preprint);
    q.page.limit = 3;
    let resp = block_on(oa.search(&q)).unwrap();

    assert!(query(&http, 0).contains(&(
        "filter".into(),
        "from_publication_date:2017-01-01,to_publication_date:2018-12-31,type:preprint,is_oa:true"
            .into()
    )));
    let page = resp.value;
    assert_eq!(page.total, Some(1090));
    assert!(page.next.is_some());
    assert!(
        page.works
            .iter()
            .all(|r| r.work.work_type == Some(WorkType::Preprint))
    );
}

#[test]
fn unknown_extra_parameters_are_rejected_before_sending() {
    let (oa, http) = provider([]);
    let mut q = SearchQuery::new("x");
    q.extra.insert("frobnicate".into(), "1".into());
    assert!(matches!(block_on(oa.search(&q)), Err(Error::BadRequest(_))));
    assert!(http.requests().is_empty());
}

#[test]
fn bad_filter_surfaces_openalex_message_truncated() {
    let (oa, _) = provider([recorded("bad_filter")]);
    let mut q = SearchQuery::new("x");
    q.extra.insert("filter".into(), "no_such_field:1".into());
    let Err(Error::BadRequest(msg)) = block_on(oa.search(&q)) else {
        panic!("expected BadRequest");
    };
    assert!(
        msg.starts_with("no_such_field is not a valid field"),
        "{msg}"
    );
    assert!(msg.chars().count() <= 301, "message not truncated");
}

#[test]
fn spent_budget_is_quota_exhausted_with_reset_time() {
    let spent = response(
        429,
        &[
            ("x-ratelimit-remaining-usd", "0"),
            ("x-ratelimit-reset", "3600"),
        ],
        r#"{"error":"Rate limit exceeded"}"#,
    );
    let (oa, _) = provider([spent]);
    let err = block_on(oa.search(&SearchQuery::new("x"))).unwrap_err();
    let Error::QuotaExhausted { resets_at } = err else {
        panic!("expected QuotaExhausted, got {err:?}");
    };
    assert_eq!(resets_at, after(3600.0));
}

#[test]
fn plain_429_is_rate_limited_with_retry_after() {
    let busy = response(
        429,
        &[("retry-after", "2"), ("x-ratelimit-remaining-usd", "0.5")],
        r#"{"error":"Rate limit exceeded"}"#,
    );
    let (oa, _) = provider([busy]);
    let err = block_on(oa.search(&SearchQuery::new("x"))).unwrap_err();
    assert!(
        matches!(err, Error::RateLimited { retry_after: Some(d) } if d == Duration::from_secs(2))
    );
}

#[test]
fn unactivated_key_is_auth_rejected_and_5xx_is_server_error() {
    let forbidden = response(403, &[], r#"{"error":"key_not_activated"}"#);
    let down = response(503, &[], "<html>Service Unavailable</html>");
    let (oa, _) = provider([forbidden, down]);
    let q = SearchQuery::new("x");
    assert!(matches!(
        block_on(oa.search(&q)),
        Err(Error::AuthRejected { message: Some(m) }) if m == "key_not_activated"
    ));
    assert!(matches!(
        block_on(oa.search(&q)),
        Err(Error::Server {
            status: 503,
            message: None
        })
    ));
}

#[test]
fn no_key_sends_no_authorization_header() {
    let (oa, http) = provider_with(None, [recorded("work_by_doi")]);
    block_on(oa.lookup(&[id("10.1038/nature14539")], FieldSet::DEFAULT)).unwrap();
    assert!(
        http.requests()[0]
            .headers
            .iter()
            .all(|(k, _)| k != "authorization")
    );
}

#[test]
fn unsupported_id_kinds_are_refused() {
    let (oa, http) = provider([]);
    let err = block_on(oa.lookup(&[id("CorpusId:13756489")], FieldSet::DEFAULT)).unwrap_err();
    assert!(matches!(err, Error::Unsupported(_)));
    assert!(http.requests().is_empty());
}
