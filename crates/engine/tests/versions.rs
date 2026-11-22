//! Following newer versions to the latest, as Cochrane reviews chain them.

mod common;

use std::sync::Arc;

use aspero_core::{Notice, NoticeKind, SourceRecord, WorkId};
use aspero_engine::{Engine, Verdict};
use common::*;

fn id(s: &str) -> WorkId {
    s.parse().unwrap()
}

/// A version of a review, superseded by `newer` if given.
fn version(doi: &str, year: i16, newer: Option<&str>) -> SourceRecord {
    let mut r = work(
        "crossref",
        &[doi],
        "Colloids versus crystalloids",
        &[],
        year,
    );
    if let Some(newer) = newer {
        let mut u = Notice::new(NoticeKind::NewVersion);
        u.notice = Some(id(newer));
        r.work.notices.push(u);
    }
    r
}

#[tokio::test]
async fn chains_are_followed_to_the_latest_version() {
    let mut crossref = Scripted::new("crossref").with(vec![
        version("10.1002/x.pub5", 2012, Some("10.1002/x.pub6")),
        version("10.1002/x.pub6", 2013, Some("10.1002/x.pub7")),
        version("10.1002/x.pub7", 2018, None),
        // A loop must not run forever.
        version("10.1002/loop.a", 2001, Some("10.1002/loop.b")),
        version("10.1002/loop.b", 2002, Some("10.1002/loop.a")),
    ]);
    crossref.caps.notices = true;
    let engine = engine(vec![Arc::new(crossref)]);

    let pub5 = version("10.1002/x.pub5", 2012, Some("10.1002/x.pub6"));
    let pub7 = version("10.1002/x.pub7", 2018, None);
    let loop_a = version("10.1002/loop.a", 2001, Some("10.1002/loop.b"));
    let report = engine.latest_versions(&[&pub5, &pub7, &loop_a]).await;

    let latest = |i: usize| report.latest[i].as_ref().map(|r| r.ids[0].to_string());
    assert_eq!(latest(0).as_deref(), Some("doi:10.1002/x.pub7"));
    assert_eq!(latest(1), None, "already the latest");
    assert_eq!(latest(2).as_deref(), Some("doi:10.1002/loop.b"));
}

/// A Cochrane review's 2015 version, as OpenAlex merges it with its 2020
/// version (both DOIs, no notice), and as Crossref gives it (superseded).
fn merged_versions() -> Engine {
    let title = "Multi-professional simulation-based team training in obstetric emergencies";
    let authors = [
        "Annemarie F. Fransen",
        "Joost van de Ven",
        "Franyke R. Banga",
    ];
    let mut merged = work(
        "openalex",
        &[
            "10.1002/14651858.cd011545",
            "10.1002/14651858.cd011545.pub2",
        ],
        title,
        &authors,
        2015,
    );
    merged.work.other_years = vec![2020];
    let mut old = work(
        "crossref",
        &["10.1002/14651858.cd011545"],
        title,
        &authors,
        2015,
    );
    let mut u = Notice::new(NoticeKind::NewVersion);
    u.notice = Some(id("10.1002/14651858.cd011545.pub2"));
    u.date = Some("2020-12-16".parse().unwrap());
    old.work.notices.push(u);
    for r in [&mut merged, &mut old] {
        r.work.container_title = Some("Cochrane Database of Systematic Reviews".into());
    }
    // Crossref's looked up; OpenAlex's found by searching for the
    // citation, and kept only if it matches better.
    let crossref = Scripted::new("crossref").with(vec![old]);
    let openalex = Scripted::new("openalex").matcher(vec![merged]);
    engine(vec![Arc::new(crossref), Arc::new(openalex)])
}

#[tokio::test]
async fn a_superseded_version_cited_with_its_newer_versions_year_is_not_verified() {
    // holdout-6 #481: the notice Crossref's record gives is weighed for
    // OpenAlex's record too, which would otherwise match better (the newer
    // version's year among its own) and be kept.
    let engine = merged_versions();
    let cited = |year: i16| {
        aspero_core::Claim::from_text(format!(
            "Fransen AF, van de Ven J, Banga FR. Multi-professional simulation-based team \
             training in obstetric emergencies. Cochrane Database Syst Rev. {year};CD011545. \
             doi: 10.1002/14651858.cd011545"
        ))
    };
    let v = engine.verify(&cited(2020), 5).await;
    assert_ne!(v.verdict, Verdict::Verified, "{}", v.reason_text);
    let v = engine.verify(&cited(2015), 5).await;
    assert_eq!(v.verdict, Verdict::Verified, "{}", v.reason_text);
}
