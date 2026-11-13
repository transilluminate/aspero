//! Europe PMC provider against responses recorded by
//! `scripts/record-europepmc.sh`.

use std::sync::Arc;

use aspero_core::testing::{ReplayClient, block_on, fixture, response};
use aspero_core::{
    Cursor, Error, FieldSet, HttpResponse, Provider, SearchQuery, Sort, UpdateKind, WorkId,
    WorkType, YearRange,
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

#[test]
fn sorts_by_publication_date_or_citations() {
    let (epmc, http) = provider([empty_page(), empty_page()]);
    let mut q = SearchQuery::new("alphafold");
    q.sort = Sort::Newest;
    block_on(epmc.search(&q)).unwrap();
    q.sort = Sort::MostCited;
    block_on(epmc.search(&q)).unwrap();
    assert!(query(&http, 0).contains(&pair("sort", "P_PDATE_D desc")));
    assert!(query(&http, 1).contains(&pair("sort", "CITED desc")));
    assert!(query(&http, 1).contains(&pair("cursorMark", "*")));
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
