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

// Who wrote it, where OpenAlex's merged record names another version's
// authors: W1949163494 has `.pub4`'s (Gates et al., 2018), Crossref's
// record of `.pub5` its own (Aregbesola et al., 2023), and its own title.

const PUB5_AUTHORS: [&str; 4] = [
    "Alex Aregbesola",
    "Clara M Tam",
    "Asha Kothari",
    "Terry P Klassen",
];
const PUB5_TITLE: &str = "Glucocorticoids for croup in children";
const PMC: &str = "https://www.ncbi.nlm.nih.gov/pmc/articles/PMC9831289";

/// OpenAlex's record, as [`merged`], naming `.pub4`'s authors (one of
/// them `.pub5`'s too), with a cited-by count and a free copy.
fn merged_pub4_authors() -> SourceRecord {
    let mut r = merged();
    r.work.authors = ["Allison Gates", "Michelle Gates", "Terry P Klassen"]
        .map(aspero_core::Person::literal)
        .to_vec();
    r.work.cited_by_count = Some(150);
    let mut oa = aspero_core::OpenAccess::default();
    oa.is_oa = true;
    oa.url = Some(PMC.into());
    r.work.open_access = Some(oa);
    r
}

fn registered_pub5() -> SourceRecord {
    let mut r = review("crossref", &[PUB5], (2023, 1, 10));
    r.work.authors = PUB5_AUTHORS.map(aspero_core::Person::literal).to_vec();
    r.work.title = Some(PUB5_TITLE.into());
    r
}

fn names(record: &SourceRecord) -> Vec<String> {
    record
        .work
        .authors
        .iter()
        .filter_map(aspero_core::Person::display_name)
        .collect()
}

#[tokio::test]
async fn a_lookup_serves_a_merged_versions_own_authors() {
    for extras in [Extras::default(), Extras::full()] {
        let engine = lookups(merged_pub4_authors(), registered_pub5());
        let report = engine.lookup_inputs(&[PUB5.to_owned()], extras).await;
        let looked = looked(&report);
        let record = looked.result.record.as_ref().unwrap();
        // Crossref's record, as where it overrules OpenAlex's.
        assert_eq!(record.source.as_str(), "crossref", "{extras:?}");
        assert_eq!(names(record), PUB5_AUTHORS, "{extras:?}");
        assert_eq!(record.work.title.as_deref(), Some(PUB5_TITLE));
        assert_eq!(record.work.issued.unwrap().to_string(), "2023-01-10");
        assert!(record.work.other_years.is_empty(), "{extras:?}");
        // What holds whichever version: OpenAlex's id, count and copy.
        assert!(record.ids.contains(&id("W1949163494")), "{:?}", record.ids);
        assert!(record.ids.contains(&id(PUB5)));
        assert_eq!(record.work.cited_by_count, Some(150));
        let oa = record.work.open_access.as_ref().unwrap();
        assert_eq!(oa.url.as_deref(), Some(PMC));
        // What OpenAlex said is still reported.
        assert_eq!(looked.disagreements[0].field, "issued");
    }
}

#[tokio::test]
async fn a_merged_record_sharing_a_year_keeps_its_authors() {
    // Its date the DOI's own: a merge (a notice and its article), not
    // another version. The authors differ by a misspelling in one record
    // (Crossref's "Shell" for Stiell), which isn't settled by the registry.
    let mut record = merged_pub4_authors();
    record.work.issued = PartialDate::new(2023, Some(1), Some(10));
    let engine = lookups(record, registered_pub5());
    for extras in [Extras::default(), Extras::full()] {
        let report = engine.lookup_inputs(&[PUB5.to_owned()], extras).await;
        let looked = looked(&report);
        let record = looked.result.record.as_ref().unwrap();
        assert_eq!(record.source.as_str(), "openalex", "{extras:?}");
        assert_eq!(names(record)[0], "Allison Gates", "{extras:?}");
    }
}

#[tokio::test]
async fn a_merged_version_merges_with_its_own_records_authors() {
    let claim = Claim::from_text(format!(
        "Aregbesola A, Tam CM, Kothari A, Klassen TP. Glucocorticoids for croup in children. \
         Cochrane Database Syst Rev. 2023;1:CD001955. doi:{PUB5}"
    ));
    let engine = gathering(merged_pub4_authors(), registered_pub5());
    let report = engine.resolve(&claim, 5).await;
    let best = &report.candidates[0];
    assert!(best.ids.contains(&id(PUB5)));
    assert_eq!(names(&best.record), PUB5_AUTHORS);
    assert_eq!(best.record.work.title.as_deref(), Some(PUB5_TITLE));
    assert_eq!(best.record.work.issued.unwrap().to_string(), "2023-01-10");
    let v = engine.verify(&claim, 5).await;
    assert_eq!(
        v.verdict,
        aspero_engine::Verdict::Verified,
        "{}",
        v.reason_text
    );

    // Cited with `.pub4`'s authors, `.pub5`'s DOI: the work found names
    // `.pub5`'s, whichever record leads it.
    let mixed = Claim::from_text(format!(
        "Gates A, Gates M, Klassen TP. Glucocorticoids for croup. \
         Cochrane Database Syst Rev. 2023;1:CD001955. doi:{PUB5}"
    ));
    let report = engine.resolve(&mixed, 5).await;
    assert_eq!(names(&report.candidates[0].record), PUB5_AUTHORS);
}
