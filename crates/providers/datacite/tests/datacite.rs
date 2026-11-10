//! DataCite provider against responses recorded by `scripts/record-datacite.sh`.

use std::sync::Arc;

use aspero_core::testing::{ReplayClient, block_on, fixture, response};
use aspero_core::{
    Cursor, Error, FieldSet, HttpResponse, Provider, SearchQuery, WorkId, WorkType, YearRange,
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
