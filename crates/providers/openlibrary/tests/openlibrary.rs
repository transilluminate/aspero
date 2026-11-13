//! Open Library provider against responses recorded by
//! `scripts/record-openlibrary.sh`.

use std::sync::Arc;

use aspero_core::testing::{ReplayClient, block_on, fixture, response};
use aspero_core::{
    Cursor, Error, FieldSet, HttpResponse, Provider, SearchQuery, Sort, WorkId, WorkType, YearRange,
};
use aspero_openlibrary::{OpenLibrary, OpenLibraryConfig};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn recorded(name: &str) -> HttpResponse {
    fixture(FIXTURES, name)
}

fn provider(responses: impl IntoIterator<Item = HttpResponse>) -> (OpenLibrary, Arc<ReplayClient>) {
    let http = Arc::new(ReplayClient::new(responses));
    (
        OpenLibrary::new(OpenLibraryConfig::default(), http.clone()),
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
fn isbn_batch_lookup_maps_editions() {
    let (ol, http) = provider([recorded("books_isbn")]);
    // The ISBN-10 form is the same book, so the request uses canonical ISBN-13.
    let requested = [id("isbn:0-262-03561-8"), id("9780000000002")];
    let resp = block_on(ol.lookup(&requested, FieldSet::DEFAULT)).unwrap();

    assert!(query(&http, 0).contains(&pair("bibkeys", "ISBN:9780262035613,ISBN:9780000000002")));
    let record = resp.value[0].record.as_ref().unwrap();
    assert_eq!(record.ids[0], id("9780262035613"));
    let w = &record.work;
    assert_eq!(w.title.as_deref(), Some("Deep Learning"));
    assert_eq!(
        w.authors[0].display_name().as_deref(),
        Some("Ian Goodfellow")
    );
    assert_eq!(
        w.issued.unwrap().year(),
        2017,
        "this edition's publish date"
    );
    assert_eq!(w.publisher.as_deref(), Some("MIT Press"));
    assert_eq!(w.work_type, Some(WorkType::Book));
}

#[test]
fn unknown_isbns_are_absent_records() {
    let (ol, _) = provider([response(200, &[], "{}")]);
    let resp = block_on(ol.lookup(&[id("9780262035613")], FieldSet::DEFAULT)).unwrap();
    assert!(resp.value[0].record.is_none());
}

#[test]
fn search_returns_works_with_their_isbns() {
    let (ol, http) = provider([recorded("search")]);
    let mut q = SearchQuery::new("deep learning goodfellow");
    q.page.limit = 3;
    let resp = block_on(ol.search(&q)).unwrap();

    let params = query(&http, 0);
    assert!(params.contains(&pair("q", "(deep learning goodfellow)")));
    assert!(params.contains(&pair("limit", "3")));
    assert!(params.contains(&pair("page", "1")));

    let top = &resp.value.hits[0].record;
    assert_eq!(top.work.title.as_deref(), Some("Deep Learning"));
    assert_eq!(top.work.issued.unwrap().year(), 2016, "first publication");
    assert_eq!(top.work.other_years, [2017], "later editions");
    assert!(top.ids.contains(&id("9780262035613")));
    assert_eq!(
        top.work.url.as_deref(),
        Some("https://openlibrary.org/works/OL17801809W")
    );
}

#[test]
fn year_ranges_use_query_syntax_and_paging_continues() {
    let (ol, http) = provider([recorded("search_filtered")]);
    let mut q = SearchQuery::new("deep learning");
    q.years = Some(YearRange {
        from: Some(2016),
        to: Some(2016),
    });
    q.page.limit = 3;
    let resp = block_on(ol.search(&q)).unwrap();

    assert!(query(&http, 0).contains(&pair(
        "q",
        "(deep learning) AND first_publish_year:[2016 TO 2016]"
    )));
    let page = resp.value;
    assert!(
        page.hits
            .iter()
            .all(|h| h.record.work.issued.unwrap().year() == 2016)
    );
    assert_eq!(page.next, Some(Cursor("2".into())));
}

#[test]
fn paging_ends_at_the_last_numbered_page() {
    let body = r#"{"numFound": 99999999999, "docs": [{"key": "/works/OL1W", "title": "A"}]}"#;
    let (ol, _) = provider([response(200, &[], body)]);
    let mut q = SearchQuery::new("x");
    q.page.cursor = Some(Cursor(u32::MAX.to_string()));
    let page = block_on(ol.search(&q)).unwrap().value;
    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.next, None);
}

#[test]
fn authors_and_titles_are_parameters_and_newest_is_sort_new() {
    let (ol, http) = provider([response(200, &[], r#"{"numFound": 0, "docs": []}"#)]);
    let mut q = SearchQuery::new("");
    q.author = Some("Goodfellow".into());
    // Open Library escapes these itself.
    q.title = Some("Deep learning: (adaptive)".into());
    q.sort = Sort::Newest;
    block_on(ol.search(&q)).unwrap();
    let params = query(&http, 0);
    assert_eq!(
        params[..4],
        [
            pair("q", ""),
            pair("author", "Goodfellow"),
            pair("title", "Deep learning: (adaptive)"),
            pair("sort", "new"),
        ]
    );

    q.sort = Sort::MostCited;
    assert!(matches!(
        block_on(ol.search(&q)),
        Err(Error::Unsupported(_))
    ));
    assert_eq!(http.requests().len(), 1);
}

#[test]
fn user_text_is_escaped_and_extras_rejected() {
    let (ol, http) = provider([recorded("search")]);
    block_on(ol.search(&SearchQuery::new("C++: a guide"))).unwrap();
    assert!(query(&http, 0).contains(&pair("q", r"(C\+\+\: a guide)")));

    let mut bad = SearchQuery::new("x");
    bad.extra.insert("sort".into(), "new".into());
    assert!(matches!(
        block_on(ol.search(&bad)),
        Err(Error::BadRequest(_))
    ));
    assert!(matches!(
        block_on(ol.search(&SearchQuery::citation("x"))),
        Err(Error::Unsupported(_))
    ));
}
