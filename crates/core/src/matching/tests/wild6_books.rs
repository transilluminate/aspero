//! Red-team wild-6: books and reports not found, and flawed citations of
//! real works that scored as none. A book's title read from its layout
//! with its edition, its translators or authors after it, or an editor's
//! byline; a report cited without its organisation; a title quoted by
//! its opening words or with a word put in.

use super::*;

/// wild-6 #594: a title quoted by its opening words beside the work's
/// whole byline and year, no journal named: plausible.
#[test]
fn a_titles_opening_words_quoted_beside_its_whole_byline_are_plausible() {
    let kraft = article(
        "A Blueprint for Scaling Tutoring and Mentoring Across Public Schools",
        &["Matthew A. Kraft", "Grace T. Falken"],
        2021,
        ("AERA Open", "7", "23328584211042858"),
    );
    let text = "Kraft & Falken, \"A Blueprint for Scaling Tutoring\" (2021), and the Stanford \
                National Student Support Accelerator.";
    let a = assess(&Claim::from_text(text), &kraft);
    assert!(
        a.confidence >= PLAUSIBLE && a.confidence < VERIFIED,
        "{a:?}"
    );
    // The first author alone, or another year, isn't enough without the
    // journal.
    for text in [
        "Kraft et al., \"A Blueprint for Scaling Tutoring\" (2021).",
        "Kraft & Falken, \"A Blueprint for Scaling Tutoring\" (2019).",
    ] {
        let a = assess(&Claim::from_text(text), &kraft);
        assert!(a.confidence < PLAUSIBLE, "{text}: {a:?}");
    }
}
