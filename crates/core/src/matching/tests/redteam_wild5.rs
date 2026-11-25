//! Red-team wild-5: wrong citations that matched the work meant, one field
//! wrong — a title cut off or rewritten beside the work's place or DOI, a
//! journal read only loosely.

use super::*;

fn twenge() -> SourceRecord {
    let mut r = placed(
        &["10.1177/2167702617723376"],
        "Increases in Depressive Symptoms, Suicide-Related Outcomes, and Suicide Rates Among \
         U.S. Adolescents After 2010 and Links to Increased New Media Screen Time",
        &["Jean M. Twenge", "Thomas E. Joiner", "Megan L. Rogers"],
        2017,
        ("Clinical Psychological Science", "6", "3-17"),
    );
    r.work.work_type = Some(crate::work::WorkType::JournalArticle);
    r
}

fn plausible(a: &Assessment) -> bool {
    (PLAUSIBLE..VERIFIED).contains(&a.confidence)
}

/// wild-5 #353: the main title cut before the words that carry the
/// finding, at the work's place: plausible, the title partly the work's.
#[test]
fn a_title_cut_off_at_its_place_is_plausible() {
    let claim = Claim::from_text(
        "Twenge, J. M., Joiner, T. E., Rogers, M. L., & Martin, G. N. (2018). Increases in \
         depressive symptoms, suicide-related outcomes, and suicide rates among U.S. adolescents \
         after 2010. Clinical Psychological Science, 6(1), 3–17.",
    );
    let a = assess(&claim, &twenge());
    assert_eq!(a.title.agreement, Agreement::Partial, "{a:?}");
    assert!(plausible(&a), "{a:?}");
    // The same, its DOI given: still plausible.
    let claim = Claim::from_text(
        "Twenge JM, Joiner TE, Rogers ML. Increases in depressive symptoms, suicide-related \
         outcomes, and suicide rates among U.S. adolescents after 2010. Clin Psychol Sci. \
         2018;6(1):3-17. doi:10.1177/2167702617723376",
    );
    assert!(plausible(&assess(&claim, &twenge())));
}

/// wild-5 #762: a quoted title cut inside a word's possessive ("'Oumuamua?"
/// for "'Oumuamua's Peculiar Acceleration?").
#[test]
fn a_quoted_title_cut_off_is_plausible() {
    let mut r = placed(
        &["10.3847/2041-8213/aaeda8"],
        "Could Solar Radiation Pressure Explain ‘Oumuamua’s Peculiar Acceleration?",
        &["Shmuel Bialy", "Abraham Loeb"],
        2018,
        ("The Astrophysical Journal Letters", "868", "L1"),
    );
    r.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let claim = Claim::from_text(
        "Shmuel Bialy and Abraham Loeb, \"Could Solar Radiation Pressure Explain 'Oumuamua?\" \
         Astrophysical Journal Letters 868, L1 (2018).",
    );
    let a = assess(&claim, &r);
    assert!(plausible(&a), "{a:?}");
}

/// Not cut off: a subtitle dropped, a cut marked "…", a misspelt or
/// footnoted last word, the whole title given.
#[test]
fn a_title_shortened_as_people_do_is_no_cut() {
    for (text, title) in [
        (
            "Smith J. Does money matter? Educ Res. 1994;23:5-14.",
            "Does Money Matter? A Meta-Analysis of Studies of the Effects of School Inputs",
        ),
        (
            "Smith J. Characteristics of useful code reviews. MSR 2015.",
            "Characteristics of useful code reviews: an empirical study at Microsoft",
        ),
        (
            "Smith J. Scaling rectified flow transformers… ICML 2024.",
            "Scaling Rectified Flow Transformers for High-Resolution Image Synthesis",
        ),
        (
            "Williams M (1988) 'Epistemological Realism and the Basis of Scepticism', Mind, 97.",
            "Epistemological Realism and the Basis of Scepticism1",
        ),
        (
            "Loh R (2006). The pathology of devil facial tumor disease in Tasmanian devils. Vet Pathol.",
            "The Pathology of Devil Facial Tumor Disease (DFTD) in Tasmanian Devils",
        ),
    ] {
        assert!(!Normalised::new(text).cut_off(title, false), "{text}");
    }
    let whole = "Could Solar Radiation Pressure Explain Oumuamua";
    assert!(Normalised::new(whole).cut_off(
        "Could Solar Radiation Pressure Explain ‘Oumuamua’s Peculiar Acceleration?",
        true
    ));
}

/// A title reworded at its end ("… transplant recipients" for "…
/// transplantation: a randomized …") is not cut there.
#[test]
fn a_title_reworded_at_its_end_is_no_cut() {
    let text = Normalised::new(
        "Streitberger K. Effect of acupuncture as antiemetic prophylaxis in stem cell transplant \
         recipients. Clin Cancer Res 2003;9:2538-44.",
    );
    assert!(!text.cut_off(
        "Effect of acupuncture as antiemetic prophylaxis in stem cell transplantation: a \
         randomized controlled single-blind trial.",
        false
    ));
}

/// wild-5 #194: an RIS record's title rewritten beside the work's DOI,
/// authors, journal and pages: plausible, the title to check.
#[test]
fn a_title_given_apart_and_rewritten_beside_the_doi_is_plausible() {
    let mut r = placed(
        &["10.1016/j.ahj.2018.09.002"],
        "Rationale and design of a large-scale, app-based study to identify cardiac \
         arrhythmias using a smartwatch: The Apple Heart Study",
        &["Mintu P. Turakhia", "Manisha Desai", "Haley Hedlin"],
        2018,
        ("American Heart Journal", "207", "66-75"),
    );
    r.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let claim = Claim::from_input(
        "TY  - JOUR\nTI  - Rationale and design of the Apple Heart Study: A prospective, \
         single-arm, pragmatic study to evaluate the ability of a smartwatch to identify atrial \
         fibrillation\nAU  - Turakhia, Mintu P.\nAU  - Desai, Manisha\nT2  - American Heart \
         Journal\nPY  - 2019\nVL  - 207\nSP  - 66\nEP  - 75\nDO  - 10.1016/j.ahj.2018.09.002\nER  -",
    );
    assert!(claim.title.is_some());
    let a = assess(&claim, &r);
    assert_eq!(a.ids, Agreement::Match);
    assert!(plausible(&a), "{a:?}");
}

/// The modifier letter apostrophe some records print ("womenʼs",
/// holdout-5 #1074) is an apostrophe, not a letter.
#[test]
fn a_modifier_letter_apostrophe_is_an_apostrophe() {
    assert_eq!(
        normalise("women\u{2bc}s perceptions"),
        normalise("women's perceptions")
    );
}

/// wild-5 #375 and #560: a journal read loosely (no stop before the
/// closing year; after a quoted title, before a note) and clearly another:
/// plausible, the journal to check.
#[test]
fn a_loosely_read_journal_that_is_another_is_plausible() {
    let mut ehb = record(
        &["10.1016/j.ehb.2018.08.007"],
        "How much do conditional cash transfers increase the utilization of maternal and child \
         health care services? New evidence from Janani Suraksha Yojana in India",
        &["Mohammad Mahbubur Rahman", "Saseendran Pallikadavath"],
        2018,
    );
    ehb.work.container_title = Some("Economics & Human Biology".into());
    ehb.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let claim = Claim::from_text(
        "Rahman MM, Pallikadavath S. How much do conditional cash transfers increase the \
         utilization of maternal and child health care services? New evidence from Janani \
         Suraksha Yojana in India. Popul Space Place 2018.",
    );
    let a = assess(&claim, &ehb);
    assert_eq!(a.container.agreement, Agreement::Mismatch, "{a:?}");
    assert!(plausible(&a), "{a:?}");
    // Its own journal so cited verifies.
    let claim = Claim::from_text(
        "Rahman MM, Pallikadavath S. How much do conditional cash transfers increase the \
         utilization of maternal and child health care services? New evidence from Janani \
         Suraksha Yojana in India. Econ Hum Biol 2018.",
    );
    assert!(assess(&claim, &ehb).confidence >= VERIFIED);

    let mut bri = record(
        &["10.1080/09613218.2014.979093"],
        "Bridging the domestic building fabric performance gap",
        &[
            "David Johnston",
            "David Farmer",
            "Malcolm Brooke-Peat",
            "David Miles-Shenton",
        ],
        2014,
    );
    bri.work.container_title = Some("Building Research & Information".into());
    bri.work.work_type = Some(crate::work::WorkType::JournalArticle);
    bri.work.other_years = vec![2016];
    let claim = Claim::from_text(
        "Johnston, Farmer, Brooke-Peat & Miles-Shenton (2016), \"Bridging the domestic building \
         fabric performance gap,\" BSERT, a follow-up on remedies.",
    );
    let a = assess(&claim, &bri);
    assert_eq!(a.container.agreement, Agreement::Mismatch, "{a:?}");
    assert!(plausible(&a), "{a:?}");
}

/// A word put in place by its opposite rewords the title, however near
/// its spelling: "Southern" for "Northern" (holdout-6 #863), at the work's
/// place, is no misspelling; plausible, the title to check.
#[test]
fn a_title_with_a_word_replaced_by_its_opposite_is_reworded() {
    let mut r = placed(
        &[],
        "Maritime Archaeology in Britain and Northern Ireland 1997 & 1998",
        &["Joe Flatman", "Lucy Blue"],
        1999,
        (
            "The International Journal of Nautical Archaeology",
            "28",
            "174-199",
        ),
    );
    r.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let claim = Claim::from_text(
        "FLATMAN, J. and BLUE, L. (1999) 'Maritime Archaeology in Britain and Southern Ireland \
         1997 & 1998', The International Journal of Nautical Archaeology, 28(2), pp. 174–199.",
    );
    let a = assess(&claim, &r);
    assert!(plausible(&a), "{a:?}");
    // Given apart, the same.
    let given = Claim {
        title: Some("Maritime Archaeology in Britain and Southern Ireland 1997 & 1998".into()),
        authors: vec!["Flatman J".into(), "Blue L".into()],
        year: Some(1999),
        container: Some("International Journal of Nautical Archaeology".into()),
        volume: Some("28".into()),
        page: Some("174".into()),
        ..Claim::default()
    };
    assert!(plausible(&assess(&given, &r)), "{:?}", assess(&given, &r));
}
