//! OpenAlex's record of a DOI that is another year's work: the corrigendum
//! to the 2025 ESC/EACTS valvular guideline (`10.1093/ejcts/ezag193`, 2026)
//! served as the 2021 guideline's corrigendum (2022), with that one's free
//! copy. The titles differ only in the year they name, and the dates share
//! none: Crossref's record of the DOI wins.
#![cfg(feature = "openalex")]

mod common;

use std::sync::Arc;

use aspero_core::{Claim, OpenAccess, PartialDate, Raw, SourceRecord, WorkId};
use aspero_engine::{Extras, InputLookup, Looked};
use common::*;

const DOI: &str = "10.1093/ejcts/ezag193";
const OTHER: &str = "10.1093/eurheartj/ehac051";

fn title(year: u16) -> String {
    format!(
        "Corrigendum to: {year} ESC/EACTS Guidelines for the management of valvular heart disease"
    )
}

fn corrigendum(source: &'static str, ids: &[&str], year: u16, date: (i16, u8)) -> SourceRecord {
    let mut r = work(source, ids, &title(year), &[], date.0);
    r.work.issued = PartialDate::new(date.0, Some(date.1), None);
    r.work.container_title = Some("European Journal of Cardio-Thoracic Surgery".into());
    r.work.volume = Some("68".into());
    r.work.issue = Some("7".into());
    r
}

/// W4212870052, minimised: the DOI's, with the 2021 corrigendum's title,
/// date and free copy, its locations naming two PubMed records.
fn openalex() -> SourceRecord {
    let mut r = corrigendum("openalex", &["W4212870052", DOI], 2021, (2022, 2));
    r.work.page = Some("2022".into());
    let mut oa = OpenAccess::default();
    oa.is_oa = true;
    oa.url = Some(format!("https://doi.org/{OTHER}"));
    oa.source = Some("openalex".into());
    r.work.open_access = Some(oa);
    r.raw = Raw::json(&serde_json::json!({
        "doi": format!("https://doi.org/{DOI}"),
        "locations": [
            {"landing_page_url": format!("https://doi.org/{DOI}")},
            {"landing_page_url": format!("https://doi.org/{OTHER}")},
            {"landing_page_url": "https://pubmed.ncbi.nlm.nih.gov/35188539"},
            {"landing_page_url": "https://pubmed.ncbi.nlm.nih.gov/42452857"},
        ],
    }));
    r
}

fn registered() -> SourceRecord {
    corrigendum("crossref", &[DOI], 2025, (2026, 7))
}

fn engine_with(record: SourceRecord, registry: SourceRecord) -> aspero_engine::Engine {
    let openalex = Scripted::new("openalex").with(vec![record]);
    let mut crossref = Scripted::new("crossref")
        .with(vec![registry.clone()])
        .matcher(vec![registry]);
    crossref.caps.notices = true;
    engine(vec![Arc::new(openalex), Arc::new(crossref)])
}

fn looked(report: &aspero_engine::InputsReport) -> Looked {
    match &report.items[0] {
        InputLookup::Looked(l) => (**l).clone(),
        InputLookup::Invalid(e) => panic!("{e}"),
    }
}

#[tokio::test]
async fn a_lookup_serves_the_dois_own_year_not_another_years_work() {
    for extras in [Extras::default(), Extras::full()] {
        let engine = engine_with(openalex(), registered());
        let report = engine.lookup_inputs(&[DOI.to_owned()], extras).await;
        let looked = looked(&report);
        let record = looked.result.record.as_ref().unwrap();
        assert_eq!(record.source.as_str(), "crossref", "{extras:?}");
        assert_eq!(record.work.title.as_deref(), Some(title(2025).as_str()));
        assert_eq!(record.work.issued.unwrap().to_string(), "2026-07");
        // Not the other work's free copy, nor its identifiers.
        assert!(record.work.open_access.is_none(), "{extras:?}");
        let other: WorkId = OTHER.parse().unwrap();
        assert!(!record.ids.contains(&other));
        assert!(
            !record
                .ids
                .iter()
                .any(|id| id.to_string().contains("W4212870052"))
        );
        // What OpenAlex said is still reported.
        assert!(looked.disagreements.iter().any(|d| d.field == "issued"));
    }
}

#[tokio::test]
async fn the_same_year_in_both_titles_or_a_shared_date_keeps_openalexs_record() {
    // Titles naming one year: the online-first date beside the print one
    // stands as before.
    let mut same = openalex();
    same.work.title = Some(title(2025));
    same.work.open_access = None;
    // A title naming another year, the dates sharing one: a book review's
    // Crossref title adding the book's year, say.
    let mut shared = openalex();
    shared.work.issued = PartialDate::new(2026, Some(7), None);
    shared.work.open_access = None;
    for record in [same, shared] {
        let engine = engine_with(record, registered());
        let report = engine
            .lookup_inputs(&[DOI.to_owned()], Extras::full())
            .await;
        let looked = looked(&report);
        let record = looked.result.record.as_ref().unwrap();
        assert_eq!(record.source.as_str(), "openalex");
    }
}

#[tokio::test]
async fn verify_finds_the_dois_own_year() {
    let engine = engine_with(openalex(), registered());
    let claim = Claim::from_text(format!(
        "{}. Eur J Cardiothorac Surg. 2026;68(7). doi:{DOI}",
        title(2025)
    ));
    let report = engine.resolve(&claim, 5).await;
    let best = &report.candidates[0];
    assert_eq!(
        best.record.work.title.as_deref(),
        Some(title(2025).as_str())
    );
    assert_eq!(best.record.work.issued.unwrap().year(), 2026);
}
