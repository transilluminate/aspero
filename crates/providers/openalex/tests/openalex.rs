//! OpenAlex provider against responses recorded by `scripts/record-openalex.sh`.

use std::sync::Arc;
use std::time::Duration;

use aspero_core::testing::{FIXTURE_TIME, ReplayClient, block_on, fixture, response};
use aspero_core::{
    Error, FieldSet, FilterValue, HttpResponse, Page, Place, Provider, QueryNode, Relation,
    SearchQuery, Sort, SourceRecord, TermField, WorkId, WorkType, YearRange,
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

    let Page { hits, total, .. } = resp.value;
    assert_eq!(hits.len(), 5);
    assert_eq!(
        hits[0].record.work.title.as_deref(),
        Some("Attention Is All You Need")
    );
    let scores: Vec<f64> = hits.iter().map(|h| h.score.unwrap()).collect();
    assert!(scores.is_sorted_by(|a, b| a >= b), "{scores:?}");
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
        page.hits
            .iter()
            .all(|h| h.record.work.work_type == Some(WorkType::Preprint))
    );
}

#[test]
fn search_by_place_filters_volume_and_first_page() {
    let (oa, http) = provider([recorded("works_search_filtered")]);
    let mut q = SearchQuery::new("");
    q.place = Some(Place::new("47", "797"));
    q.years = Some(YearRange {
        from: Some(2015),
        to: Some(2017),
    });
    block_on(oa.search(&q)).unwrap();
    let params = query(&http, 0);
    assert!(params.contains(&(
        "filter".into(),
        "from_publication_date:2015-01-01,to_publication_date:2017-12-31,biblio.volume:47,biblio.first_page:797"
            .into()
    )));
    assert!(!params.iter().any(|(k, _)| k == "search"), "{params:?}");
}

#[test]
fn fields_alone_search_by_filter_and_sort_when_asked() {
    let (oa, http) = provider([recorded("works_search_fields")]);
    let mut q = SearchQuery::new("");
    q.author = Some("lecun".into());
    q.title = Some(" deep learning ".into());
    q.sort = Sort::MostCited;
    q.page.limit = 3;
    let resp = block_on(oa.search(&q)).unwrap();

    assert_eq!(
        query(&http, 0),
        [
            ("sort", "cited_by_count:desc"),
            (
                "filter",
                "raw_author_name.search:lecun,title.search:deep learning"
            ),
            ("per-page", "3"),
            ("cursor", "*"),
        ]
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
    );
    let counts: Vec<u64> = resp
        .value
        .hits
        .iter()
        .map(|h| h.record.work.cited_by_count.unwrap())
        .collect();
    assert!(counts.is_sorted_by(|a, b| a >= b), "{counts:?}");
    assert_eq!(
        resp.value.hits[0].record.work.title.as_deref(),
        Some("Deep learning")
    );
}

#[test]
fn relevance_without_text_sends_no_sort() {
    // OpenAlex refuses `relevance_score` sorting without a search to rank by.
    let (oa, http) = provider([recorded("works_search_fields")]);
    let mut q = SearchQuery::new("");
    q.author = Some("lecun".into());
    block_on(oa.search(&q)).unwrap();
    let params = query(&http, 0);
    assert!(
        !params.iter().any(|(k, _)| k == "sort" || k == "search"),
        "{params:?}"
    );

    let (oa, http) = provider([recorded("works_search")]);
    let mut q = SearchQuery::new("x");
    q.sort = Sort::Newest;
    block_on(oa.search(&q)).unwrap();
    assert!(query(&http, 0).contains(&("sort".into(), "publication_date:desc".into())));
}

#[test]
fn field_values_lose_the_commas_and_pipes_that_split_filters() {
    let (oa, http) = provider([recorded("works_search_fields")]);
    let mut q = SearchQuery::new("");
    q.author = Some("LeCun, Y|Bengio".into());
    block_on(oa.search(&q)).unwrap();
    assert!(query(&http, 0).contains(&(
        "filter".into(),
        "raw_author_name.search:LeCun YBengio".into()
    )));
}

#[test]
fn container_searches_the_sources_so_named_first() {
    let empty = r#"{"meta": {"count": 0}, "results": []}"#;
    let (oa, http) = provider([
        recorded("sources_named"),
        response(200, &[("x-ratelimit-cost-usd", "0.001")], empty),
    ]);
    let mut q = SearchQuery::new("deep learning");
    q.container = Some("nature machine".into());
    let resp = block_on(oa.search(&q)).unwrap();

    assert!(
        http.requests()[0]
            .url
            .starts_with("https://api.openalex.org/sources?")
    );
    assert_eq!(
        query(&http, 0),
        [
            ("filter", "display_name.search:nature machine"),
            ("select", "id,display_name"),
            ("per-page", "100"),
        ]
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
    );
    let filter = query(&http, 1)
        .into_iter()
        .find(|(k, _)| k == "filter")
        .unwrap()
        .1;
    assert_eq!(filter, "primary_location.source.id:S2912241403|S7407104579");
    // Both searches are paid for.
    assert_eq!(resp.usage.unwrap().cost, Some(0.002));
}

#[test]
fn a_journal_phrase_keeps_the_source_so_named_exactly() {
    let sources = r#"{"meta": {"count": 3}, "results": [
        {"id": "https://openalex.org/S64187185", "display_name": "Nature Communications"},
        {"id": "https://openalex.org/S137773608", "display_name": "Nature"},
        {"id": "https://openalex.org/S106963461", "display_name": "Nature Biotechnology"}
    ]}"#;
    let empty = r#"{"meta": {"count": 0}, "results": []}"#;
    let (oa, http) = provider([
        response(200, &[("x-ratelimit-cost-usd", "0.001")], sources),
        response(200, &[("x-ratelimit-cost-usd", "0.001")], empty),
    ]);
    let mut q = SearchQuery::new("");
    q.tree = Some(QueryNode::phrase(TermField::Container, "Nature"));
    block_on(oa.search(&q)).unwrap();
    let filter = query(&http, 1)
        .into_iter()
        .find(|(k, _)| k == "filter")
        .unwrap()
        .1;
    assert_eq!(filter, "primary_location.source.id:S137773608");
}

#[test]
fn a_container_no_source_is_named_finds_nothing() {
    let none = r#"{"meta": {"count": 0}, "results": []}"#;
    let (oa, http) = provider([response(200, &[("x-ratelimit-cost-usd", "0.001")], none)]);
    let mut q = SearchQuery::new("");
    q.container = Some("no such journal".into());
    let page = block_on(oa.search(&q)).unwrap().value;
    assert!(page.hits.is_empty() && page.next.is_none());
    assert_eq!(page.total, Some(0));
    assert_eq!(http.requests().len(), 1);
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
fn the_request_crossing_the_budget_is_quota_exhausted_whatever_its_status() {
    let crossing = |status| {
        response(
            status,
            &[
                ("x-ratelimit-remaining-usd", "0"),
                ("x-ratelimit-reset", "3600"),
            ],
            "",
        )
    };
    let worded = response(402, &[], r#"{"error":"not_enough_credits"}"#);
    let (oa, _) = provider([crossing(500), crossing(503), worded]);
    for _ in 0..3 {
        let err = block_on(oa.search(&SearchQuery::new("x"))).unwrap_err();
        assert!(matches!(err, Error::QuotaExhausted { .. }), "{err:?}");
    }
}

#[test]
fn a_server_error_with_budget_left_is_a_server_error() {
    let fault = response(503, &[("x-ratelimit-remaining-usd", "0.5")], "");
    let (oa, _) = provider([fault]);
    let err = block_on(oa.search(&SearchQuery::new("x"))).unwrap_err();
    assert!(matches!(err, Error::Server { status: 503, .. }), "{err:?}");
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
fn citation_mode_is_unsupported() {
    let (oa, http) = provider([]);
    let err = block_on(oa.search(&SearchQuery::citation("LeCun 2015 Deep learning"))).unwrap_err();
    assert!(matches!(err, Error::Unsupported(_)));
    assert!(http.requests().is_empty());
}

#[test]
fn unsupported_id_kinds_are_refused() {
    let (oa, http) = provider([]);
    let err = block_on(oa.lookup(&[id("CorpusId:13756489")], FieldSet::DEFAULT)).unwrap_err();
    assert!(matches!(err, Error::Unsupported(_)));
    assert!(http.requests().is_empty());
}

#[test]
fn is_retracted_becomes_a_retraction_update() {
    let (oa, _) = provider([recorded("work_retracted")]);
    let resp =
        block_on(oa.lookup(&[id("10.1016/S0140-6736(97)11096-0")], FieldSet::DEFAULT)).unwrap();
    let w = &resp.value[0].record.as_ref().unwrap().work;
    let r = w.retraction().expect("retracted");
    assert_eq!(r.source.as_deref(), Some("openalex"));
    assert_eq!(r.notice, None, "OpenAlex gives no notice details");
    assert!(w.title.as_deref().unwrap().starts_with("RETRACTED"));
}

#[test]
fn unretracted_works_have_no_updates() {
    let (oa, _) = provider([recorded("work_by_doi")]);
    let resp = block_on(oa.lookup(&[id("10.1038/nature14539")], FieldSet::DEFAULT)).unwrap();
    assert!(
        resp.value[0]
            .record
            .as_ref()
            .unwrap()
            .work
            .updates
            .is_empty()
    );
}

#[test]
fn open_access_comes_from_the_best_free_copy() {
    let (oa, _) = provider([
        recorded("work_by_arxiv_doi"),
        recorded("work_by_doi"),
        recorded("work_retracted"),
    ]);
    let get = |i: &str| {
        let resp = block_on(oa.lookup(&[id(i)], FieldSet::DEFAULT)).unwrap();
        resp.value[0]
            .record
            .clone()
            .unwrap()
            .work
            .open_access
            .unwrap()
    };

    let arxiv = get("arXiv:1706.03762");
    assert!(arxiv.is_oa);
    assert_eq!(arxiv.status.as_deref(), Some("green"));
    assert_eq!(
        arxiv.pdf_url.as_deref(),
        Some("https://arxiv.org/pdf/1706.03762")
    );
    assert_eq!(arxiv.source.as_deref(), Some("openalex"));

    // A free repository copy without a PDF link.
    let nature = get("10.1038/nature14539");
    assert!(nature.is_oa && nature.pdf_url.is_none() && nature.url.is_some());
    assert_eq!(nature.license.as_deref(), Some("other-oa"));

    let closed = get("10.1016/S0140-6736(97)11096-0");
    assert!(!closed.is_oa);
    assert_eq!(closed.status.as_deref(), Some("closed"));
    assert_eq!((closed.pdf_url, closed.url), (None, None));
}

fn tree_query(tree: QueryNode) -> SearchQuery {
    let mut q = SearchQuery::new("");
    q.tree = Some(tree);
    q.page.limit = 3;
    q
}

fn pairs<const N: usize>(list: [(&str, &str); N]) -> Vec<(String, String)> {
    list.map(|(k, v)| (k.to_owned(), v.to_owned())).to_vec()
}

#[test]
fn trees_search_keywords_and_and_filters_with_boolean_fields() {
    let (oa, http) = provider([recorded("works_search_tree")]);
    let tree = QueryNode::All(vec![
        QueryNode::term(TermField::Keywords, "protein structure"),
        QueryNode::Any(vec![
            QueryNode::term(TermField::Title, "alphafold"),
            QueryNode::phrase(TermField::Title, "structure prediction"),
        ]),
        QueryNode::None(vec![QueryNode::term(TermField::Author, "jumper")]),
        QueryNode::filter(FilterValue::NotRetracted),
        QueryNode::filter(FilterValue::Language("en".into())),
        QueryNode::filter(FilterValue::Years(YearRange {
            from: Some(2021),
            to: None,
        })),
    ]);
    assert_eq!(oa.express(&tree), Ok(()));
    let page = block_on(oa.search(&tree_query(tree))).unwrap().value;
    assert_eq!(
        query(&http, 0),
        pairs([
            ("search", "protein structure"),
            (
                "filter",
                r#"title.search:alphafold OR "structure prediction",raw_author_name.search:!jumper,is_retracted:false,language:en,from_publication_date:2021-01-01"#
            ),
            ("per-page", "3"),
            ("cursor", "*"),
        ])
    );
    for hit in &page.hits {
        let w = &hit.record.work;
        let title = w.title.as_deref().unwrap().to_lowercase();
        assert!(title.contains("alphafold") || title.contains("structure prediction"));
        assert!(w.issued.as_ref().unwrap().year() >= 2021);
        assert!(
            w.authors
                .iter()
                .all(|a| a.family.as_deref() != Some("Jumper"))
        );
    }
}

#[test]
fn relations_and_any_of_one_filter_are_filters() {
    let (oa, http) = provider([recorded("works_search_relation")]);
    let tree = QueryNode::All(vec![
        QueryNode::relation(Relation::Cites, "W2919115771"),
        QueryNode::term(TermField::Title, "medical"),
        QueryNode::Any(vec![
            QueryNode::filter(FilterValue::WorkType(WorkType::JournalArticle)),
            QueryNode::filter(FilterValue::WorkType(WorkType::Other("review".into()))),
        ]),
    ]);
    let mut q = tree_query(tree);
    q.sort = Sort::MostCited;
    let page = block_on(oa.search(&q)).unwrap().value;
    assert_eq!(
        query(&http, 0),
        pairs([
            ("sort", "cited_by_count:desc"),
            (
                "filter",
                "cites:W2919115771,title.search:medical,type:article|review"
            ),
            ("per-page", "3"),
            ("cursor", "*"),
        ])
    );
    assert_eq!(
        page.hits[0].record.work.title.as_deref(),
        Some("A survey on deep learning in medical image analysis")
    );
}

#[test]
fn author_ids_and_flags_are_filters() {
    let (oa, http) = provider([recorded("works_search_orcid")]);
    let tree = QueryNode::All(vec![
        QueryNode::Any(vec![
            QueryNode::term(TermField::AuthorId, "https://orcid.org/0000-0002-3271-7861"),
            QueryNode::term(TermField::AuthorId, "0000-0001-6187-6610"),
        ]),
        QueryNode::filter(FilterValue::HasAbstract),
        QueryNode::filter(FilterValue::OpenAccess(true)),
    ]);
    let page = block_on(oa.search(&tree_query(tree))).unwrap().value;
    assert_eq!(
        query(&http, 0),
        pairs([
            (
                "filter",
                "authorships.author.orcid:0000-0002-3271-7861|0000-0001-6187-6610,has_abstract:true,is_oa:true"
            ),
            ("per-page", "3"),
            ("cursor", "*"),
        ])
    );
    assert!(page.hits.iter().all(|h| {
        let oa = h.record.work.open_access.as_ref().unwrap();
        oa.is_oa && h.record.work.abstract_text.is_some()
    }));
}

#[test]
fn trees_openalex_cant_say_fail_before_sending() {
    let (oa, http) = provider([]);
    let across = QueryNode::Any(vec![
        QueryNode::term(TermField::Title, "a"),
        QueryNode::term(TermField::Author, "b"),
    ]);
    let reason = "OpenAlex can't combine title and author with any";
    assert_eq!(oa.express(&across).unwrap_err(), reason);
    match block_on(oa.search(&tree_query(across))) {
        Err(Error::UnsupportedQuery(r)) => assert_eq!(r, reason),
        other => panic!("{other:?}"),
    }
    // Relations name OpenAlex works by the time they're searched.
    let doi = QueryNode::relation(Relation::Cites, "10.1038/nature14539");
    assert_eq!(oa.express(&doi), Ok(()));
    assert!(matches!(
        block_on(oa.search(&tree_query(doi))),
        Err(Error::BadRequest(_))
    ));
    assert!(http.requests().is_empty());
}

#[test]
fn journals_in_trees_are_sources_kept_or_left_out() {
    let empty = r#"{"meta": {"count": 0}, "results": []}"#;
    let (oa, http) = provider([
        recorded("sources_named"),
        response(200, &[("x-ratelimit-cost-usd", "0.001")], empty),
        response(200, &[("x-ratelimit-cost-usd", "0.001")], empty),
    ]);
    let tree = QueryNode::All(vec![
        QueryNode::term(TermField::Keywords, "x"),
        QueryNode::None(vec![QueryNode::phrase(
            TermField::Container,
            "nature machine",
        )]),
        // Named by no source: nothing to leave out.
        QueryNode::None(vec![QueryNode::term(
            TermField::Container,
            "no such journal",
        )]),
    ]);
    let resp = block_on(oa.search(&tree_query(tree))).unwrap();
    assert_eq!(
        query(&http, 0)[0],
        pairs([("filter", r#"display_name.search:"nature machine""#)])[0]
    );
    let filter = query(&http, 2)
        .into_iter()
        .find(|(k, _)| k == "filter")
        .unwrap()
        .1;
    assert_eq!(
        filter,
        "primary_location.source.id:!S2912241403|S7407104579"
    );
    assert_eq!(resp.usage.unwrap().cost, Some(0.003));
}

#[test]
fn records_carry_the_works_they_cite() {
    let (oa, _) = provider([recorded("work_by_doi")]);
    let resp = block_on(oa.lookup(&[id("10.1038/nature14539")], FieldSet::DEFAULT)).unwrap();
    let record = resp.value[0].record.as_ref().unwrap();
    let cited = record.references.as_ref().unwrap();
    // As `referenced_works_count` says, each as an OpenAlex work id.
    assert_eq!(cited.len(), 62);
    assert!(
        cited
            .iter()
            .all(|w| w.to_string().starts_with("openalex:W"))
    );
}

/// OpenAlex lists no references for works it has no reference data for
/// (here, a book indexed from Crossref): unknown, not "cites nothing".
#[test]
fn works_without_listed_references_have_unknown_references() {
    let (oa, _) = provider([recorded("work_no_references")]);
    let doi = id("10.1176/appi.books.9780890425596");
    let resp = block_on(oa.lookup(&[doi], FieldSet::DEFAULT)).unwrap();
    let record = resp.value[0].record.as_ref().unwrap();
    assert!(record.raw.body.contains(r#""referenced_works":[]"#));
    assert_eq!(record.references, None);
}

/// The check of a hundred works for retractions: an id filter with
/// `is_retracted:true`, answering only the retracted ones ($0.0001).
#[test]
fn retracted_works_among_ids_come_from_one_filter() {
    let (oa, http) = provider([recorded("works_retracted_batch")]);
    let mut q = SearchQuery::new("");
    q.page.limit = 100;
    let filter = "openalex:W2158048826|W2919115771|W3010930696,is_retracted:true";
    q.extra.insert("filter".into(), filter.into());
    let resp = block_on(oa.search(&q)).unwrap();
    assert_eq!(
        query(&http, 0),
        pairs([("filter", filter), ("per-page", "100"), ("cursor", "*")])
    );
    let found: Vec<String> = resp
        .value
        .hits
        .iter()
        .map(|h| h.record.ids[0].to_string())
        .collect();
    assert_eq!(found, ["openalex:W2158048826", "openalex:W3010930696"]);
    assert!(
        resp.value
            .hits
            .iter()
            .all(|h| h.record.work.retraction().is_some())
    );
    assert_eq!(resp.usage.unwrap().cost, Some(0.0001));
}

/// OpenAlex finds a work by a DOI its record no longer carries (the
/// journal changed publisher and re-registered it): in a batch, that
/// record answers the key that found nothing, as the free lookup of the
/// DOI alone does.
#[test]
fn batches_find_works_by_dois_they_no_longer_carry() {
    let old = "10.1197/j.aem.2003.09.015";
    let (oa, http) = provider([recorded("works_batch_reregistered")]);
    let resp =
        block_on(oa.lookup(&[id(old), id("10.1038/nature14539")], FieldSet::DEFAULT)).unwrap();
    let record = resp.value[0].record.as_ref().unwrap();
    assert_eq!(
        ids(record),
        [
            "openalex:W2153551409",
            "doi:10.1111/j.1553-2712.2004.tb01439.x",
            "pmid:14759970"
        ]
    );
    assert!(resp.value[1].record.is_some());
    assert_eq!(http.requests().len(), 1, "one key missed, one record over");

    // Two keys missed and a record over: each missed key asked alone.
    let (oa, http) = provider([
        recorded("works_batch_reregistered"),
        recorded("work_reregistered"),
        recorded("work_not_found"),
    ]);
    let asked = [
        id(old),
        id("10.1038/nature14539"),
        id("10.5555/aspero-does-not-exist"),
    ];
    let resp = block_on(oa.lookup(&asked, FieldSet::DEFAULT)).unwrap();
    let urls: Vec<String> = http.requests().into_iter().map(|r| r.url).collect();
    assert_eq!(
        urls[1..],
        [
            format!("https://api.openalex.org/works/doi:{old}"),
            "https://api.openalex.org/works/doi:10.5555/aspero-does-not-exist".to_owned(),
        ]
    );
    let found: Vec<Option<String>> = resp
        .value
        .iter()
        .map(|l| l.record.as_ref().map(|r| r.ids[0].to_string()))
        .collect();
    assert_eq!(
        found,
        [
            Some("openalex:W2153551409".into()),
            Some("openalex:W2919115771".into()),
            None
        ]
    );
}

/// The works citing any of a few works, as `related` reads them: a
/// `cites:` filter, most cited first, 200 a page ($0.0001), each work only
/// its id, year and references. Which of the two each cites is in its
/// references.
#[test]
fn citing_works_come_lean_from_a_cites_filter() {
    let (oa, http) = provider([recorded("works_cites_page")]);
    let mut q = SearchQuery::new("");
    q.page.limit = 500;
    let filter = "cites:W2293082134|W2568812702";
    q.extra.insert("filter".into(), filter.into());
    q.extra.insert(
        "select".into(),
        "id,publication_year,referenced_works".into(),
    );
    q.extra.insert("sort".into(), "cited_by_count:desc".into());
    let resp = block_on(oa.search(&q)).unwrap();
    // Capped at OpenAlex's 200 a page.
    assert_eq!(
        query(&http, 0),
        pairs([
            ("select", "id,publication_year,referenced_works"),
            ("sort", "cited_by_count:desc"),
            ("filter", filter),
            ("per-page", "200"),
            ("cursor", "*"),
        ])
    );
    let page = resp.value;
    // The two works' `cited_by_count`s (4 and 3), on one page.
    assert_eq!(page.total, Some(7));
    assert_eq!(page.hits.len(), 7);
    assert!(page.next.is_none());
    let (a, b) = (id("W2293082134"), id("W2568812702"));
    let citing: Vec<(String, bool, bool)> = page
        .hits
        .iter()
        .map(|h| {
            let cited = h.record.references.as_ref().unwrap();
            (
                ids(&h.record).join(" "),
                cited.contains(&a),
                cited.contains(&b),
            )
        })
        .collect();
    assert_eq!(citing.iter().filter(|(_, a, _)| *a).count(), 4);
    assert_eq!(citing.iter().filter(|(_, _, b)| *b).count(), 3);
    // Only the id and year: a lean record.
    assert_eq!(citing[0].0, "openalex:W2952073233");
    assert!(page.hits.iter().all(|h| h.record.work.title.is_none()));
    assert!(page.hits.iter().all(|h| h.record.work.issued.is_some()));
    assert_eq!(resp.usage.unwrap().cost, Some(0.0001));
}

#[test]
fn since_is_refused_without_asking() {
    let (oa, http) = provider([]);
    assert!(
        !oa.capabilities()
            .search_filters
            .contains(&aspero_core::Filter::Since)
    );
    let reason = "OpenAlex can't search by date added (it needs a paid plan)";
    assert_eq!(oa.since_unsupported(), reason);
    let mut q = SearchQuery::new("sepsis");
    q.since = Some(jiff::civil::date(2026, 11, 1));
    match block_on(oa.search(&q)) {
        Err(Error::UnsupportedQuery(r)) => assert_eq!(r, reason),
        other => panic!("{other:?}"),
    }
    assert!(http.requests().is_empty());
}
