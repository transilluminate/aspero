//! `add_notices`: editorial notices in both directions from the provider
//! that reports them (Crossref), onto records another provider supplied.

mod common;

use std::sync::Arc;

use aspero_core::{FieldSet, Update, UpdateKind, UpdateTo, WorkId};
use common::*;

fn id(s: &str) -> WorkId {
    s.parse().unwrap()
}

#[tokio::test]
async fn originals_and_their_corrections_point_at_each_other() {
    let mut guideline = work("openalex", &["10.1161/cir.1415"], "Guideline", &LECUN, 2026);
    let mut erratum = work(
        "openalex",
        &["10.1161/cir.1462"],
        "Correction to: Guideline",
        &[],
        2026,
    );
    let openalex =
        Arc::new(Scripted::new("openalex").with(vec![guideline.clone(), erratum.clone()]));

    let mut correction = Update::new(UpdateKind::Correction);
    correction.notice = Some(id("10.1161/cir.1462"));
    guideline.work.updates = vec![correction.clone()];
    erratum.work.update_to = vec![UpdateTo::new(
        UpdateKind::Correction,
        id("10.1161/cir.1415"),
    )];
    let mut crossref = Scripted::new("crossref").with(vec![guideline, erratum]);
    crossref.caps.notices = true;
    let crossref = Arc::new(crossref);

    let engine = engine(vec![openalex, crossref]);
    let mut report = engine
        .lookup(
            &[id("10.1161/cir.1415"), id("10.1161/cir.1462")],
            FieldSet::DEFAULT,
        )
        .await;
    let source = |i: usize| {
        report.results[i]
            .record
            .as_ref()
            .unwrap()
            .source
            .to_string()
    };
    assert_eq!(
        (source(0), source(1)),
        ("openalex".into(), "openalex".into())
    );

    engine.add_notices(&mut report).await;
    let work = |i: usize| &report.results[i].record.as_ref().unwrap().work;
    assert_eq!(work(0).updates, [correction]);
    assert_eq!(work(1).update_to[0].work, id("10.1161/cir.1415"));

    // Merging again adds nothing.
    engine.add_notices(&mut report).await;
    let work = |i: usize| &report.results[i].record.as_ref().unwrap().work;
    assert_eq!((work(0).updates.len(), work(1).update_to.len()), (1, 1));
}

#[tokio::test]
async fn a_notice_flagged_as_retracted_names_itself_as_the_notice() {
    // OpenAlex flags the withdrawal notice (Cochrane .pub3) as retracted.
    let mut flagged = work(
        "openalex",
        &["10.1002/x.pub3"],
        "Withdrawn review",
        &[],
        2015,
    );
    flagged.work.updates = vec![Update::new(UpdateKind::Retraction)];
    let openalex = Arc::new(Scripted::new("openalex").with(vec![flagged]));
    // Crossref knows it is the notice withdrawing .pub2.
    let mut notice = work(
        "crossref",
        &["10.1002/x.pub3"],
        "Withdrawn review",
        &[],
        2015,
    );
    notice.work.update_to = vec![UpdateTo::new(UpdateKind::Withdrawal, id("10.1002/x.pub2"))];
    let mut crossref = Scripted::new("crossref").with(vec![notice]);
    crossref.caps.notices = true;

    let engine = engine(vec![openalex, Arc::new(crossref)]);
    let mut report = engine
        .lookup(&[id("10.1002/x.pub3")], FieldSet::DEFAULT)
        .await;
    engine.add_notices(&mut report).await;

    let work = &report.results[0].record.as_ref().unwrap().work;
    assert_eq!(work.updates.len(), 1, "flagged, still");
    assert_eq!(work.updates[0].notice, Some(id("10.1002/x.pub3")));
}
