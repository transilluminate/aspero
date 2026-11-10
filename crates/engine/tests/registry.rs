//! Checking records against the DOI registry (Crossref): a record that
//! contradicts it is flagged, and isn't trusted for dating notices.

mod common;

use std::sync::Arc;

use aspero_core::{FieldSet, PartialDate, SourceRecord, Update, UpdateKind, WorkId};
use common::*;

fn id(s: &str) -> WorkId {
    s.parse().unwrap()
}

fn dated(mut r: SourceRecord, y: i16, m: u8, d: Option<u8>, page: &str) -> SourceRecord {
    r.work.issued = PartialDate::new(y, Some(m), d);
    r.work.page = Some(page.into());
    r
}

fn corrected_by(mut r: SourceRecord, notices: &[(&str, PartialDate)]) -> SourceRecord {
    for (notice, date) in notices {
        let mut u = Update::new(UpdateKind::Correction);
        u.notice = Some(id(notice));
        u.date = Some(*date);
        r.work.updates.push(u);
    }
    r
}

#[tokio::test]
async fn notices_are_dated_by_when_they_appeared_unless_records_conflict() {
    let date = |y, m, d| PartialDate::new(y, Some(m), d).unwrap();
    let guideline = || work("x", &["10.1234/guideline"], "Guideline", &LECUN, 2025);
    let notice = |doi| work("x", &[doi], "Correction to Guideline", &[], 2026);

    // OpenAlex answers first: the guideline, an online-first notice, and a
    // record that merged two notices (another notice's date and page).
    let openalex = Scripted::new("openalex").with(vec![
        guideline(),
        dated(notice("10.1234/fix.a"), 2026, 10, Some(12), "111313"),
        dated(notice("10.1234/fix.b"), 2025, 6, Some(19), "1541"),
    ]);
    // Crossref registered them: notices deposited with issue dates.
    let mut crossref = Scripted::new("crossref").with(vec![
        corrected_by(
            guideline(),
            &[
                ("10.1234/fix.a", date(2026, 12, Some(1))),
                ("10.1234/fix.b", date(2026, 6, Some(30))),
            ],
        ),
        dated(notice("10.1234/fix.a"), 2026, 12, None, "111313"),
        dated(notice("10.1234/fix.b"), 2026, 6, Some(30), "1823"),
    ]);
    crossref.caps.notices = true;
    let engine = engine(vec![Arc::new(openalex), Arc::new(crossref)]);

    let mut report = engine
        .lookup(
            &[id("10.1234/guideline"), id("10.1234/fix.b")],
            FieldSet::DEFAULT,
        )
        .await;
    engine.add_notices(&mut report).await;

    let updates = &report.results[0].record.as_ref().unwrap().work.updates;
    let a = updates
        .iter()
        .find(|u| u.notice == Some(id("10.1234/fix.a")))
        .unwrap();
    assert_eq!(
        a.date,
        Some(date(2026, 10, Some(12))),
        "online, not the issue"
    );
    assert_eq!(a.deposited, Some(date(2026, 12, Some(1))));
    let b = updates
        .iter()
        .find(|u| u.notice == Some(id("10.1234/fix.b")))
        .unwrap();
    assert_eq!(
        b.date,
        Some(date(2026, 6, Some(30))),
        "the merged record's date is ignored"
    );
    assert_eq!(b.deposited, None);

    let conflicts = engine.cross_check(&mut report).await;
    assert!(conflicts[0].is_empty());
    let fields: Vec<&str> = conflicts[1].iter().map(|d| d.field).collect();
    assert_eq!(fields, ["issued", "page"]);
    assert_eq!(conflicts[1][1].registry_value, "1823");
}
