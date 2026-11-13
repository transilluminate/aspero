//! DataCite provider against responses recorded by `scripts/record-datacite.sh`.

use std::sync::Arc;

use aspero_core::testing::{ReplayClient, block_on, fixture, response};
use aspero_core::{
    Cursor, Error, FieldSet, FilterValue, HttpResponse, Provider, QueryNode, SearchQuery, Sort,
    TermField, WorkId, WorkType, YearRange,
};
use aspero_datacite::{Datacite, DataciteConfig};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn recorded(name: &str) -> HttpResponse {
    fixture(FIXTURES, name)
}

fn provider(responses: impl IntoIterator<Item = HttpResponse>) -> (Datacite, Arc<ReplayClient>) {
    let http = Arc::new(ReplayClient::new(responses));
    (Datacite::new(DataciteConfig::default(), http.clone()), http)
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
fn arxiv_lookup_maps_record_with_abstract_and_arxiv_id() {
    let (dc, http) = provider([recorded("doi_arxiv")]);
    let resp = block_on(dc.lookup(&[id("arXiv:1706.03762v7")], FieldSet::DEFAULT)).unwrap();

    assert_eq!(
        http.requests()[0].url,
        "https://api.datacite.org/dois/10.48550/arxiv.1706.03762"
    );
    let record = resp.value[0].record.as_ref().unwrap();
    assert_eq!(
        record.ids,
        [id("10.48550/arXiv.1706.03762"), id("arXiv:1706.03762")]
    );
    let w = &record.work;
    assert_eq!(w.title.as_deref(), Some("Attention Is All You Need"));
    assert_eq!(w.work_type, Some(WorkType::Preprint));
    assert_eq!(w.issued.unwrap().year(), 2017);
    assert_eq!(w.publisher.as_deref(), Some("arXiv"));
    assert_eq!(w.authors[0].family.as_deref(), Some("Vaswani"));
    let abstract_text = w.abstract_text.as_deref().unwrap();
    assert!(abstract_text.starts_with("The dominant sequence transduction"));
    assert!(
        !abstract_text.contains("15 pages"),
        "only the Abstract description"
    );
}

#[test]
fn missing_doi_is_an_absent_record() {
    let (dc, _) = provider([recorded("doi_not_found")]);
    let resp =
        block_on(dc.lookup(&[id("10.5555/aspero-does-not-exist")], FieldSet::DEFAULT)).unwrap();
    assert!(resp.value[0].record.is_none());
}

#[test]
fn batch_lookup_answers_every_id_in_order() {
    let (dc, http) = provider([recorded("batch_doi")]);
    let requested = [
        id("10.48550/arXiv.1706.03762"),
        id("10.5281/zenodo.24110911"),
        id("10.5555/aspero-does-not-exist"),
    ];
    let resp = block_on(dc.lookup(&requested, FieldSet::DEFAULT)).unwrap();

    assert!(query(&http, 0).contains(&pair(
        "query",
        r#"doi:("10.48550/arxiv.1706.03762" OR "10.5281/zenodo.24110911" OR "10.5555/aspero-does-not-exist")"#
    )));
    let found: Vec<bool> = resp.value.iter().map(|l| l.record.is_some()).collect();
    assert_eq!(found, [true, true, false]);
}

#[test]
fn search_ranks_by_relevance_with_page_number_paging() {
    let (dc, http) = provider([recorded("search")]);
    let mut q = SearchQuery::new("attention is all you need");
    q.page.limit = 3;
    let resp = block_on(dc.search(&q)).unwrap();

    let params = query(&http, 0);
    assert!(params.contains(&pair("query", "(attention is all you need)")));
    assert!(params.contains(&pair("sort", "relevance")));
    assert!(params.contains(&pair("page[number]", "1")));
    assert!(params.contains(&pair("page[size]", "3")));

    let page = resp.value;
    assert_eq!(page.total, Some(2836));
    assert_eq!(page.next, Some(Cursor("2".into())));
    let titles: Vec<String> = page
        .hits
        .iter()
        .map(|h| h.record.work.title.clone().unwrap().to_lowercase())
        .collect();
    assert!(
        titles.iter().all(|t| t == "attention is all you need"),
        "{titles:?}"
    );
    // DataCite's 0 is "no citations it tracks", so none.
    let counts: Vec<Option<u64>> = page
        .hits
        .iter()
        .map(|h| h.record.work.cited_by_count)
        .collect();
    assert_eq!(counts, [None, None, Some(69)]);
}

#[test]
fn search_filters_by_year_range_and_type() {
    let (dc, http) = provider([recorded("search_filtered")]);
    let mut q = SearchQuery::new("transformer");
    q.years = Some(YearRange {
        from: Some(2017),
        to: Some(2018),
    });
    q.work_type = Some(WorkType::Preprint);
    q.page.limit = 3;
    let resp = block_on(dc.search(&q)).unwrap();

    let params = query(&http, 0);
    assert!(params.contains(&pair(
        "query",
        "(transformer) AND publicationYear:[2017 TO 2018]"
    )));
    assert!(params.contains(&pair("resource-type-id", "preprint")));
    assert!(resp.value.hits.iter().all(|h| {
        let w = &h.record.work;
        w.work_type == Some(WorkType::Preprint) && (2017..=2018).contains(&w.issued.unwrap().year())
    }));
}

#[test]
fn query_syntax_in_user_text_is_escaped() {
    let (dc, http) = provider([recorded("search")]);
    block_on(dc.search(&SearchQuery::new("BERT: pre-training (deep)"))).unwrap();
    assert!(query(&http, 0).contains(&pair("query", r"(BERT\: pre\-training \(deep\))")));
}

fn empty_page() -> HttpResponse {
    response(
        200,
        &[],
        r#"{"data": [], "meta": {"total": 0, "totalPages": 0}}"#,
    )
}

#[test]
fn authors_and_titles_are_escaped_field_clauses() {
    let (dc, http) = provider([empty_page()]);
    let mut q = SearchQuery::new("");
    q.author = Some("Vaswani".into());
    q.title = Some("Attention: all you need".into());
    block_on(dc.search(&q)).unwrap();
    let params = query(&http, 0);
    assert_eq!(
        params[0],
        pair(
            "query",
            r"creators.name:(Vaswani) AND titles.title:(Attention\: all you need)"
        )
    );
    assert!(params.contains(&pair("sort", "relevance")));
}

#[test]
fn most_cited_first_is_the_only_other_sort() {
    let (dc, http) = provider([empty_page()]);
    let mut q = SearchQuery::new("transformer");
    q.sort = Sort::MostCited;
    block_on(dc.search(&q)).unwrap();
    assert!(query(&http, 0).contains(&pair("sort", "-citation-count")));

    q.sort = Sort::Newest;
    assert!(matches!(
        block_on(dc.search(&q)),
        Err(Error::Unsupported(_))
    ));
    assert_eq!(http.requests().len(), 1);
}

#[test]
fn open_year_ranges_and_cursors_continue_paging() {
    let (dc, http) = provider([recorded("search")]);
    let mut q = SearchQuery::new("x");
    q.years = Some(YearRange {
        from: Some(2017),
        to: None,
    });
    q.page.cursor = Some(Cursor("4".into()));
    block_on(dc.search(&q)).unwrap();
    let params = query(&http, 0);
    assert!(params.contains(&pair("query", "(x) AND publicationYear:[2017 TO *]")));
    assert!(params.contains(&pair("page[number]", "4")));
}

#[test]
fn errors_map_with_datacite_messages() {
    let (dc, _) = provider([
        recorded("bad_request"),
        response(429, &[("retry-after", "5")], ""),
    ]);
    let mut q = SearchQuery::new("x");
    q.extra.insert("query".into(), "published:[bad".into());
    let Err(Error::BadRequest(msg)) = block_on(dc.search(&q)) else {
        panic!("expected BadRequest");
    };
    assert!(msg.starts_with("failed to parse date field"), "{msg}");
    assert!(matches!(
        block_on(dc.search(&q)),
        Err(Error::RateLimited { .. })
    ));

    let (dc, http) = provider([]);
    let mut bad = SearchQuery::new("x");
    bad.extra.insert("frobnicate".into(), "1".into());
    assert!(matches!(
        block_on(dc.search(&bad)),
        Err(Error::BadRequest(_))
    ));
    assert!(matches!(
        block_on(dc.search(&SearchQuery::citation("x"))),
        Err(Error::Unsupported(_))
    ));
    assert!(http.requests().is_empty());
}

fn tree_query(tree: QueryNode) -> SearchQuery {
    let mut q = SearchQuery::new("");
    q.tree = Some(tree);
    q.page.limit = 3;
    q
}

#[test]
fn trees_are_query_strings_across_fields() {
    let (dc, http) = provider([recorded("search_tree")]);
    let tree = QueryNode::All(vec![
        QueryNode::Any(vec![
            QueryNode::term(TermField::Title, "transformer"),
            QueryNode::phrase(TermField::Abstract, "attention mechanism"),
        ]),
        QueryNode::filter(FilterValue::WorkType(WorkType::Dataset)),
        QueryNode::filter(FilterValue::HasAbstract),
        QueryNode::None(vec![QueryNode::term(TermField::Author, "smith")]),
        QueryNode::filter(FilterValue::Years(YearRange {
            from: Some(2020),
            to: None,
        })),
    ]);
    let page = block_on(dc.search(&tree_query(tree))).unwrap().value;
    assert_eq!(
        query(&http, 0),
        [
            pair(
                "query",
                r#"(titles.title:(transformer) OR descriptions.description:"attention mechanism") AND types.resourceTypeGeneral:Dataset AND descriptions.descriptionType:Abstract AND (NOT creators.name:(smith)) AND publicationYear:[2020 TO *]"#
            ),
            pair("sort", "relevance"),
            pair("page[number]", "1"),
            pair("page[size]", "3"),
        ]
    );
    assert_eq!(page.hits.len(), 3);
    assert!(
        page.hits
            .iter()
            .all(|h| h.record.work.work_type == Some(WorkType::Dataset))
    );
}

#[test]
fn orcids_match_as_urls_or_bare_and_languages_both_ways() {
    let (dc, http) = provider([recorded("search_tree_orcid")]);
    let tree = QueryNode::All(vec![
        QueryNode::term(TermField::AuthorId, "0000-0001-6187-6610"),
        QueryNode::filter(FilterValue::Language("en".into())),
    ]);
    let page = block_on(dc.search(&tree_query(tree))).unwrap().value;
    assert_eq!(
        query(&http, 0)[0],
        pair(
            "query",
            r#"creators.nameIdentifiers.nameIdentifier:("https://orcid.org/0000-0001-6187-6610" OR "0000-0001-6187-6610") AND language:(en OR eng)"#
        )
    );
    assert_eq!(page.total, Some(8));
    let (dc, _) = provider([]);
    assert_eq!(
        dc.express(&QueryNode::term(TermField::AuthorId, "A5023888391"))
            .unwrap_err(),
        "DataCite finds authors by ORCID, not OpenAlex author ids"
    );
    assert_eq!(
        dc.express(&QueryNode::filter(FilterValue::NotRetracted))
            .unwrap_err(),
        "DataCite can't leave out retracted works"
    );
    assert_eq!(
        dc.express(&QueryNode::term(TermField::Container, "Nature"))
            .unwrap_err(),
        "DataCite can't search by journal"
    );
}

#[test]
fn identifiers_are_dois_arxiv_ids_by_theirs() {
    let (dc, http) = provider([recorded("search_tree_ids")]);
    let id = |v| QueryNode::term(TermField::Identifier, v);
    let tree = QueryNode::All(vec![
        QueryNode::Any(vec![
            id("arXiv:1706.03762v7"),
            id("https://doi.org/10.5281/zenodo.24110911"),
        ]),
        QueryNode::filter(FilterValue::Years(YearRange {
            from: Some(2017),
            to: Some(2017),
        })),
    ]);
    let page = block_on(dc.search(&tree_query(tree))).unwrap().value;
    assert_eq!(
        query(&http, 0)[0],
        pair(
            "query",
            r#"(doi:"10.48550/arxiv.1706.03762" OR doi:"10.5281/zenodo.24110911") AND publicationYear:[2017 TO 2017]"#
        )
    );
    assert_eq!(page.total, Some(1));
    let (dc, _) = provider([]);
    for other in [
        "pmid:26017442",
        "PMC7254481",
        "W2919115771",
        "978-0-262-03561-3",
    ] {
        assert_eq!(
            dc.express(&id(other)).unwrap_err(),
            "DataCite finds works by DOI or arXiv id only",
            "{other}"
        );
    }
}

#[test]
fn since_is_the_registration_date_anded_with_text_or_a_tree() {
    let (dc, http) = provider([recorded("search_since"), empty_page()]);
    let since = Some(jiff::civil::date(2026, 11, 1));
    let mut q = SearchQuery::new("sepsis");
    q.since = since;
    q.page.limit = 3;
    let page = block_on(dc.search(&q)).unwrap().value;
    assert_eq!(
        query(&http, 0),
        [
            pair("query", "(sepsis) AND created:[2026-11-01 TO *]"),
            pair("sort", "relevance"),
            pair("page[number]", "1"),
            pair("page[size]", "3"),
        ]
    );
    assert_eq!(page.total, Some(280));
    assert_eq!(page.hits.len(), 3);

    // A tree's top-level any is bracketed, so the date bounds all of it.
    let mut q = tree_query(QueryNode::Any(vec![
        QueryNode::term(TermField::Title, "sepsis"),
        QueryNode::phrase(TermField::Abstract, "septic shock"),
    ]));
    q.since = since;
    block_on(dc.search(&q)).unwrap();
    assert_eq!(
        query(&http, 1)[0],
        pair(
            "query",
            r#"(titles.title:(sepsis) OR descriptions.description:"septic shock") AND created:[2026-11-01 TO *]"#
        )
    );
}
