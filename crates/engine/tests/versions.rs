//! Following newer versions to the latest, as Cochrane reviews chain them.

mod common;

use std::sync::Arc;

use aspero_core::{SourceRecord, Update, UpdateKind, WorkId};
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
        let mut u = Update::new(UpdateKind::NewVersion);
        u.notice = Some(id(newer));
        r.work.updates.push(u);
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
