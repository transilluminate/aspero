//! A work's fetchable free copy from PMC's open data (#204): asked only
//! when a caller wants it, never in lookups, full lookups or verifies.
#![cfg(feature = "pmc")]

mod common;

use std::sync::Arc;

use aspero_core::testing::{ReplayClient, fixture};
use aspero_core::{
    Claim, FieldSet, FreeCopyKind, HttpResponse, IdKind, OpenAccess, ResiliencePolicy,
    SourceRecord, WorkId,
};
use aspero_engine::{Engine, Extras, InputLookup, Outcome, Served, SqliteCache};
use aspero_pmc::{Pmc, PmcConfig};
use common::*;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../providers/pmc/tests/fixtures"
);
const RENDER: &str = "https://europepmc.org/articles/PMC8114386?pdf=render";
const S3_PDF: &str = "https://pmc-oa-opendata.s3.amazonaws.com/PMC8114386.1/PMC8114386.1.pdf";

/// Europe PMC's record of a PMC article: free, its PDF link the render
/// endpoint that turns scripts away.
fn on_pmc(pmcid: &str, doi: &str) -> SourceRecord {
    let mut r = work(
        "europepmc",
        &[pmcid, doi],
        "An article",
        &["Ann Author"],
        2018,
    );
    let mut oa = OpenAccess::default();
    oa.is_oa = true;
    oa.status = Some("green".into());
    oa.license = Some("cc by".into());
    oa.pdf_url = Some(RENDER.into());
    oa.source = Some("europepmc".into());
    r.work.open_access = Some(oa);
    r
}

fn setup(
    records: Vec<SourceRecord>,
    responses: impl IntoIterator<Item = HttpResponse>,
) -> (Engine, Arc<ReplayClient>) {
    let mut europepmc = Scripted::new("europepmc").with(records);
    europepmc.caps.lookup_kinds = vec![IdKind::Pmcid, IdKind::Doi];
    let http = Arc::new(ReplayClient::new(responses));
    let pmc = Pmc::new(PmcConfig::default(), http.clone());
    let engine = Engine::builder()
        .provider(Arc::new(europepmc), ResiliencePolicy::default(), 0)
        .free_copies(Arc::new(pmc), ResiliencePolicy::default())
        .cache(Arc::new(SqliteCache::open_in_memory().unwrap()))
        .build();
    (engine, http)
}

fn oa_fixtures() -> [HttpResponse; 2] {
    [fixture(FIXTURES, "list_oa"), fixture(FIXTURES, "meta_oa")]
}

fn looked_oa(item: &InputLookup) -> OpenAccess {
    let InputLookup::Looked(looked) = item else {
        panic!("not looked up")
    };
    let record = looked.result.record.as_ref().unwrap();
    record.work.open_access.clone().unwrap()
}

fn id(s: &str) -> WorkId {
    s.parse().unwrap()
}

#[tokio::test]
async fn lookups_full_or_not_and_verifies_never_ask_pmc() {
    let record = on_pmc("PMC8114386", "10.1080/21642850.2018.1462707");
    let (engine, http) = setup(vec![record], []);
    let ids = vec!["PMC8114386".to_owned()];

    let plain = engine.lookup(&[id("PMC8114386")], FieldSet::ALL).await;
    assert!(plain.results[0].record.is_some());
    let full = engine.lookup_inputs(&ids, Extras::full()).await;
    let oa = looked_oa(&full.items[0]);
    assert_eq!(oa.pdf_url.as_deref(), Some(RENDER), "left as it was");
    assert_eq!(oa.free_copy(), FreeCopyKind::BrowserOnly);
    let _ = engine
        .verify(&Claim::from_text("An article. PMCID: PMC8114386"), 5)
        .await;

    assert!(http.requests().is_empty(), "{:?}", http.requests());
    let asked: Vec<String> = engine.providers().map(|p| p.id().to_string()).collect();
    assert_eq!(asked, ["europepmc"], "not in the lookup chain");
    assert_eq!(engine.free_copy_source().unwrap().id().as_str(), "pmc");
}

#[tokio::test]
async fn a_lookup_asking_for_free_copies_gets_the_fetchable_pdf() {
    let record = on_pmc("PMC8114386", "10.1080/21642850.2018.1462707");
    let (engine, http) = setup(vec![record], oa_fixtures());
    let mut extras = Extras::full();
    extras.free_copies = true;

    let report = engine
        .lookup_inputs(&["PMC8114386".to_owned()], extras)
        .await;
    let oa = looked_oa(&report.items[0]);
    assert_eq!(oa.pdf_url.as_deref(), Some(S3_PDF));
    assert_eq!(oa.free_copy(), FreeCopyKind::Pdf);
    assert_eq!(oa.source.as_deref(), Some("pmc"));
    assert_eq!(oa.status.as_deref(), Some("green"), "status kept");
    assert_eq!(oa.license.as_deref(), Some("cc by"), "licence kept");
    assert_eq!(http.requests().len(), 2, "the listing and the metadata");
}

#[tokio::test]
async fn free_copy_asks_once_then_answers_from_the_cache() {
    let record = on_pmc("PMC8114386", "10.1080/21642850.2018.1462707");
    let (engine, http) = setup(vec![], oa_fixtures());

    let copy = engine.free_copy(&record).await;
    assert_eq!(copy.kind(), FreeCopyKind::Pdf);
    let attempt = copy.attempt.unwrap();
    assert!(matches!(attempt.outcome, Outcome::Found));
    assert_eq!(attempt.provider.as_str(), "pmc");

    let again = engine.free_copy(&record).await;
    assert_eq!(again.kind(), FreeCopyKind::Pdf);
    assert_eq!(again.attempt.unwrap().served, Served::Cache);
    assert_eq!(http.requests().len(), 2);
}

#[tokio::test]
async fn a_manuscript_stays_browser_only() {
    // doi:10.1001/jama.2010.158 (#204): free on PMC as an author
    // manuscript, outside the open-access subset, with no PDF there.
    let record = on_pmc("PMC2918907", "10.1001/jama.2010.158");
    let (engine, http) = setup(
        vec![],
        [
            fixture(FIXTURES, "list_manuscript"),
            fixture(FIXTURES, "meta_manuscript"),
        ],
    );
    let copy = engine.free_copy(&record).await;
    assert_eq!(copy.kind(), FreeCopyKind::BrowserOnly);
    let oa = copy.open_access.unwrap();
    assert_eq!(oa.pdf_url.as_deref(), Some(RENDER));
    assert_eq!(
        oa.source.as_deref(),
        Some("europepmc"),
        "nothing taken from PMC"
    );
    assert!(matches!(copy.attempt.unwrap().outcome, Outcome::Found));
    assert_eq!(http.requests().len(), 2);
}

#[tokio::test]
async fn an_article_outside_pmcs_open_data_costs_one_request() {
    let record = on_pmc("PMC5008955", "10.5555/missing");
    let (engine, http) = setup(vec![], [fixture(FIXTURES, "list_missing")]);
    let copy = engine.free_copy(&record).await;
    assert_eq!(copy.kind(), FreeCopyKind::BrowserOnly);
    assert!(matches!(copy.attempt.unwrap().outcome, Outcome::NotFound));
    assert_eq!(http.requests().len(), 1);
}

#[tokio::test]
async fn a_work_with_a_pdf_or_no_pmcid_isnt_asked_about() {
    let (engine, http) = setup(vec![], []);
    let mut with_pdf = on_pmc("PMC8114386", "10.1080/21642850.2018.1462707");
    if let Some(oa) = with_pdf.work.open_access.as_mut() {
        oa.pdf_url = Some("https://journal.example/article.pdf".into());
    }
    let copy = engine.free_copy(&with_pdf).await;
    assert_eq!(copy.kind(), FreeCopyKind::Pdf);
    assert!(copy.attempt.is_none());

    let no_pmcid = deep_learning();
    let copy = engine.free_copy(&no_pmcid).await;
    assert_eq!(copy.kind(), FreeCopyKind::NoneKnown);
    assert!(copy.attempt.is_none());
    assert!(http.requests().is_empty());
}

#[tokio::test]
async fn a_pmcid_of_another_work_lends_no_copy() {
    // The record's PMCID, PMC's metadata says, is another DOI's.
    let record = on_pmc("PMC8114386", "10.1038/nature14539");
    let (engine, _) = setup(vec![], oa_fixtures());
    let copy = engine.free_copy(&record).await;
    assert_eq!(copy.kind(), FreeCopyKind::BrowserOnly);
    assert_eq!(copy.open_access.unwrap().pdf_url.as_deref(), Some(RENDER));
}
