//! `check`: a bibliography's claims to verdicts plus fixes.

mod common;

use std::sync::Arc;

use aspero_core::{Claim, WorkId};
use aspero_engine::{Finding, Severity, Verdict};
use common::*;

fn claim(title: &str, authors: &[&str], year: i16, ids: &[&str]) -> Claim {
    let mut c = Claim::default();
    c.title = Some(title.into());
    c.authors = authors.iter().map(|a| (*a).to_owned()).collect();
    c.year = Some(year);
    c.ids = ids.iter().map(|i| i.parse().unwrap()).collect();
    c
}

fn id(s: &str) -> WorkId {
    s.parse().unwrap()
}

fn setup() -> aspero_engine::Engine {
    let openalex = Arc::new(Scripted::new("openalex").keyword().with(vec![
        attention_real(),
        deep_learning(),
        deep_learning_for_ai(),
    ]));
    engine(vec![openalex])
}

#[tokio::test]
async fn a_bibliography_gets_verdicts_and_fixes() {
    let engine = setup();
    let claims = [
        // Correct, but no DOI given: suggest one.
        claim("Deep learning", &["LeCun, Yann"], 2015, &[]),
        // Wrong DOI (another real paper's): replace it.
        claim("Deep learning", &["LeCun, Y."], 2015, &["10.1145/3448250"]),
        // Right paper, cited a year late (off by one still verifies).
        claim(
            "Attention is all you need",
            &["Vaswani, A."],
            2018,
            &["arXiv:1706.03762"],
        ),
        // Invented.
        claim("A paper nobody wrote", &["Nobody, N."], 2020, &[]),
    ];

    let mut seen = Vec::new();
    let checked = engine.check(&claims, 5, |i| seen.push(i)).await;
    assert_eq!(seen, [0, 1, 2, 3], "progress per claim");

    let findings: Vec<Vec<&Finding>> = checked
        .iter()
        .map(|c| c.findings.iter().collect())
        .collect();

    assert_eq!(checked[0].verification.verdict, Verdict::Verified);
    assert!(findings[0].contains(&&Finding::AddDoi {
        doi: id("10.1038/nature14539")
    }));
    assert_eq!(checked[0].severity(), Some(Severity::Info));

    assert_eq!(checked[1].verification.verdict, Verdict::Mismatch);
    assert!(findings[1].contains(&&Finding::ReplaceId {
        claimed: id("10.1145/3448250"),
        with: Some(id("10.1038/nature14539")),
    }));
    assert_eq!(checked[1].severity(), Some(Severity::Error));

    assert!(findings[2].contains(&&Finding::YearDiffers {
        claimed: 2018,
        actual: 2017
    }));
    // A year off by one verifies and is only worth knowing.
    assert_eq!(checked[2].severity(), Some(Severity::Info));

    assert_eq!(checked[3].verification.verdict, Verdict::NotFound);
    assert_eq!(checked[3].severity(), Some(Severity::Error));
}

#[tokio::test]
async fn entries_citing_the_same_work_are_duplicates() {
    let engine = setup();
    let claims = [
        claim(
            "Deep learning",
            &["LeCun, Yann"],
            2015,
            &["10.1038/nature14539"],
        ),
        claim("Attention is all you need", &["Vaswani, A."], 2017, &[]),
        claim("Deep Learning", &["LeCun, Y.", "Bengio, Y."], 2015, &[]),
    ];
    let checked = engine.check(&claims, 5, |_| {}).await;
    assert!(checked[2].findings.contains(&Finding::Duplicate { of: 0 }));
    assert!(
        !checked[0]
            .findings
            .iter()
            .any(|f| matches!(f, Finding::Duplicate { .. }))
    );
    assert_eq!(
        checked[0].severity(),
        None,
        "clean entry, nothing to report"
    );
}
