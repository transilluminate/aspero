//! A real work cited with an error, found but scoring below plausible: a
//! mismatch naming what differs, not a miss (wild-6 #710, #108).

mod common;

use std::sync::Arc;

use aspero_core::{Claim, SourceRecord};
use aspero_engine::{CitedField, Verdict, Verification};
use common::*;

async fn verify(hit: SourceRecord, text: &str) -> Verification {
    let crossref = Arc::new(
        Scripted::new("crossref")
            .matcher(vec![hit.clone()])
            .with(vec![hit]),
    );
    engine(vec![crossref])
        .verify(&Claim::from_text(text), 5)
        .await
}

fn year_differs(v: &Verification, actual: &str) -> bool {
    v.reason
        .differs()
        .iter()
        .any(|d| d.field == CitedField::Year && d.actual.as_deref() == Some(actual))
}

#[tokio::test]
async fn a_distinctive_title_with_only_its_year_wrong_is_a_mismatch() {
    let fordham = work(
        "crossref",
        &["10.3390/educsci2040242"],
        "Disciplinary History and the Situation of History Teachers",
        &["Michael Fordham"],
        2012,
    );
    let v = verify(
        fordham,
        "\"Disciplinary history and the situation of history teachers\" (2016)",
    )
    .await;
    assert_eq!(v.verdict, Verdict::Mismatch, "{}", v.reason_text);
    assert!(year_differs(&v, "2012"), "{:?}", v.reason);

    // A title of a word or two, with only a wrong year, names no work.
    let v = verify(deep_learning(), "\"Deep learning\" (2011)").await;
    assert_eq!(v.verdict, Verdict::NotFound, "{}", v.reason_text);

    // Nor a record with no byline to tell it from the claim's (holdout-5
    // #951), nor one whose main title alone the claim gives (wild-5 #437).
    let unsigned = work(
        "crossref",
        &["10.1000/unsigned"],
        "Disciplinary History and the Situation of History Teachers",
        &[],
        2012,
    );
    let v = verify(
        unsigned,
        "\"Disciplinary history and the situation of history teachers\" (2016)",
    )
    .await;
    assert_eq!(v.verdict, Verdict::NotFound, "{}", v.reason_text);
    let basals = work(
        "crossref",
        &["10.1000/basals"],
        "Teaching Children to Read in the Primary Grades: From Basals to Books",
        &["D. Ray Reutzel", "Robert B. Cooter"],
        1991,
    );
    let v = verify(
        basals,
        "National Reading Panel (2000), Teaching Children to Read in the Primary Grades.",
    )
    .await;
    assert_ne!(v.verdict, Verdict::Mismatch, "{}", v.reason_text);
}

#[tokio::test]
async fn the_whole_byline_with_a_word_put_in_the_title_and_the_year_wrong_is_a_mismatch() {
    let mut bowering = work(
        "crossref",
        &["10.1016/j.jpain.2012.09.007"],
        "The Effects of Graded Motor Imagery and Its Components on Chronic Pain: A Systematic \
         Review and Meta-Analysis",
        &["K. Jane Bowering", "Neil E. O'Connell", "Abby Tabor"],
        2013,
    );
    bowering.work.container_title = Some("The Journal of Pain".into());
    bowering.work.volume = Some("14".into());
    bowering.work.page = Some("3-13".into());
    let cited = |byline: &str| {
        format!(
            "{byline} (2020). The effects of graded motor imagery and its components on chronic \
             musculoskeletal pain: A systematic review and meta-analysis. European Journal of \
             Pain, 24(2), 262–276."
        )
    };
    let v = verify(
        bowering.clone(),
        &cited("Bowering, K. J., O'Connell, N. E., & Tabor, A."),
    )
    .await;
    assert_eq!(v.verdict, Verdict::Mismatch, "{}", v.reason_text);
    assert!(year_differs(&v, "2013"), "{:?}", v.reason);

    // The first author alone doesn't pin it.
    let v = verify(bowering, &cited("Bowering, K. J., et al.")).await;
    assert_eq!(v.verdict, Verdict::NotFound, "{}", v.reason_text);
}
