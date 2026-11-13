//! Crossref provider against responses recorded by `scripts/record-crossref.sh`.

use std::sync::Arc;
use std::time::Duration;

use aspero_core::testing::{FIXTURE_TIME, ReplayClient, block_on, fixture, response};
use aspero_core::{
    Error, FieldSet, FilterValue, HttpResponse, Provider, QueryNode, SearchQuery, Sort, TermField,
    WorkId, WorkType, YearRange,
};
use aspero_crossref::{Crossref, CrossrefConfig};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn recorded(name: &str) -> HttpResponse {
    fixture(FIXTURES, name)
}

fn provider(responses: impl IntoIterator<Item = HttpResponse>) -> (Crossref, Arc<ReplayClient>) {
    let http = Arc::new(ReplayClient::new(responses));
    let mut config = CrossrefConfig::default();
    config.mailto = Some("test@example.org".into());
    (Crossref::new(config, http.clone()), http)
}

fn id(s: &str) -> WorkId {
    s.parse().unwrap()
}

fn query(http: &ReplayClient, n: usize) -> Vec<(String, String)> {
    let url = url::Url::parse(&http.requests()[n].url).unwrap();
    url.query_pairs().into_owned().collect()
}

fn pair(k: &str, v: &str) -> (String, String) {
    (k.to_owned(), v.to_owned())
}

#[test]
fn single_lookup_maps_bibliographic_fields() {
    let (cr, http) = provider([recorded("work_by_doi")]);
    let resp = block_on(cr.lookup(&[id("10.1038/nature14539")], FieldSet::DEFAULT)).unwrap();

    assert_eq!(
        http.requests()[0].url,
        "https://api.crossref.org/works/10.1038/nature14539?mailto=test%40example.org"
    );
    let record = resp.value[0].record.as_ref().unwrap();
    assert_eq!(record.ids, [id("10.1038/nature14539")]);
    assert_eq!(record.fetched_at, FIXTURE_TIME);
    let w = &record.work;
    assert_eq!(w.title.as_deref(), Some("Deep learning"));
    assert_eq!(w.issued.unwrap().to_string(), "2015-05-27");
    assert_eq!(w.work_type, Some(WorkType::JournalArticle));
    assert_eq!(w.container_title.as_deref(), Some("Nature"));
    assert_eq!(w.page.as_deref(), Some("436-444"));
    let first = &w.authors[0];
    assert_eq!(
        (first.given.as_deref(), first.family.as_deref()),
        (Some("Yann"), Some("LeCun"))
    );
    // Raw is the work object itself, not the {status, message} envelope.
    assert!(record.raw.body.starts_with('{') && !record.raw.body.contains("\"message-type\""));
    assert!(resp.usage.is_none());
}

#[test]
fn proceedings_articles_are_conference_papers() {
    let (cr, _) = provider([recorded("work_conference")]);
    let resp = block_on(cr.lookup(&[id("10.1145/3368089.3409741")], FieldSet::DEFAULT)).unwrap();
    let w = &resp.value[0].record.as_ref().unwrap().work;
    assert_eq!(w.work_type, Some(WorkType::ConferencePaper));
    assert_eq!(w.page.as_deref(), Some("292-303"));
}

#[test]
fn unknown_doi_is_an_absent_record() {
    let (cr, _) = provider([recorded("work_not_found")]);
    let resp =
        block_on(cr.lookup(&[id("10.5555/aspero-does-not-exist")], FieldSet::DEFAULT)).unwrap();
    assert!(resp.value[0].record.is_none());
}

#[test]
fn batch_lookup_answers_every_doi_in_request_order() {
    let (cr, http) = provider([recorded("works_batch_doi")]);
    let requested = [
        id("10.1038/nature14539"),
        id("10.1145/3368089.3409741"),
        id("10.5555/aspero-does-not-exist"),
    ];
    let resp = block_on(cr.lookup(&requested, FieldSet::DEFAULT)).unwrap();

    assert!(query(&http, 0).contains(&pair(
        "filter",
        "doi:10.1038/nature14539,doi:10.1145/3368089.3409741,doi:10.5555/aspero-does-not-exist"
    )));
    let titles: Vec<_> = resp
        .value
        .iter()
        .map(|l| l.record.as_ref().map(|r| r.work.title.clone().unwrap()))
        .collect();
    assert_eq!(titles[0].as_deref(), Some("Deep learning"));
    assert!(
        titles[1]
            .as_deref()
            .unwrap()
            .starts_with("Efficient incident identification")
    );
    assert_eq!(titles[2], None);
}

#[test]
fn citation_mode_uses_the_bibliographic_matcher() {
    let (cr, http) = provider([recorded("works_bibliographic_ref")]);
    let mut q = SearchQuery::citation(
        "LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015;521:436-444.",
    );
    q.page.limit = 3;
    let resp = block_on(cr.search(&q)).unwrap();

    let params = query(&http, 0);
    assert!(params.iter().any(|(k, _)| k == "query.bibliographic"));
    assert!(params.contains(&pair("rows", "3")));
    // Cursor paging drops relevance order unless asked for explicitly.
    assert!(params.contains(&pair("sort", "score")) && params.contains(&pair("order", "desc")));

    let hits = resp.value.hits;
    assert_eq!(hits[0].record.ids, [id("10.1038/nature14539")]);
    let (top, second) = (hits[0].score.unwrap(), hits[1].score.unwrap());
    assert!(top > 1.5 * second, "a clear winner: {top} vs {second}");
}

/// Recorded evidence for `resolve`: Crossref's best matches for this famous
/// arXiv paper are 2025 re-deposits under another publisher, because the
/// real paper's only DOI is registered with DataCite.
#[test]
fn citation_matches_can_be_lookalikes() {
    let (cr, _) = provider([recorded("works_bibliographic")]);
    let resp = block_on(cr.search(&SearchQuery::citation(
        "Vaswani 2017 Attention is all you need",
    )))
    .unwrap();
    let top = &resp.value.hits[0].record.work;
    assert_eq!(top.title.as_deref(), Some("Attention Is All You Need"));
    assert_eq!(top.authors[0].family.as_deref(), Some("Vaswani"));
    assert_eq!(top.issued.unwrap().year(), 2025);
    assert_eq!(top.work_type, Some(WorkType::Preprint));
}

#[test]
fn keyword_search_applies_filters_and_pages_by_cursor() {
    let (cr, http) = provider([recorded("works_search_filtered")]);
    let mut q = SearchQuery::new("transformer");
    q.years = Some(YearRange {
        from: Some(2017),
        to: Some(2018),
    });
    q.work_type = Some(WorkType::JournalArticle);
    q.page.limit = 3;
    let resp = block_on(cr.search(&q)).unwrap();

    let params = query(&http, 0);
    assert!(params.contains(&pair("query", "transformer")));
    assert!(params.contains(&pair(
        "filter",
        "from-pub-date:2017,until-pub-date:2018,type:journal-article"
    )));
    assert!(params.contains(&pair("cursor", "*")));
    assert!(params.contains(&pair("sort", "score")));
    assert!(params.contains(&pair("mailto", "test@example.org")));

    let page = resp.value;
    assert_eq!(page.total, Some(1813));
    assert!(page.next.is_some());
    assert_eq!(page.hits.len(), 3);
    let scores: Vec<f64> = page.hits.iter().map(|h| h.score.unwrap()).collect();
    assert!(scores.is_sorted_by(|a, b| a >= b), "{scores:?}");
    // Crossref sends entities and JATS tags in titles; works have plain text.
    let containers: Vec<&str> = page
        .hits
        .iter()
        .filter_map(|h| h.record.work.container_title.as_deref())
        .collect();
    assert!(
        containers.contains(&"International Journal of Modern Trends in Engineering & Research"),
        "{containers:?}"
    );
    assert!(page.hits.iter().all(|h| {
        let year = h.record.work.issued.unwrap().year();
        (2017..=2018).contains(&year)
    }));
}

#[test]
fn bad_filter_surfaces_crossref_validation_message() {
    let (cr, _) = provider([recorded("bad_filter")]);
    let mut q = SearchQuery::new("x");
    q.extra.insert("filter".into(), "no-such-filter:1".into());
    let Err(Error::BadRequest(msg)) = block_on(cr.search(&q)) else {
        panic!("expected BadRequest");
    };
    assert!(
        msg.starts_with("Filter 'no-such-filter' specified but there is no such filter"),
        "{msg}"
    );
}

#[test]
fn query_extras_pass_through_and_unknown_extras_are_rejected() {
    let (cr, http) = provider([recorded("works_search_filtered")]);
    let mut q = SearchQuery::new("");
    q.extra.insert("query.author".into(), "Hinton".into());
    block_on(cr.search(&q)).unwrap();
    assert!(query(&http, 0).contains(&pair("query.author", "Hinton")));

    q.extra.insert("select".into(), "DOI".into());
    assert!(matches!(block_on(cr.search(&q)), Err(Error::BadRequest(_))));
    assert_eq!(http.requests().len(), 1);
}

fn empty_page() -> HttpResponse {
    response(
        200,
        &[],
        r#"{"status": "ok", "message": {"total-results": 0, "items": []}}"#,
    )
}

#[test]
fn fields_become_field_queries_ranked_by_score() {
    let (cr, http) = provider([recorded("works_search_fields")]);
    let mut q = SearchQuery::new("");
    q.author = Some("LeCun".into());
    q.title = Some("Deep learning".into());
    q.container = Some(" Nature ".into());
    q.page.limit = 5;
    let page = block_on(cr.search(&q)).unwrap().value;
    let first = &page.hits[0].record;
    assert_eq!(first.ids, [id("10.1038/nature14539")]);
    let scores: Vec<f64> = page.hits.iter().map(|h| h.score.unwrap()).collect();
    assert!(scores.is_sorted_by(|a, b| a >= b), "{scores:?}");
    assert_eq!(
        query(&http, 0),
        [
            pair("query.author", "LeCun"),
            pair("query.title", "Deep learning"),
            pair("query.container-title", "Nature"),
            pair("sort", "score"),
            pair("order", "desc"),
            pair("rows", "5"),
            pair("cursor", "*"),
            pair("mailto", "test@example.org"),
        ]
    );
}

#[test]
fn newest_first_sorts_by_published() {
    let (cr, http) = provider([recorded("works_search_newest")]);
    let mut q = SearchQuery::new("deep learning");
    q.sort = Sort::Newest;
    q.page.limit = 3;
    let page = block_on(cr.search(&q)).unwrap().value;
    assert_eq!(
        query(&http, 0),
        [
            pair("query", "deep learning"),
            pair("sort", "published"),
            pair("order", "desc"),
            pair("rows", "3"),
            pair("offset", "0"),
            pair("mailto", "test@example.org"),
        ]
    );
    assert_eq!(page.hits.len(), 3);
    // Crossref refuses date sorts with a cursor: the next page is by offset.
    assert_eq!(page.next.map(|c| c.0).as_deref(), Some("offset:3"));
}

#[test]
fn most_cited_first_sorts_by_references_to_the_work() {
    let (cr, http) = provider([recorded("works_search_most_cited")]);
    let mut q = SearchQuery::new("deep learning");
    q.sort = Sort::MostCited;
    q.page.limit = 3;
    let page = block_on(cr.search(&q)).unwrap().value;
    assert_eq!(
        query(&http, 0),
        [
            pair("query", "deep learning"),
            pair("sort", "is-referenced-by-count"),
            pair("order", "desc"),
            pair("rows", "3"),
            pair("cursor", "*"),
            pair("mailto", "test@example.org"),
        ]
    );
    let counts: Vec<u64> = page
        .hits
        .iter()
        .map(|h| h.record.work.cited_by_count.unwrap())
        .collect();
    assert!(counts.is_sorted_by(|a, b| a >= b), "{counts:?}");
}

#[test]
fn a_sort_given_as_an_extra_wins() {
    let (cr, http) = provider([empty_page()]);
    let mut q = SearchQuery::new("deep learning");
    q.sort = Sort::MostCited;
    q.extra.insert("sort".into(), "deposited".into());
    block_on(cr.search(&q)).unwrap();
    let sort: Vec<_> = query(&http, 0)
        .into_iter()
        .filter(|(k, _)| k == "sort" || k == "order")
        .collect();
    assert_eq!(sort, [pair("sort", "deposited")]);
}

#[test]
fn citation_text_and_a_title_are_separate_queries() {
    let (cr, http) = provider([empty_page()]);
    let mut q = SearchQuery::citation("LeCun Y. Nature. 2015");
    q.title = Some("Deep learning".into());
    block_on(cr.search(&q)).unwrap();
    let params = query(&http, 0);
    assert!(params.contains(&pair("query.bibliographic", "LeCun Y. Nature. 2015")));
    assert!(params.contains(&pair("query.title", "Deep learning")));
}

#[test]
fn transport_statuses_map_to_errors() {
    let (cr, _) = provider([
        response(429, &[("retry-after", "3")], ""),
        response(502, &[], "Bad Gateway"),
    ]);
    let q = SearchQuery::new("x");
    assert!(matches!(
        block_on(cr.search(&q)),
        Err(Error::RateLimited { retry_after: Some(d) }) if d == Duration::from_secs(3)
    ));
    assert!(matches!(
        block_on(cr.search(&q)),
        Err(Error::Server { status: 502, .. })
    ));
}

#[test]
fn a_record_that_does_not_decode_is_skipped_not_the_page() {
    let body = r#"{"status": "ok", "message": {"total-results": 3, "items": [
        {"DOI": "10.1000/a", "title": ["A"]},
        {"DOI": "10.1000/b", "title": 42},
        {"DOI": "10.1000/c", "title": ["C"]}
    ]}}"#;
    let (cr, _) = provider([response(200, &[], body)]);
    let page = block_on(cr.search(&SearchQuery::new("x"))).unwrap().value;
    let titles: Vec<_> = page
        .hits
        .iter()
        .map(|h| h.record.work.title.as_deref())
        .collect();
    assert_eq!(titles, [Some("A"), Some("C")]);
}

#[test]
fn null_reads_as_an_absent_field() {
    let body = r#"{"status": "ok", "message": {"DOI": "10.1000/a", "title": ["A"],
        "author": null, "container-title": null, "issued": null, "relation": null}}"#;
    let (cr, _) = provider([response(200, &[], body)]);
    let resp = block_on(cr.lookup(&[id("10.1000/a")], FieldSet::DEFAULT)).unwrap();
    let work = &resp.value[0].record.as_ref().unwrap().work;
    assert_eq!(work.title.as_deref(), Some("A"));
    assert!(work.authors.is_empty());
}

#[test]
fn only_dois_are_supported() {
    let (cr, http) = provider([]);
    let err = block_on(cr.lookup(&[id("arXiv:1706.03762")], FieldSet::DEFAULT)).unwrap_err();
    assert!(matches!(err, Error::Unsupported(_)));
    assert!(http.requests().is_empty());
}

#[test]
fn retraction_watch_notices_are_mapped() {
    let (cr, _) = provider([recorded("work_retracted")]);
    let resp =
        block_on(cr.lookup(&[id("10.1016/S0140-6736(97)11096-0")], FieldSet::DEFAULT)).unwrap();
    let w = &resp.value[0].record.as_ref().unwrap().work;
    let r = w.retraction().expect("retracted");
    assert_eq!(r.date.unwrap().to_string(), "2010-02-06");
    assert_eq!(
        r.notice.as_ref().unwrap().to_string(),
        "doi:10.1016/s0140-6736(10)60175-4"
    );
    assert_eq!(r.source.as_deref(), Some("retraction-watch"));
    assert!(
        w.updates
            .iter()
            .any(|u| u.kind == aspero_core::UpdateKind::Correction)
    );
}

fn tree_query(tree: QueryNode) -> SearchQuery {
    let mut q = SearchQuery::new("");
    q.tree = Some(tree);
    q.page.limit = 3;
    q
}

fn years(from: i16, to: i16) -> QueryNode {
    QueryNode::filter(FilterValue::Years(YearRange {
        from: Some(from),
        to: Some(to),
    }))
}

#[test]
fn trees_are_all_of_query_fields_and_filters() {
    let (cr, http) = provider([recorded("works_search_tree")]);
    let tree = QueryNode::All(vec![
        QueryNode::term(TermField::Title, "deep"),
        QueryNode::term(TermField::Author, "lecun"),
        QueryNode::term(TermField::Title, "learning"),
        years(2010, 2015),
        years(2015, 2020),
        QueryNode::filter(FilterValue::WorkType(WorkType::JournalArticle)),
    ]);
    let page = block_on(cr.search(&tree_query(tree))).unwrap().value;
    // Terms of one field share its parameter; overlapping years narrow.
    assert_eq!(
        query(&http, 0),
        [
            pair("query.title", "deep learning"),
            pair("query.author", "lecun"),
            pair(
                "filter",
                "from-pub-date:2015,until-pub-date:2015,type:journal-article"
            ),
            pair("sort", "score"),
            pair("order", "desc"),
            pair("rows", "3"),
            pair("cursor", "*"),
            pair("mailto", "test@example.org"),
        ]
    );
    assert_eq!(
        page.hits[0].record.work.title.as_deref(),
        Some("Deep learning")
    );
}

#[test]
fn trees_crossref_cant_say_and_why() {
    let (cr, http) = provider([]);
    let reason = |tree: QueryNode| cr.express(&tree).unwrap_err();
    assert_eq!(
        reason(QueryNode::Any(vec![
            QueryNode::term(TermField::Title, "a"),
            QueryNode::term(TermField::Title, "b"),
        ])),
        "Crossref can't combine terms with any"
    );
    assert_eq!(
        reason(QueryNode::None(vec![QueryNode::term(
            TermField::Title,
            "a"
        )])),
        "Crossref can't leave out terms with none"
    );
    assert_eq!(
        reason(QueryNode::phrase(TermField::Title, "deep learning")),
        "Crossref can't search for phrases"
    );
    assert_eq!(
        reason(QueryNode::term(TermField::Abstract, "x")),
        "Crossref can't search by abstract"
    );
    assert_eq!(
        reason(QueryNode::filter(FilterValue::NotRetracted)),
        "Crossref can't leave out retracted works"
    );
    assert_eq!(
        reason(QueryNode::term(TermField::AuthorId, "A5023888391")),
        "Crossref finds authors by ORCID, not OpenAlex author ids"
    );
    assert_eq!(
        reason(QueryNode::All(vec![
            QueryNode::filter(FilterValue::WorkType(WorkType::Book)),
            QueryNode::filter(FilterValue::WorkType(WorkType::Chapter)),
        ])),
        "Crossref can't require two work types at once"
    );
    // An ORCID and an abstract are filters.
    let (cr2, http2) = provider([response(200, &[], r#"{"message": {"items": []}}"#)]);
    let tree = QueryNode::All(vec![
        QueryNode::term(TermField::Keywords, "x"),
        QueryNode::term(TermField::AuthorId, "https://orcid.org/0000-0002-1825-0097"),
        QueryNode::filter(FilterValue::HasAbstract),
    ]);
    block_on(cr2.search(&tree_query(tree))).unwrap();
    assert!(query(&http2, 0).contains(&pair(
        "filter",
        "has-abstract:true,orcid:0000-0002-1825-0097"
    )));
    assert!(http.requests().is_empty());
}

fn id_term(v: &str) -> QueryNode {
    QueryNode::term(TermField::Identifier, v)
}

#[test]
fn an_any_of_dois_is_repeated_doi_filters() {
    let (cr, http) = provider([recorded("works_search_ids")]);
    let tree = QueryNode::All(vec![
        QueryNode::Any(vec![
            id_term("https://doi.org/10.1038/nature14539"),
            id_term("doi:10.1145/3065386"),
        ]),
        QueryNode::filter(FilterValue::Years(YearRange {
            from: Some(2015),
            to: None,
        })),
    ]);
    let page = block_on(cr.search(&tree_query(tree))).unwrap().value;
    assert_eq!(
        query(&http, 0),
        [
            pair(
                "filter",
                "from-pub-date:2015,doi:10.1038/nature14539,doi:10.1145/3065386"
            ),
            pair("rows", "3"),
            pair("cursor", "*"),
            pair("mailto", "test@example.org"),
        ]
    );
    assert_eq!(page.hits.len(), 2);
    // One DOI, with keywords: a query and a filter, ranked by score.
    let (cr, http) = provider([empty_page()]);
    let tree = QueryNode::All(vec![
        QueryNode::term(TermField::Keywords, "deep learning"),
        QueryNode::phrase(TermField::Identifier, "10.1038/nature14539"),
    ]);
    block_on(cr.search(&tree_query(tree))).unwrap();
    let sent = query(&http, 0);
    assert!(sent.contains(&pair("query", "deep learning")));
    assert!(sent.contains(&pair("filter", "doi:10.1038/nature14539")));
}

#[test]
fn identifiers_crossref_cant_search_by_and_why() {
    let (cr, http) = provider([]);
    let reason = |tree: QueryNode| cr.express(&tree).unwrap_err();
    for other in [
        "pmid:26017442",
        "PMC7254481",
        "W2919115771",
        "978-0-262-03561-3",
    ] {
        assert_eq!(
            reason(id_term(other)),
            "Crossref finds works by DOI only",
            "{other}"
        );
    }
    assert_eq!(
        reason(QueryNode::Any(vec![
            id_term("10.1038/nature14539"),
            id_term("arXiv:1706.03762"),
        ])),
        "Crossref has no arXiv works: their DOIs are DataCite's"
    );
    assert_eq!(
        reason(QueryNode::All(vec![
            id_term("10.1038/nature14539"),
            id_term("10.1145/3065386"),
        ])),
        "Crossref can't require two DOIs at once, only any of them"
    );
    assert_eq!(
        reason(QueryNode::None(vec![id_term("10.1038/nature14539")])),
        "Crossref can't leave out terms with none"
    );
    assert_eq!(
        reason(QueryNode::Any(vec![
            id_term("10.1038/nature14539"),
            QueryNode::term(TermField::Title, "deep learning"),
        ])),
        "Crossref can't combine terms with any"
    );
    assert_eq!(
        reason(id_term("10.1000/a,b")),
        "Crossref can't filter by a DOI holding a comma"
    );
    assert!(http.requests().is_empty());
}

#[test]
fn since_is_the_first_deposit_with_text_or_a_tree() {
    let (cr, http) = provider([recorded("works_search_since"), empty_page()]);
    let since = Some(jiff::civil::date(2026, 11, 1));
    let mut q = SearchQuery::new("sepsis");
    q.since = since;
    q.page.limit = 3;
    let page = block_on(cr.search(&q)).unwrap().value;
    assert_eq!(
        query(&http, 0),
        [
            pair("query", "sepsis"),
            pair("filter", "from-created-date:2026-11-01"),
            pair("sort", "score"),
            pair("order", "desc"),
            pair("rows", "3"),
            pair("cursor", "*"),
            pair("mailto", "test@example.org"),
        ]
    );
    assert_eq!(page.total, Some(399));
    assert_eq!(page.hits.len(), 3);
    // Added since, whenever published: one was issued in October.
    let months: Vec<Option<u8>> = page
        .hits
        .iter()
        .map(|h| h.record.work.issued.unwrap().month())
        .collect();
    assert_eq!(months, [Some(11), Some(10), Some(11)]);

    // A tree's filters join it.
    let mut q = tree_query(QueryNode::All(vec![
        QueryNode::term(TermField::Title, "sepsis"),
        years(2026, 2026),
    ]));
    q.since = since;
    block_on(cr.search(&q)).unwrap();
    assert!(query(&http, 1).contains(&pair(
        "filter",
        "from-created-date:2026-11-01,from-pub-date:2026,until-pub-date:2026"
    )));
}
