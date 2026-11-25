//! Red-team wild-5: journal references without a title, found by their
//! journal, volume and first page, and what the place doesn't confirm (a
//! byline with a stranger); works cited by part of their title.

use super::*;

/// Albers et al. 2005, as Crossref names its journal.
fn albers(journal: &str) -> SourceRecord {
    article(
        "Midwifery Care Measures in the Second Stage of Labor and Reduction of Genital \
         Tract Trauma at Birth: A Randomized Trial",
        &["Leah L. Albers", "Kay D. Sedler", "Edward J. Bedrick"],
        2005,
        (journal, "50", "365-372"),
    )
}

/// wild-5 #7: a journal with a possessive ("Women's"), abbreviated
/// without its apostrophe, and a description after the place.
#[test]
fn a_possessive_journal_abbreviated_without_its_apostrophe_is_the_journal() {
    let claim = Claim::from_text(
        "Albers L, et al. J Midwifery Womens Health 2005;50(5):365-372. A larger RCT of \
         about 1,200 women comparing several midwifery measures.",
    );
    for journal in [
        "Journal of Midwifery & Women's Health",
        // As OpenAlex has it, the apostrophe a space.
        "Journal of Midwifery & Women s Health",
    ] {
        let a = assess(&claim, &albers(journal));
        assert_eq!(a.container.agreement, Agreement::Match, "{journal}: {a:?}");
        assert!(a.confidence >= VERIFIED, "{journal}: {a:?}");
    }
}
