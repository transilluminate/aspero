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

/// Laughlin et al. 2008, in Science.
fn laughlin() -> SourceRecord {
    article(
        "In Vivo Imaging of Membrane-Associated Glycans in Developing Zebrafish",
        &[
            "Scott T. Laughlin",
            "Jeremy M. Baskin",
            "Sharon L. Amacher",
            "Carolyn R. Bertozzi",
        ],
        2008,
        ("Science", "320", "664-667"),
    )
}

/// wild-5 #586: a reference without a title naming an author the work
/// lacks ("Agard" for Amacher) is no verified citation at its place.
#[test]
fn a_stranger_in_a_titleless_byline_is_not_verified_at_the_place() {
    for text in [
        "Laughlin, Baskin, Agard & Bertozzi. Science 2008, 320, 664-667.",
        "Laughlin ST, Baskin JM, Agard NJ, Bertozzi CR. Science 2008;320:664-667.",
    ] {
        let a = assess(&Claim::from_text(text), &laughlin());
        assert!(
            (PLAUSIBLE..VERIFIED).contains(&a.confidence),
            "{text}: {a:?}"
        );
    }
    for text in [
        "Laughlin, Baskin, Amacher & Bertozzi. Science 2008, 320, 664-667.",
        "Laughlin ST, Baskin JM, Amacher SL, Bertozzi CR. Science 2008;320:664-667.",
        // A running header opening with its journal names no one.
        "Science 320 (2008) 664-667",
    ] {
        let a = assess(&Claim::from_text(text), &laughlin());
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
    // The byline ends at "et al." and at the journal: a label after it
    // ("CATIE Phase 2E"), or the journal's words ("Am"), are no names
    // (wild-5 #91, #196).
    let mcevoy = article(
        "Effectiveness of Clozapine Versus Olanzapine, Quetiapine, and Risperidone in \
         Patients With Chronic Schizophrenia Who Did Not Respond to Prior Atypical \
         Antipsychotic Treatment",
        &[
            "Joseph P. McEvoy",
            "Jeffrey A. Lieberman",
            "T. Scott Stroup",
        ],
        2006,
        ("American Journal of Psychiatry", "163", "600-610"),
    );
    let text = "McEvoy et al., CATIE Phase 2E, Am J Psychiatry 2006;163(4):600–610";
    let a = assess(&Claim::from_text(text), &mcevoy);
    assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    let guo = article(
        "Cardiovascular disease risk in patients with heart failure",
        &["Ying Guo", "Gregory Y.H. Lip"],
        2020,
        (
            "Journal of the American College of Cardiology",
            "75",
            "1523-1534",
        ),
    );
    for text in [
        "Guo et al., J Am Coll Cardiol 2020;75:1523–34",
        "Guo Y, Lip GYH. J Am Coll Cardiol 2020;75:1523–34",
    ] {
        let a = assess(&Claim::from_text(text), &guo);
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
}

/// wild-5 #400, #488: a work cited by its title's opening words or its
/// subtitle, quoted, beside its authors, year and journal: plausible.
#[test]
fn a_work_cited_by_a_quoted_part_of_its_title_is_plausible() {
    let hayden = article(
        "Some types of exercise are more effective than others in people with chronic low \
         back pain: a network meta-analysis",
        &["Jill A. Hayden", "Jenny Ellis", "Rachel Ogilvie"],
        2021,
        ("Journal of Physiotherapy", "67", "252-262"),
    );
    let wagner = article(
        "Insect decline in the Anthropocene: Death by a thousand cuts",
        &["David L. Wagner", "Eliza M. Grames", "Matthew L. Forister"],
        2021,
        (
            "Proceedings of the National Academy of Sciences",
            "118",
            "e2023989118",
        ),
    );
    let mut book = record(
        &["10.1515/9783110713329"],
        "Death by a Thousand Cuts",
        &["Matt Qvortrup"],
        2021,
    );
    book.work.work_type = Some(crate::work::WorkType::Book);
    let hayden_text = "Hayden et al. (2021), J Physiother : méta-analyse en réseau comparant \
                       les types d'exercice (« Some types of exercise are more effective than \
                       others »).";
    let wagner_text = "Wagner et al. 2021, PNAS special feature, \"Death by a thousand cuts\":";
    for (text, record) in [(hayden_text, &hayden), (wagner_text, &wagner)] {
        let a = assess(&Claim::from_text(text), record);
        assert!(
            (PLAUSIBLE..VERIFIED).contains(&a.confidence),
            "{text}: {a:?}"
        );
    }
    // Over the namesake by another author.
    let namesake = assess(&Claim::from_text(wagner_text), &book);
    assert!(namesake.confidence < PLAUSIBLE, "{namesake:?}");
    // Not beside another year, nor other authors, nor words of the title
    // from its middle.
    for text in [
        "Wagner et al. 2019, PNAS special feature, \"Death by a thousand cuts\":",
        "Kessler et al. 2021, PNAS special feature, \"Death by a thousand cuts\":",
        "Wagner et al. 2021, PNAS, \"decline in the Anthropocene\":",
    ] {
        let a = assess(&Claim::from_text(text), &wagner);
        assert!(a.confidence < PLAUSIBLE, "{text}: {a:?}");
    }
}
