//! Where free copies are: filled in from other providers, and kept when
//! candidates merge.

mod common;

use std::sync::Arc;

use aspero_core::{Claim, Field, FieldSet, OpenAccess, SourceRecord, WorkId};
use common::*;

fn free_pdf(mut record: SourceRecord, source: &str) -> SourceRecord {
    let mut oa = OpenAccess::default();
    oa.is_oa = true;
    oa.pdf_url = Some("https://example.org/free.pdf".into());
    oa.source = Some(source.into());
    record.work.open_access = Some(oa);
    record
}

fn without_open_access(mut s: Scripted) -> Scripted {
    s.caps.fields = FieldSet::ALL.difference(FieldSet::of(&[Field::OpenAccess]));
    s
}

#[tokio::test]
async fn lookups_borrow_open_access_from_providers_that_report_it() {
    let crossref = Arc::new(without_open_access(
        Scripted::new("crossref").with(vec![deep_learning()]),
    ));
    let europepmc =
        Arc::new(Scripted::new("europepmc").with(vec![free_pdf(deep_learning(), "europepmc")]));
    let engine = engine(vec![crossref, europepmc.clone()]);
    let doi: WorkId = "10.1038/nature14539".parse().unwrap();

    let mut report = engine.lookup(&[doi], FieldSet::DEFAULT).await;
    let record = report.results[0].record.as_ref().unwrap();
    assert_eq!(record.source.as_str(), "crossref");
    assert!(record.work.open_access.is_none());

    engine.add_open_access(&mut report).await;
    let record = report.results[0].record.as_ref().unwrap();
    let oa = record.work.open_access.as_ref().unwrap();
    assert_eq!(oa.source.as_deref(), Some("europepmc"), "provenance kept");
    assert_eq!(
        record.source.as_str(),
        "crossref",
        "the record stays Crossref's"
    );
}

#[tokio::test]
async fn merged_candidates_keep_a_free_copy_whichever_record_wins() {
    // The matcher's record wins (it's first and as good); OpenAlex's,
    // merged into it, knows the free copy.
    let crossref = Arc::new(Scripted::new("crossref").matcher(vec![deep_learning()]));
    let openalex = Arc::new(
        Scripted::new("openalex")
            .keyword()
            .with(vec![free_pdf(deep_learning(), "openalex")]),
    );
    let engine = engine(vec![openalex, crossref]);
    let report = engine
        .resolve(
            &Claim::from_text("LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015."),
            5,
        )
        .await;

    let best = &report.candidates[0];
    assert_eq!(best.sightings.len(), 2);
    assert_eq!(best.record.source.as_str(), "crossref");
    let oa = best.record.work.open_access.as_ref().unwrap();
    assert_eq!(oa.source.as_deref(), Some("openalex"));
}
