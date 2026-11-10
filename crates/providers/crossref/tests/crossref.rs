//! Crossref provider against responses recorded by `scripts/record-crossref.sh`.

use std::sync::Arc;
use std::time::Duration;

use aspero_core::testing::{FIXTURE_TIME, ReplayClient, block_on, fixture, response};
use aspero_core::{
    Error, FieldSet, HttpResponse, Provider, SearchQuery, WorkId, WorkType, YearRange,
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
fn only_dois_are_supported() {
    let (cr, http) = provider([]);
    let err = block_on(cr.lookup(&[id("arXiv:1706.03762")], FieldSet::DEFAULT)).unwrap_err();
    assert!(matches!(err, Error::Unsupported(_)));
    assert!(http.requests().is_empty());
}
