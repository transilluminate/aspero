//! Europe PMC provider against responses recorded by
//! `scripts/record-europepmc.sh`.

use std::sync::Arc;

use aspero_core::testing::{ReplayClient, block_on, fixture, response};
use aspero_core::{
    Cursor, Error, FieldSet, FilterValue, HttpResponse, Provider, QueryNode, SearchQuery, Sort,
    TermField, UpdateKind, WorkId, WorkType, YearRange,
};
use aspero_europepmc::{EuropePmc, EuropePmcConfig};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn recorded(name: &str) -> HttpResponse {
    fixture(FIXTURES, name)
}

fn provider(responses: impl IntoIterator<Item = HttpResponse>) -> (EuropePmc, Arc<ReplayClient>) {
    let http = Arc::new(ReplayClient::new(responses));
    (
        EuropePmc::new(EuropePmcConfig::default(), http.clone()),
        http,
    )
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
fn batch_lookup_of_pmids_pmcids_and_dois() {
    let (epmc, http) = provider([recorded("lookup_batch")]);
    let requested = [
        id("pmid:26017442"),
        id("PMC6716493"),
        id("10.1016/S0140-6736(97)11096-0"),
        id("pmid:99999999"),
    ];
    let resp = block_on(epmc.lookup(&requested, FieldSet::DEFAULT)).unwrap();

    assert!(query(&http, 0).contains(&pair(
        "query",
        r#"(EXT_ID:26017442 AND SRC:MED) OR PMCID:PMC6716493 OR DOI:"10.1016/s0140-6736(97)11096-0" OR (EXT_ID:99999999 AND SRC:MED)"#
    )));
    assert!(query(&http, 0).contains(&pair("pageSize", "8")));
    let [lecun, pmc, wakefield, missing] = &resp.value[..] else {
        panic!()
    };

    let r = lecun.record.as_ref().unwrap();
    assert!(r.ids.contains(&id("10.1038/nature14539")));
    assert_eq!(
        r.work.title.as_deref(),
        Some("Deep learning"),
        "trailing period dropped"
    );
    assert_eq!(r.work.container_title.as_deref(), Some("Nature"));
    assert_eq!(r.work.authors[0].family.as_deref(), Some("LeCun"));
    assert!(
        r.work.abstract_text.is_some(),
        "abstracts where OpenAlex has none"
    );

    let r = pmc.record.as_ref().unwrap();
    assert!(
        r.ids.contains(&id("pmid:31507447")) && r.ids.contains(&id("10.3389/fphys.2019.01083"))
    );
    assert_eq!(
        r.work.authors[0].orcid.as_deref(),
        Some("0000-0001-5410-9085")
    );
    assert_eq!(r.work.work_type, Some(WorkType::JournalArticle));
    let oa = r.work.open_access.as_ref().unwrap();
    assert!(oa.is_oa);
    assert_eq!(
        oa.pdf_url.as_deref(),
        Some("https://europepmc.org/articles/PMC6716493?pdf=render"),
        "Europe PMC's own copy first"
    );
    assert_eq!(oa.license.as_deref(), Some("cc-by"));

    let r = wakefield.record.as_ref().unwrap();
    let retraction = r.work.retraction().expect("Retracted Publication flag");
    assert_eq!(retraction.kind, UpdateKind::Retraction);
    assert_eq!(retraction.source.as_deref(), Some("europepmc"));

    assert!(missing.record.is_none());
}

#[test]
fn bookshelf_chapters_are_looked_up_by_nbk_id() {
    let (epmc, http) = provider([recorded("lookup_nbk")]);
    let resp =
        block_on(epmc.lookup(&[id("NBK526056"), id("NBK999999999")], FieldSet::DEFAULT)).unwrap();

    assert!(query(&http, 0).contains(&pair("query", "BOOK_ID:NBK526056 OR BOOK_ID:NBK999999999")));
    let [chapter, missing] = &resp.value[..] else {
        panic!()
    };
    let r = chapter.record.as_ref().unwrap();
    assert!(r.ids.contains(&id("pmid:30252312")));
    // Not "Pacemaker syndrome": the check this lookup exists for.
    assert_eq!(
        r.work.title.as_deref(),
        Some("Anatomy, Shoulder and Upper Limb, Nerves")
    );
    assert_eq!(r.work.work_type, Some(WorkType::Chapter));
    assert_eq!(r.work.container_title.as_deref(), Some("StatPearls"));
    assert_eq!(
        r.work.url.as_deref(),
        Some("https://www.ncbi.nlm.nih.gov/books/NBK526056/")
    );
    assert!(missing.record.is_none());
}

#[test]
fn search_pages_by_cursor_mark() {
    let (epmc, http) = provider([recorded("search")]);
    let mut q = SearchQuery::new("protein structure prediction alphafold");
    q.page.limit = 3;
    let resp = block_on(epmc.search(&q)).unwrap();

    let params = query(&http, 0);
    assert!(params.contains(&pair("query", "(protein structure prediction alphafold)")));
    assert!(params.contains(&pair("cursorMark", "*")));
    assert!(params.contains(&pair("pageSize", "3")));
    let page = resp.value;
    assert_eq!(page.hits.len(), 3);
    assert!(matches!(page.next, Some(Cursor(ref c)) if c != "*"));
}

#[test]
fn search_filters_years_and_open_access() {
    let (epmc, http) = provider([recorded("search_filtered")]);
    let mut q = SearchQuery::new("protein structure prediction");
    q.years = Some(YearRange {
        from: Some(2021),
        to: Some(2021),
    });
    q.open_access = Some(true);
    q.page.limit = 3;
    let resp = block_on(epmc.search(&q)).unwrap();

    assert!(query(&http, 0).contains(&pair(
        "query",
        "(protein structure prediction) AND PUB_YEAR:[2021 TO 2021] AND OPEN_ACCESS:y"
    )));
    assert!(
        resp.value
            .hits
            .iter()
            .all(|h| h.record.work.issued.unwrap().year() == 2021)
    );
}

fn empty_page() -> HttpResponse {
    response(200, &[], r#"{"hitCount": 0, "resultList": {"result": []}}"#)
}

#[test]
fn fields_alone_are_field_clauses() {
    let (epmc, http) = provider([recorded("search_fields")]);
    let mut q = SearchQuery::new("");
    q.author = Some("Jumper J".into());
    q.title = Some("protein structure prediction".into());
    q.container = Some("Nature".into());
    q.page.limit = 3;
    let page = block_on(epmc.search(&q)).unwrap().value;
    assert_eq!(
        query(&http, 0),
        [
            pair(
                "query",
                r#"AUTH:"Jumper J" AND TITLE:(protein structure prediction) AND JOURNAL:"Nature""#
            ),
            pair("resultType", "core"),
            pair("format", "json"),
            pair("pageSize", "3"),
            pair("cursorMark", "*"),
        ]
    );
    assert!(
        page.hits
            .iter()
            .all(|h| h.record.work.container_title.as_deref() == Some("Nature")),
    );
}

#[test]
fn fields_are_escaped_field_clauses() {
    let (epmc, http) = provider([empty_page()]);
    let mut q = SearchQuery::new("");
    q.author = Some("Jumper J".into());
    q.title = Some("AlphaFold: \"protein\" structure".into());
    q.container = Some("Nature".into());
    block_on(epmc.search(&q)).unwrap();
    let params = query(&http, 0);
    assert_eq!(
        params[0],
        pair(
            "query",
            r#"AUTH:"Jumper J" AND TITLE:(AlphaFold\: \"protein\" structure) AND JOURNAL:"Nature""#
        )
    );
    // Relevance is the default order: no sort sent.
    assert!(!params.iter().any(|(k, _)| k == "sort"), "{params:?}");
}

/// The parameters of a sorted search for "alphafold", three a page.
fn sorted_by(sort: &str) -> [(String, String); 6] {
    [
        pair("query", "(alphafold)"),
        pair("resultType", "core"),
        pair("format", "json"),
        pair("pageSize", "3"),
        pair("cursorMark", "*"),
        pair("sort", sort),
    ]
}

#[test]
fn newest_first_sorts_by_publication_date() {
    let (epmc, http) = provider([recorded("search_newest")]);
    let mut q = SearchQuery::new("alphafold");
    q.sort = Sort::Newest;
    q.page.limit = 3;
    let page = block_on(epmc.search(&q)).unwrap().value;
    assert_eq!(query(&http, 0), sorted_by("P_PDATE_D desc"));
    // Sorted results still page by cursor.
    assert!(matches!(page.next, Some(Cursor(ref c)) if c != "*"));
}

#[test]
fn most_cited_first_sorts_by_citations() {
    let (epmc, http) = provider([recorded("search_most_cited")]);
    let mut q = SearchQuery::new("alphafold");
    q.sort = Sort::MostCited;
    q.page.limit = 3;
    let page = block_on(epmc.search(&q)).unwrap().value;
    assert_eq!(query(&http, 0), sorted_by("CITED desc"));
    let counts: Vec<u64> = page
        .hits
        .iter()
        .map(|h| h.record.work.cited_by_count.unwrap())
        .collect();
    assert!(counts.is_sorted_by(|a, b| a >= b), "{counts:?}");
}

#[test]
fn unknown_extras_and_citation_mode_are_refused() {
    let (epmc, http) = provider([]);
    let mut q = SearchQuery::new("x");
    q.extra.insert("sort".into(), "CITED desc".into());
    assert!(matches!(
        block_on(epmc.search(&q)),
        Err(Error::BadRequest(_))
    ));
    assert!(matches!(
        block_on(epmc.search(&SearchQuery::citation("x"))),
        Err(Error::Unsupported(_))
    ));
    assert!(matches!(
        block_on(epmc.lookup(&[id("arXiv:1706.03762")], FieldSet::DEFAULT)),
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
fn trees_are_lucene_queries_across_fields() {
    let (epmc, http) = provider([recorded("search_tree")]);
    let tree = QueryNode::All(vec![
        QueryNode::Any(vec![
            QueryNode::phrase(TermField::Title, "protein structure prediction"),
            QueryNode::term(TermField::Abstract, "alphafold"),
        ]),
        QueryNode::filter(FilterValue::Language("en".into())),
        QueryNode::filter(FilterValue::HasAbstract),
        QueryNode::filter(FilterValue::NotRetracted),
        QueryNode::None(vec![QueryNode::term(TermField::Title, "review")]),
        QueryNode::filter(FilterValue::Years(YearRange {
            from: Some(2021),
            to: Some(2023),
        })),
    ]);
    let page = block_on(epmc.search(&tree_query(tree))).unwrap().value;
    assert_eq!(
        query(&http, 0),
        [
            pair(
                "query",
                r#"(TITLE:"protein structure prediction" OR ABSTRACT:(alphafold)) AND LANG:eng AND HAS_ABSTRACT:Y AND (NOT PUB_TYPE:"Retracted Publication") AND (NOT TITLE:(review)) AND PUB_YEAR:[2021 TO 2023]"#
            ),
            pair("resultType", "core"),
            pair("format", "json"),
            pair("pageSize", "3"),
            pair("cursorMark", "*"),
        ]
    );
    for hit in &page.hits {
        let w = &hit.record.work;
        assert!(w.abstract_text.is_some());
        assert!(
            !w.title
                .as_deref()
                .unwrap()
                .to_lowercase()
                .contains("review")
        );
        assert!(w.updates.iter().all(|u| u.kind != UpdateKind::Retraction));
    }
}

#[test]
fn orcids_are_author_ids_and_languages_medline_codes() {
    let (epmc, http) = provider([recorded("search_tree_orcid")]);
    let tree = QueryNode::All(vec![
        QueryNode::term(TermField::AuthorId, "https://orcid.org/0000-0001-6187-6610"),
        QueryNode::None(vec![QueryNode::filter(FilterValue::Language("de".into()))]),
    ]);
    let mut q = tree_query(tree);
    q.sort = Sort::Newest;
    let page = block_on(epmc.search(&q)).unwrap().value;
    assert_eq!(
        query(&http, 0)[0],
        pair(
            "query",
            r#"AUTHORID:"0000-0001-6187-6610" AND (NOT LANG:ger)"#
        )
    );
    assert_eq!(page.total, Some(2));
    let (epmc, _) = provider([]);
    assert_eq!(
        epmc.express(&QueryNode::term(TermField::AuthorId, "A5023888391"))
            .unwrap_err(),
        "Europe PMC finds authors by ORCID, not OpenAlex author ids"
    );
    assert_eq!(
        epmc.express(&QueryNode::relation(aspero_core::Relation::Cites, "W1"))
            .unwrap_err(),
        "Europe PMC can't find the works citing a work"
    );
    assert_eq!(
        epmc.express(&QueryNode::filter(FilterValue::WorkType(WorkType::Book)))
            .unwrap_err(),
        "Europe PMC can't filter by work type"
    );
}

#[test]
fn since_is_the_first_index_date_anded_with_text_or_a_tree() {
    let (epmc, http) = provider([recorded("search_since"), empty_page(), empty_page()]);
    let since = Some(jiff::civil::date(2026, 11, 1));
    let mut q = SearchQuery::new("sepsis");
    q.since = since;
    q.page.limit = 3;
    let page = block_on(epmc.search(&q)).unwrap().value;
    assert_eq!(
        query(&http, 0),
        [
            pair("query", "(sepsis) AND FIRST_IDATE:[2026-11-01 TO *]"),
            pair("resultType", "core"),
            pair("format", "json"),
            pair("pageSize", "3"),
            pair("cursorMark", "*"),
        ]
    );
    assert_eq!(page.total, Some(1294));
    assert_eq!(page.hits.len(), 3);

    // A tree's top-level any is bracketed, so the date bounds all of it.
    let any = QueryNode::Any(vec![
        QueryNode::term(TermField::Title, "sepsis"),
        QueryNode::term(TermField::Abstract, "septic shock"),
    ]);
    let mut q = tree_query(any.clone());
    q.since = since;
    block_on(epmc.search(&q)).unwrap();
    assert_eq!(
        query(&http, 1)[0],
        pair(
            "query",
            "(TITLE:(sepsis) OR ABSTRACT:(septic shock)) AND FIRST_IDATE:[2026-11-01 TO *]"
        )
    );
    // Without since, the tree is sent as before.
    block_on(epmc.search(&tree_query(any))).unwrap();
    assert_eq!(
        query(&http, 2)[0],
        pair("query", "TITLE:(sepsis) OR ABSTRACT:(septic shock)")
    );
}
