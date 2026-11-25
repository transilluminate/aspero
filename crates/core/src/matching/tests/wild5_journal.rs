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

/// Mahal et al. 1997, in Science.
fn mahal() -> SourceRecord {
    article(
        "Engineering Chemical Reactivity on Cell Surfaces Through Oligosaccharide Biosynthesis",
        &["Lara K. Mahal", "Kevin J. Yarema", "Carolyn R. Bertozzi"],
        1997,
        ("Science", "276", "1125-1128"),
    )
}

/// Blackman et al. 2008, in JACS.
fn blackman() -> SourceRecord {
    article(
        "Tetrazine Ligation: Fast Bioconjugation Based on Inverse-Electron-Demand \
         Diels-Alder Reactivity",
        &["Melissa L. Blackman", "Maksim Royzen", "Joseph M. Fox"],
        2008,
        (
            "Journal of the American Chemical Society",
            "130",
            "13518-13519",
        ),
    )
}

/// wild-5 #579–#589: chemistry's ACS style, "Authors. Journal Year,
/// Volume, Pages", no title. The journal, volume and first page name the
/// work, its authors agreeing.
#[test]
fn acs_references_without_a_title_verify_at_their_place() {
    for (text, record) in [
        (
            "Mahal, Yarema & Bertozzi. Science 1997, 276, 1125-1128.",
            mahal(),
        ),
        ("Blackman, Royzen & Fox, JACS 2008, 130, 13518", blackman()),
        (
            "Agard, Prescher & Bertozzi. J. Am. Chem. Soc. 2004, 126, 15046-15047.",
            article(
                "A Strain-Promoted [3 + 2] Azide-Alkyne Cycloaddition for Covalent \
                 Modification of Biomolecules in Living Systems",
                &[
                    "Nicholas J. Agard",
                    "Jennifer A. Prescher",
                    "Carolyn R. Bertozzi",
                ],
                2004,
                (
                    "Journal of the American Chemical Society",
                    "126",
                    "15046-15047",
                ),
            ),
        ),
        (
            "Baskin et al. PNAS 2007, 104, 16793-16797.",
            article(
                "Copper-free click chemistry for dynamic in vivo imaging",
                &[
                    "Jeremy M. Baskin",
                    "Jennifer A. Prescher",
                    "Scott T. Laughlin",
                ],
                2007,
                (
                    "Proceedings of the National Academy of Sciences",
                    "104",
                    "16793-16797",
                ),
            ),
        ),
    ] {
        let a = assess(&Claim::from_text(text), &record);
        assert_eq!(a.volume.agreement, Agreement::Match, "{text}: {a:?}");
        assert_eq!(a.page.agreement, Agreement::Match, "{text}: {a:?}");
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
}

/// A chimera in that style: another work's place, or this work's authors at
/// another page or in another journal, doesn't verify.
#[test]
fn acs_references_at_another_place_or_by_others_do_not_verify() {
    for text in [
        // Other authors at Mahal's place.
        "Kowalczyk, Brandt & Ilves. Science 1997, 276, 1125-1128.",
        // Mahal's authors at another page, or in another journal.
        "Mahal, Yarema & Bertozzi. Science 1997, 276, 1225-1228.",
        "Mahal, Yarema & Bertozzi. Nature 1997, 276, 1125-1128.",
    ] {
        let a = assess(&Claim::from_text(text), &mahal());
        assert!(a.confidence < VERIFIED, "{text}: {a:?}");
    }
}
