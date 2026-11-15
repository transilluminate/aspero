//! `identify`: which work a document is, from its first page, against
//! scripted providers.

mod common;

use std::fmt::Write as _;
use std::sync::Arc;

use aspero_engine::{Decision, Verdict};
use common::*;

/// A first page that prints a related article's DOI before its own, cites
/// a third work, and ends with its own DOI in the footer.
fn page() -> String {
    let mut page = String::from(
        "See the related editorial: https://doi.org/10.1038/s41586-021-03819-2\n\
         Deep learning\n\
         Yann LeCun, Yoshua Bengio & Geoffrey Hinton\n\
         Deep learning allows computational models that are composed of multiple\n",
    );
    for i in 0..12 {
        let _ = writeln!(
            page,
            "processing layers to learn representations of data {i}."
        );
    }
    page.push_str("as shown before (doi:10.1145/3448250).\n");
    page.push_str("Nature 521, 436-444 (2015). doi:10.1038/nature14539\n");
    page
}

fn works() -> Vec<aspero_core::SourceRecord> {
    let editorial = work(
        "crossref",
        &["10.1038/s41586-021-03819-2"],
        "Highly accurate protein structure prediction with AlphaFold",
        &["Jumper"],
        2021,
    );
    vec![editorial, deep_learning_for_ai(), deep_learning()]
}

#[tokio::test]
async fn the_title_given_picks_the_documents_own_identifier() {
    let crossref = Arc::new(Scripted::new("crossref").with(works()));
    let engine = engine(vec![crossref]);
    let found = engine
        .identify(
            &page(),
            Some("Microsoft Word - final.docx"),
            Some("Deep learning"),
        )
        .await
        .identified
        .expect("identified");
    assert_eq!(found.decided_by, Decision::IdentifierAndTitle);
    let v = &found.verification;
    assert_eq!(v.verdict, Verdict::Verified);
    let best = &v.report.candidates[v.best.unwrap()];
    assert!(
        best.ids
            .contains(&"doi:10.1038/nature14539".parse().unwrap())
    );
}

#[tokio::test]
async fn without_a_title_the_page_confirms_its_own_identifier() {
    let crossref = Arc::new(Scripted::new("crossref").with(works()));
    let engine = engine(vec![crossref]);
    let found = engine
        .identify(&page(), Some("untitled"), None)
        .await
        .identified
        .expect("identified");
    assert_eq!(found.decided_by, Decision::IdentifierAndPage);
    let v = &found.verification;
    let best = &v.report.candidates[v.best.unwrap()];
    assert!(
        best.ids
            .contains(&"doi:10.1038/nature14539".parse().unwrap())
    );
}

#[tokio::test]
async fn with_no_identifier_confirmed_the_title_is_searched() {
    // The page's only identifier is another work's; the title finds it.
    let page = "Deep learning\nYann LeCun, Yoshua Bengio, Geoffrey Hinton\n\
                Nature 2015\nSee also doi:10.1145/3448250\n";
    let crossref = Arc::new(
        Scripted::new("crossref")
            .matcher(vec![deep_learning()])
            .with(vec![deep_learning_for_ai(), deep_learning()]),
    );
    let engine = engine(vec![crossref]);
    let found = engine
        .identify(page, None, Some("Deep learning"))
        .await
        .identified
        .expect("identified");
    assert_eq!(found.decided_by, Decision::TitleSearch);
    let v = &found.verification;
    let best = &v.report.candidates[v.best.unwrap()];
    assert!(
        best.ids
            .contains(&"doi:10.1038/nature14539".parse().unwrap())
    );
}

#[tokio::test]
async fn nothing_sure_enough_is_none() {
    let crossref = Arc::new(
        Scripted::new("crossref")
            .matcher(vec![])
            .with(works())
            .costing(0.25),
    );
    let engine = engine(vec![crossref]);
    let page = "Some unrelated notes\nSee doi:10.1145/3448250 for background.\n";
    let report = engine
        .identify(page, Some("Notes on things to do"), None)
        .await;
    assert!(report.identified.is_none());
    // What it spent finding nothing is still reported.
    let spent: f64 = report.usage.iter().filter_map(|(_, u)| u.cost).sum();
    assert!(spent >= 0.25, "{:?}", report.usage);
}

#[tokio::test]
async fn on_a_title_tie_the_header_identifier_wins() {
    let mut page = String::from("https://doi.org/10.1101/2015.01.01.000001\nDeep learning\n");
    for i in 0..6 {
        let _ = writeln!(page, "representations of data {i}.");
    }
    page.push_str("as published (doi:10.1038/nature14539).\n");
    for i in 0..6 {
        let _ = writeln!(page, "more layers {i}.");
    }
    let works = vec![
        work(
            "crossref",
            &["10.1101/2015.01.01.000001"],
            "Deep learning",
            &["LeCun"],
            2015,
        ),
        work(
            "crossref",
            &["10.1038/nature14539"],
            "Deep learning",
            &["LeCun"],
            2015,
        ),
    ];
    let crossref = Arc::new(Scripted::new("crossref").with(works));
    let engine = engine(vec![crossref]);
    let found = engine
        .identify(&page, None, Some("Deep learning"))
        .await
        .identified
        .expect("identified");
    assert_eq!(found.decided_by, Decision::IdentifierAndTitle);
    let v = &found.verification;
    let best = &v.report.candidates[v.best.unwrap()];
    assert!(
        best.ids
            .contains(&"doi:10.1101/2015.01.01.000001".parse().unwrap()),
        "{:?}",
        best.ids
    );
}
