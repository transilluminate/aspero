//! `verify` of a claim whose identifier and exact place (journal, volume,
//! first page) agree while its title or byline doesn't (wild-4 #361, #367,
//! #369): not outvoted by a lookalike whose identifiers and page differ.

mod common;

use std::sync::Arc;

use aspero_core::{Claim, SourceRecord};
use aspero_engine::{Reason, Verdict};
use common::*;

const TTH: &str = "Observation of tt¯H Production";

/// CMS's tt¯H observation, in PRL 120, 231801.
fn tth() -> SourceRecord {
    let mut r = work(
        "crossref",
        &["10.1103/physrevlett.120.231801", "arXiv:1804.02610"],
        TTH,
        &["A. M. Sirunyan", "A. Tumasyan", "W. Adam"],
        2018,
    );
    r.work.container_title = Some("Physical Review Letters".into());
    r.work.volume = Some("120".into());
    r.work.page = Some("231801".into());
    r
}

/// A conference talk on it: its own DOI, another venue, another page.
fn talk(year: i16) -> SourceRecord {
    let mut r = work(
        "crossref",
        &["10.22323/1.340.0001"],
        "Observation of ttH production at CMS",
        &["Kirill Skovpen"],
        year,
    );
    r.work.container_title = Some("Proceedings of Science".into());
    r.work.volume = Some("340".into());
    r.work.page = Some("1".into());
    r
}

fn engine_with(talk_year: i16) -> aspero_engine::Engine {
    let crossref = Arc::new(
        Scripted::new("crossref")
            .matcher(vec![talk(talk_year)])
            .with(vec![tth(), talk(talk_year)]),
    );
    engine(vec![crossref])
}

#[tokio::test]
async fn an_identifier_and_place_that_agree_beat_a_lookalike() {
    let claim = Claim::from_text(
        "CMS ttH observation | Observation of ttH production | PRL 120, 231801 (2018) | \
         10.1103/PhysRevLett.120.231801",
    );
    let v = engine_with(2018).verify(&claim, 5).await;
    // Plausible where the title is held against it; verified once the
    // record's "tt¯H" reads as the "ttH" cited.
    assert!(
        matches!(v.verdict, Verdict::Verified | Verdict::Plausible),
        "{}",
        v.reason_text
    );
    assert!(
        matches!(v.reason, Reason::IdPartlyMatches { .. } | Reason::IdAgrees),
        "{:?}",
        v.reason
    );
    let best = &v.report.candidates[v.best_match.unwrap()];
    assert_eq!(best.record.work.title.as_deref(), Some(TTH));
}

#[tokio::test]
async fn a_wrong_year_at_the_identifiers_place_is_not_sent_to_a_lookalike() {
    // The year two off: not plausible, but the identifier and the place
    // still name the work, not the talk (of that year) with another DOI
    // and page.
    let claim = Claim::from_text(
        "CMS ttH observation | Observation of ttH production | PRL 120, 231801 (2016) | \
         10.1103/PhysRevLett.120.231801",
    );
    let v = engine_with(2016).verify(&claim, 5).await;
    assert!(
        !matches!(v.reason, Reason::IdElsewhere { .. }),
        "{}",
        v.reason_text
    );
    let best = &v.report.candidates[v.best_match.unwrap()];
    assert_eq!(best.record.work.title.as_deref(), Some(TTH));
}
