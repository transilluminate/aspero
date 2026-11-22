//! PMC open data against responses recorded by `scripts/record-pmc.sh`.

use std::sync::Arc;

use aspero_core::testing::{ReplayClient, block_on, fixture, response};
use aspero_core::{FieldSet, FreeCopyKind, HttpResponse, Provider, WorkId};
use aspero_pmc::{Pmc, PmcConfig};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn provider(responses: impl IntoIterator<Item = HttpResponse>) -> (Pmc, Arc<ReplayClient>) {
    let http = Arc::new(ReplayClient::new(responses));
    (Pmc::new(PmcConfig::default(), http.clone()), http)
}

fn id(s: &str) -> WorkId {
    s.parse().unwrap()
}

fn urls(http: &ReplayClient) -> Vec<String> {
    http.requests().into_iter().map(|r| r.url).collect()
}

#[test]
fn an_open_access_article_gives_its_pdf() {
    let (pmc, http) = provider([fixture(FIXTURES, "list_oa"), fixture(FIXTURES, "meta_oa")]);
    let resp = block_on(pmc.lookup(&[id("PMC8114386")], FieldSet::ALL)).unwrap();
    assert_eq!(
        urls(&http),
        [
            "https://pmc-oa-opendata.s3.amazonaws.com/?list-type=2&prefix=PMC8114386.&delimiter=/",
            "https://pmc-oa-opendata.s3.amazonaws.com/PMC8114386.1/PMC8114386.1.json",
        ],
        "the listing, then the latest version's metadata"
    );
    let record = resp.value[0].record.as_ref().unwrap();
    assert_eq!(record.source.as_str(), "pmc");
    assert_eq!(
        record.ids,
        [
            id("PMC8114386"),
            id("10.1080/21642850.2018.1462707"),
            id("pmid:34040824")
        ]
    );
    let oa = record.work.open_access.as_ref().unwrap();
    assert!(oa.is_oa);
    assert_eq!(
        oa.pdf_url.as_deref(),
        Some("https://pmc-oa-opendata.s3.amazonaws.com/PMC8114386.1/PMC8114386.1.pdf")
    );
    assert_eq!(oa.free_copy(), FreeCopyKind::Pdf);
    assert_eq!(oa.license.as_deref(), Some("cc-by"));
    assert_eq!(oa.version.as_deref(), Some("publishedVersion"));
    assert_eq!(oa.source.as_deref(), Some("pmc"));
    assert_eq!(
        oa.url.as_deref(),
        Some("https://pmc.ncbi.nlm.nih.gov/articles/PMC8114386/")
    );
}

#[test]
fn an_author_manuscript_is_free_to_read_without_a_pdf() {
    // doi:10.1001/jama.2010.158, an example in #204: not in PMC's
    // open-access subset, licensed for text mining only.
    let (pmc, _) = provider([
        fixture(FIXTURES, "list_manuscript"),
        fixture(FIXTURES, "meta_manuscript"),
    ]);
    let resp = block_on(pmc.lookup(&[id("PMC2918907")], FieldSet::ALL)).unwrap();
    let oa = resp.value[0]
        .record
        .as_ref()
        .unwrap()
        .work
        .open_access
        .clone()
        .unwrap();
    assert!(oa.is_oa);
    assert_eq!(oa.pdf_url, None);
    assert_eq!(oa.license, None, "TDM is no reuse licence");
    assert_eq!(oa.version.as_deref(), Some("acceptedVersion"));
    assert_eq!(oa.free_copy(), FreeCopyKind::BrowserOnly);
}

#[test]
fn an_article_outside_the_dataset_is_not_found_in_one_request() {
    let (pmc, http) = provider([fixture(FIXTURES, "list_missing")]);
    let resp = block_on(pmc.lookup(&[id("PMC5008955")], FieldSet::ALL)).unwrap();
    assert!(resp.value[0].record.is_none());
    assert_eq!(http.requests().len(), 1);
}

#[test]
fn a_listed_version_without_metadata_is_not_found() {
    let (pmc, _) = provider([
        fixture(FIXTURES, "list_oa"),
        fixture(FIXTURES, "meta_missing"),
    ]);
    let resp = block_on(pmc.lookup(&[id("PMC8114386")], FieldSet::ALL)).unwrap();
    assert!(resp.value[0].record.is_none());
}

#[test]
fn the_latest_of_several_versions_is_read() {
    // Hand-made: a manuscript (.1) and the published article (.2).
    let listing = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ListBucketResult>\
        <CommonPrefixes><Prefix>PMC77.1/</Prefix></CommonPrefixes>\
        <CommonPrefixes><Prefix>PMC77.2/</Prefix></CommonPrefixes></ListBucketResult>";
    let meta = r#"{"pmcid": "PMC77", "version": 2, "is_pmc_openaccess": true,
        "is_manuscript": false, "is_retracted": true, "license_code": "CC BY-NC",
        "pdf_url": "s3://pmc-oa-opendata/PMC77.2/PMC77.2.pdf?md5=00"}"#;
    let (pmc, http) = provider([
        response(200, &[("content-type", "application/xml")], listing),
        response(200, &[], meta),
    ]);
    let resp = block_on(pmc.lookup(&[id("PMC77")], FieldSet::ALL)).unwrap();
    assert!(urls(&http)[1].ends_with("/PMC77.2/PMC77.2.json"));
    let oa = resp.value[0]
        .record
        .as_ref()
        .unwrap()
        .work
        .open_access
        .clone();
    let oa = oa.unwrap();
    assert_eq!(
        oa.pdf_url.as_deref(),
        Some("https://pmc-oa-opendata.s3.amazonaws.com/PMC77.2/PMC77.2.pdf"),
        "a retracted article keeps its copy: notices tell the retraction"
    );
    assert_eq!(oa.license.as_deref(), Some("cc-by-nc"));
}

#[test]
fn a_pdf_outside_the_open_access_subset_is_not_offered() {
    let listing = "<ListBucketResult><CommonPrefixes><Prefix>PMC78.1/</Prefix>\
        </CommonPrefixes></ListBucketResult>";
    let meta = r#"{"pmcid": "PMC78", "version": 1, "is_pmc_openaccess": false,
        "license_code": "TDM", "pdf_url": "s3://pmc-oa-opendata/PMC78.1/PMC78.1.pdf"}"#;
    let (pmc, _) = provider([response(200, &[], listing), response(200, &[], meta)]);
    let resp = block_on(pmc.lookup(&[id("PMC78")], FieldSet::ALL)).unwrap();
    let oa = resp.value[0]
        .record
        .as_ref()
        .unwrap()
        .work
        .open_access
        .clone();
    assert_eq!(oa.unwrap().pdf_url, None);
}

#[test]
fn server_errors_fail_the_lookup() {
    let (pmc, _) = provider([response(503, &[], "Slow Down")]);
    assert!(block_on(pmc.lookup(&[id("PMC1")], FieldSet::ALL)).is_err());
}
