//! doi.org provider against responses recorded by `scripts/record-doi.sh`.

use std::sync::Arc;

use aspero_core::testing::{FIXTURE_TIME, ReplayClient, block_on, fixture, response};
use aspero_core::{Error, FieldSet, HttpResponse, Provider, WorkId, WorkType};
use aspero_doi::{DoiConfig, DoiOrg};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn provider(responses: impl IntoIterator<Item = HttpResponse>) -> (DoiOrg, Arc<ReplayClient>) {
    let http = Arc::new(ReplayClient::new(responses));
    (DoiOrg::new(DoiConfig::default(), http.clone()), http)
}

fn id(s: &str) -> WorkId {
    s.parse().unwrap()
}

#[test]
fn crossref_registered_doi_maps_via_csl() {
    let (doi, http) = provider([fixture(FIXTURES, "csl_crossref")]);
    let resp = block_on(doi.lookup(&[id("10.1038/nature14539")], FieldSet::DEFAULT)).unwrap();

    let req = &http.requests()[0];
    assert_eq!(req.url, "https://doi.org/10.1038/nature14539");
    assert!(req.headers.contains(&(
        "accept".into(),
        "application/vnd.citationstyles.csl+json".into()
    )));

    let record = resp.value[0].record.as_ref().unwrap();
    assert_eq!(record.ids, [id("10.1038/nature14539")]);
    assert_eq!(record.source.as_str(), "doi");
    assert_eq!(record.fetched_at, FIXTURE_TIME);
    let w = &record.work;
    assert_eq!(w.title.as_deref(), Some("Deep learning"));
    assert_eq!(
        w.work_type,
        Some(WorkType::JournalArticle),
        "Crossref's type names"
    );
    assert_eq!(w.container_title.as_deref(), Some("Nature"));
    assert_eq!(w.issued.unwrap().to_string(), "2015-05-27");
    assert_eq!(w.authors[0].family.as_deref(), Some("LeCun"));
}

#[test]
fn arxiv_ids_resolve_through_datacite_with_abstract() {
    let (doi, http) = provider([fixture(FIXTURES, "csl_datacite")]);
    let resp = block_on(doi.lookup(&[id("arXiv:1706.03762v5")], FieldSet::DEFAULT)).unwrap();

    assert_eq!(
        http.requests()[0].url,
        "https://doi.org/10.48550/arxiv.1706.03762"
    );
    let record = resp.value[0].record.as_ref().unwrap();
    assert_eq!(
        record.ids,
        [id("10.48550/arXiv.1706.03762"), id("arXiv:1706.03762")]
    );
    let w = &record.work;
    assert_eq!(w.work_type, Some(WorkType::Preprint));
    assert_eq!(w.issued.unwrap().year(), 2017);
    assert!(
        w.abstract_text
            .as_deref()
            .unwrap()
            .starts_with("The dominant sequence transduction")
    );
}

#[test]
fn a_404_is_an_absent_record() {
    let (doi, _) = provider([fixture(FIXTURES, "csl_not_found")]);
    let resp =
        block_on(doi.lookup(&[id("10.5555/aspero-does-not-exist")], FieldSet::DEFAULT)).unwrap();
    assert!(resp.value[0].record.is_none());
}

#[test]
fn several_ids_are_fetched_one_by_one_in_order() {
    let (doi, http) = provider([
        fixture(FIXTURES, "csl_not_found"),
        fixture(FIXTURES, "csl_crossref"),
    ]);
    let resp = block_on(doi.lookup(
        &[
            id("10.5555/aspero-does-not-exist"),
            id("10.1038/nature14539"),
        ],
        FieldSet::DEFAULT,
    ))
    .unwrap();
    assert_eq!(http.requests().len(), 2);
    assert!(resp.value[0].record.is_none() && resp.value[1].record.is_some());
}

#[test]
fn failures_map_to_errors() {
    let (doi, _) = provider([
        response(406, &[], ""),
        response(429, &[("retry-after", "4")], ""),
        response(503, &[], ""),
    ]);
    let ask = |d: &DoiOrg| block_on(d.lookup(&[id("10.1000/x")], FieldSet::DEFAULT));
    assert!(matches!(ask(&doi), Err(Error::Unsupported(_))));
    assert!(matches!(
        ask(&doi),
        Err(Error::RateLimited {
            retry_after: Some(_)
        })
    ));
    assert!(matches!(ask(&doi), Err(Error::Server { status: 503, .. })));
}

#[test]
fn other_identifier_kinds_are_refused() {
    let (doi, http) = provider([]);
    assert!(matches!(
        block_on(doi.lookup(&[id("pmid:26017442")], FieldSet::DEFAULT)),
        Err(Error::Unsupported(_))
    ));
    assert!(http.requests().is_empty());
}

#[test]
fn crossref_notices_arrive_through_csl() {
    let (doi, _) = provider([fixture(FIXTURES, "csl_retracted")]);
    let resp =
        block_on(doi.lookup(&[id("10.1016/S0140-6736(97)11096-0")], FieldSet::DEFAULT)).unwrap();
    let r = resp.value[0]
        .record
        .as_ref()
        .unwrap()
        .work
        .retraction()
        .cloned()
        .expect("retracted");
    assert_eq!(r.date.unwrap().year(), 2010);
}
