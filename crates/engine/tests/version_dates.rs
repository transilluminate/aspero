//! A work's date where OpenAlex's record merges it with another work: a
//! Cochrane review's `.pub5` (2023) merged with its earlier versions has
//! the 2011 version's date (W1949163494). The DOI's own record (Crossref's,
//! a version's alone) dates it; OpenAlex's year is kept among its other
//! years. A record without the signs keeps its earlier (online-first) date.
#![cfg(feature = "openalex")]

mod common;

use std::sync::Arc;

use aspero_core::{Claim, PartialDate, Raw, SourceRecord, WorkId};
use aspero_engine::{Extras, InputLookup, Looked};
use common::*;

const PUB5: &str = "10.1002/14651858.cd001955.pub5";
const PUB4: &str = "10.1002/14651858.cd001955.pub4";
const TITLE: &str = "Glucocorticoids for croup";
const AUTHORS: [&str; 3] = ["Allison Gates", "Michelle Gates", "Ben Vandermeer"];

fn id(s: &str) -> WorkId {
    s.parse().unwrap()
}

fn review(source: &'static str, ids: &[&str], date: (i16, u8, u8)) -> SourceRecord {
    let mut r = work(source, ids, TITLE, &AUTHORS, date.0);
    r.work.issued = PartialDate::new(date.0, Some(date.1), Some(date.2));
    r.work.container_title = Some("Cochrane Database of Systematic Reviews".into());
    r.work.volume = Some("2023".into());
    r.work.issue = Some("1".into());
    r.work.page = Some("CD001955".into());
    r
}

/// OpenAlex's record, minimised: `.pub5`'s DOI, the 2011 version's date,
/// and locations at its earlier versions' DOIs (`urls`).
fn openalex(urls: &[String]) -> SourceRecord {
    let mut r = review("openalex", &["W1949163494", PUB5], (2011, 1, 19));
    let locations: Vec<_> = urls
        .iter()
        .map(|u| serde_json::json!({ "landing_page_url": u }))
        .collect();
    r.raw = Raw::json(&serde_json::json!({
        "doi": format!("https://doi.org/{PUB5}"),
        "locations": locations,
    }));
    r
}

fn merged() -> SourceRecord {
    openalex(&[
        format!("https://doi.org/{PUB5}"),
        format!("https://doi.org/{PUB4}"),
    ])
}

fn registered() -> SourceRecord {
    review("crossref", &[PUB5], (2023, 1, 10))
}

fn lookups(record: SourceRecord, registry: SourceRecord) -> aspero_engine::Engine {
    let openalex = Scripted::new("openalex").with(vec![record]);
    let mut crossref = Scripted::new("crossref").with(vec![registry]);
    crossref.caps.notices = true;
    engine(vec![Arc::new(openalex), Arc::new(crossref)])
}

/// As live: Crossref's record of the DOI is found by its citation search.
fn gathering(record: SourceRecord, registry: SourceRecord) -> aspero_engine::Engine {
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
async fn a_lookup_dates_a_merged_version_by_its_own_record() {
    for extras in [Extras::default(), Extras::full()] {
        let engine = lookups(merged(), registered());
        let report = engine.lookup_inputs(&[PUB5.to_owned()], extras).await;
        let looked = looked(&report);
        let record = looked.result.record.as_ref().unwrap();
        assert_eq!(record.source.as_str(), "openalex", "{extras:?}");
        assert_eq!(record.work.issued.unwrap().to_string(), "2023-01-10");
        assert_eq!(record.work.other_years, [2011]);
        // What OpenAlex said is still reported.
        let fields: Vec<_> = looked
            .disagreements
            .iter()
            .map(|d| (d.field, d.value.as_str(), d.registry_value.as_str()))
            .collect();
        assert_eq!(fields, [("issued", "2011-01-19", "2023-01-10")]);
    }
}

#[tokio::test]
async fn a_record_without_the_signs_keeps_its_date() {
    // OpenAlex's online-first date, a year before the print issue
    // Crossref dates the DOI by: the record shows no other work.
    let mut record = openalex(&[format!("https://doi.org/{PUB5}")]);
    record.work.issued = PartialDate::new(2022, Some(12), Some(22));
    let engine = lookups(record, registered());
    let report = engine
        .lookup_inputs(&[PUB5.to_owned()], Extras::full())
        .await;
    let looked = looked(&report);
    let record = looked.result.record.as_ref().unwrap();
    assert_eq!(record.work.issued.unwrap().to_string(), "2022-12-22");
    assert_eq!(looked.disagreements.len(), 1);
}

/// Gathering a claim's candidates (as `verify` does until one is
/// decisive), the records of `.pub5` merge into one work.
#[tokio::test]
async fn a_merged_version_merges_with_its_own_records_date() {
    let engine = gathering(merged(), registered());
    let claim = Claim::from_text(format!(
        "Gates A, Gates M, Vandermeer B, et al. Glucocorticoids for croup. \
         Cochrane Database Syst Rev. 2023;1:CD001955. doi:{PUB5}"
    ));
    let report = engine.resolve(&claim, 5).await;
    let best = &report.candidates[0];
    assert!(best.ids.contains(&id(PUB5)));
    assert_eq!(best.record.work.issued.unwrap().to_string(), "2023-01-10");
    assert!(best.record.work.other_years.contains(&2011));

    // Without the signs, the earliest date stands, as before.
    let mut online = openalex(&[]);
    online.work.issued = PartialDate::new(2022, Some(12), Some(22));
    let engine = gathering(online, registered());
    let report = engine.resolve(&claim, 5).await;
    let issued = report.candidates[0].record.work.issued.unwrap();
    assert_eq!(issued.to_string(), "2022-12-22");
}
