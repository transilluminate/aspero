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

fn retracted(
    mut record: aspero_core::SourceRecord,
    notice: Option<&str>,
    source: &str,
) -> aspero_core::SourceRecord {
    let mut u = aspero_core::Update::new(aspero_core::UpdateKind::Retraction);
    u.notice = notice.map(|n| n.parse().unwrap());
    u.source = Some(source.into());
    record.work.updates.push(u);
    record
}

#[tokio::test]
async fn retractions_are_errors_with_the_detailed_notice() {
    // OpenAlex (first) has only the bare flag, under the "RETRACTED:" title;
    // Crossref has the Retraction Watch notice.
    let mut flagged = work(
        "openalex",
        &["10.1016/s0140-6736(97)11096-0"],
        "RETRACTED: Ileal-lymphoid-nodular hyperplasia, non-specific colitis",
        &["Andrew J Wakefield"],
        1998,
    );
    flagged = retracted(flagged, None, "openalex");
    let detailed = retracted(
        flagged.clone(),
        Some("10.1016/s0140-6736(10)60175-4"),
        "retraction-watch",
    );
    let openalex = Arc::new(Scripted::new("openalex").keyword().with(vec![flagged]));
    let crossref = Arc::new(Scripted::new("crossref").matcher(vec![detailed]));
    let engine = engine(vec![openalex, crossref]);

    let claims = [claim(
        "Ileal-lymphoid-nodular hyperplasia, non-specific colitis",
        &["Wakefield, A. J."],
        1998,
        &[],
    )];
    let checked = engine.check(&claims, 5, |_| {}).await;
    let c = &checked[0];

    assert_eq!(
        c.verification.verdict,
        Verdict::Verified,
        "the marker doesn't hide the title"
    );
    assert!(
        c.verification.reason.contains("note: RETRACTED"),
        "{}",
        c.verification.reason
    );
    let notices: Vec<_> = c
        .findings
        .iter()
        .filter_map(|f| match f {
            Finding::Updated { update } => Some(update.notice.as_ref().map(ToString::to_string)),
            _ => None,
        })
        .collect();
    assert_eq!(
        notices,
        [Some("doi:10.1016/s0140-6736(10)60175-4".to_owned())],
        "one, detailed"
    );
    assert_eq!(c.severity(), Some(Severity::Error));
}
