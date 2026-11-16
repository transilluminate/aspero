//! Scoring against hand-built records, each case from a real failure.

use jiff::Timestamp;

use super::*;
use crate::provider::ProviderId;
use crate::record::{FieldSet, Raw};
use crate::work::{PartialDate, Person, Work};

fn record(ids: &[&str], title: &str, authors: &[&str], year: i16) -> SourceRecord {
    let work = Work {
        title: Some(title.into()),
        authors: authors.iter().map(|a| Person::literal(*a)).collect(),
        issued: Some(PartialDate::from_year(year)),
        ..Work::default()
    };
    let raw = Raw {
        media_type: "application/json".into(),
        body: "{}".into(),
    };
    SourceRecord::new(
        ProviderId::from_static("test"),
        ids.iter().map(|i| i.parse().unwrap()).collect(),
        work,
        FieldSet::ALL,
        raw,
        Timestamp::UNIX_EPOCH,
    )
}

const VASWANI: [&str; 3] = ["Ashish Vaswani", "Noam Shazeer", "Niki Parmar"];

fn attention(ids: &[&str], year: i16) -> SourceRecord {
    record(ids, "Attention Is All You Need", &VASWANI, year)
}

#[test]
fn the_real_paper_verifies_and_the_lookalike_does_not() {
    let claim = Claim::from_text("Vaswani 2017 Attention is all you need");
    let real = assess(&claim, &attention(&["arXiv:1706.03762"], 2017));
    let lookalike = assess(&claim, &attention(&["10.65215/r5bs2d54"], 2025));

    assert!(real.confidence >= VERIFIED, "{real:?}");
    assert_eq!(real.title.agreement, Agreement::Match);
    assert_eq!(lookalike.year.agreement, Agreement::Mismatch);
    assert!(lookalike.confidence < PLAUSIBLE, "{lookalike:?}");
}

#[test]
fn a_messy_reference_matches_its_work_not_its_namesakes() {
    let claim =
        Claim::from_text("LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015;521:436-444.");
    let authors = ["Yann LeCun", "Yoshua Bengio", "Geoffrey Hinton"];
    let right = assess(&claim, &lecun());
    let later = assess(
        &claim,
        &record(&["10.1145/3448250"], "Deep learning for AI", &authors, 2021),
    );
    assert!(right.confidence >= VERIFIED, "{right:?}");
    assert!(later.confidence < PLAUSIBLE, "{later:?}");
}

#[test]
fn structured_claims_compare_field_by_field() {
    let mut claim = Claim {
        title: Some("Attention is all you need".into()),
        authors: vec!["Vaswani, A.".into(), "N. Shazeer".into()],
        year: Some(2017),
        ..Claim::default()
    };
    let a = assess(&claim, &attention(&[], 2017));
    assert_eq!(a.authors.agreement, Agreement::Match);
    assert!(a.confidence >= VERIFIED);

    claim.year = Some(2018);
    let off_by_one = assess(&claim, &attention(&[], 2017));
    assert_eq!(off_by_one.year.agreement, Agreement::Partial);
    assert!(off_by_one.confidence >= PLAUSIBLE);
}

#[test]
fn first_author_alone_matches_but_a_missing_first_author_does_not() {
    let families = [
        "vaswani".to_owned(),
        "shazeer".to_owned(),
        "parmar".to_owned(),
    ];
    let etal = Normalised::new("Vaswani et al. Attention is all you need");
    let others = Normalised::new("Shazeer N, Parmar N. Attention is all you need");
    assert_eq!(
        FieldCheck::scored(etal.author_coverage(&families)).agreement,
        Agreement::Match
    );
    assert_eq!(
        FieldCheck::scored(others.author_coverage(&families)).agreement,
        Agreement::Partial
    );
    let sole = ["anfinsen".to_owned()];
    assert_eq!(
        etal.author_coverage(&sole),
        0.0,
        "an unmentioned sole author"
    );
}

#[test]
fn any_edition_year_counts_for_books() {
    let mut first_edition = record(
        &[],
        "Reinforcement Learning: An Introduction",
        &["Richard S. Sutton", "Andrew G. Barto"],
        1998,
    );
    let claim = Claim::from_text(
        "Sutton RS, Barto AG. Reinforcement Learning: An Introduction. 2nd ed. MIT Press; 2018.",
    );
    assert_eq!(
        assess(&claim, &first_edition).year.agreement,
        Agreement::Mismatch
    );
    first_edition.work.other_years = vec![2018];
    let a = assess(&claim, &first_edition);
    assert_eq!(a.year.agreement, Agreement::Match);
    assert!(a.confidence >= VERIFIED, "{a:?}");
}

#[test]
fn different_authors_are_penalised() {
    let claim =
        Claim::from_text("Sutton RS, Barto AG. Reinforcement Learning: An Introduction. 2018.");
    let other = record(
        &[],
        "An Introduction to Deep Reinforcement Learning",
        &["Vincent Francois-Lavet", "Peter Henderson"],
        2018,
    );
    let a = assess(&claim, &other);
    assert_eq!(a.authors.agreement, Agreement::Mismatch);
    assert!(a.confidence < PLAUSIBLE, "{a:?}");
}

#[test]
fn nearer_years_score_higher_even_when_both_are_wrong() {
    let claim = Claim::from_text("Vaswani A, Shazeer N. Attention is all you need. NeurIPS 2019.");
    let real = assess(&claim, &attention(&[], 2017));
    let lookalike = assess(&claim, &attention(&[], 2025));
    assert_eq!(real.year.agreement, Agreement::Mismatch);
    assert!(real.confidence > lookalike.confidence);
}

#[test]
fn a_title_alone_cannot_verify() {
    let claim = Claim {
        title: Some("Deep learning".into()),
        ..Claim::default()
    };
    let a = assess(&claim, &record(&[], "Deep learning", &[], 2015));
    assert_eq!(a.title.agreement, Agreement::Match);
    assert!(a.confidence <= SINGLE_FIELD_CAP);
}

#[test]
fn accents_and_punctuation_do_not_matter() {
    assert_eq!(
        normalise("Schölkopf, B. — Kernel‐Methods!"),
        "scholkopf b kernel methods"
    );
    assert_eq!(
        title_similarity("Attention Is All You Need", "attention is all you need."),
        1.0
    );
    let partial = title_similarity(
        "Attention Is All You Need",
        "Attention Is All You Need In Speech Separation",
    );
    assert!((0.5..0.9).contains(&partial), "{partial}");
}

#[test]
fn claimed_ids_are_checked_against_the_record() {
    let claim = Claim::from_text("Deep learning. Nature 2015. doi:10.1038/nature14539.");
    assert_eq!(
        claim.ids,
        [WorkId::parse_as(crate::IdKind::Doi, "10.1038/nature14539").unwrap()]
    );
    let right = assess(
        &claim,
        &record(&["10.1038/nature14539"], "Deep learning", &[], 2015),
    );
    let wrong = assess(
        &claim,
        &record(&["10.1145/3448250"], "Deep learning for AI", &[], 2021),
    );
    assert_eq!(right.ids, Agreement::Match);
    assert_eq!(wrong.ids, Agreement::Mismatch);
}

#[test]
fn input_is_an_identifier_or_a_reference() {
    let id_only = Claim::from_input(" 10.1038/nature14539 ");
    assert!(id_only.text.is_none() && id_only.ids.len() == 1);
    let reference = Claim::from_input("Deep learning. Nature 2015. doi:10.1038/nature14539");
    assert!(reference.text.is_some() && reference.ids.len() == 1);
    // A link a copy spaced out is still the whole input.
    let spaced = Claim::from_input("https:// doi. org/ 10. 1183/ 23120 541. 00489- 2021");
    assert!(spaced.text.is_none(), "{spaced:?}");
    assert_eq!(spaced.ids[0].to_string(), "doi:10.1183/23120541.00489-2021");
}

#[test]
fn shared_title_finds_the_cited_title_inside_a_derivative_one() {
    let reference = "Jumper J, et al. Highly accurate protein structure prediction with AlphaFold. Nature. 2021.";
    assert_eq!(
        shared_title(
            "Faculty Opinions recommendation of Highly accurate protein structure prediction with AlphaFold.",
            reference
        )
        .as_deref(),
        Some("highly accurate protein structure prediction with alphafold")
    );
    assert_eq!(
        shared_title("Deep learning", "LeCun Y. Deep learning. Nature 2015").as_deref(),
        Some("deep learning"),
        "a short title counts when whole"
    );
    assert_eq!(
        shared_title("Deep learning for AI", "Smith J. On learning. 2020"),
        None
    );
}

#[test]
fn retraction_markers_do_not_hide_a_title_match() {
    assert_eq!(
        unmarked("RETRACTED: Ileal-lymphoid hyperplasia"),
        "Ileal-lymphoid hyperplasia"
    );
    assert_eq!(unmarked("Retracted Article - Something"), "Something");
    assert_eq!(unmarked("Retraction: Something"), "Retraction: Something");
    assert_eq!(
        unmarked("Retracted proteins in yeast"),
        "Retracted proteins in yeast"
    );
    assert_eq!(
        title_similarity(
            "RETRACTED: Ileal-lymphoid hyperplasia",
            "Ileal-lymphoid hyperplasia"
        ),
        1.0
    );
}

/// The PEITHO trial as published: NEJM 2014;370(15):1402-11.
fn peitho() -> SourceRecord {
    let mut r = record(
        &["10.1056/nejmoa1302097"],
        "Fibrinolysis for Patients with Intermediate-Risk Pulmonary Embolism",
        &["Guy Meyer", "Eric Vicaut"],
        2014,
    );
    r.work.container_title = Some("New England Journal of Medicine".into());
    r.work.volume = Some("370".into());
    r.work.page = Some("1402-1411".into());
    r
}

#[test]
fn journal_volume_and_page_are_checked_in_text_and_fields() {
    let text = "Meyer G, et al. Fibrinolysis for patients with intermediate-risk pulmonary embolism. N Engl J Med. 2014;370(15):1402-11.";
    let a = assess(&Claim::from_text(text), &peitho());
    assert_eq!(
        (a.container.agreement, a.volume.agreement, a.page.agreement),
        (Agreement::Match, Agreement::Match, Agreement::Match)
    );
    assert!(a.confidence >= VERIFIED, "{a:?}");

    // A wrong first page: a real error, and what tells the article from
    // a reply printed later in the same volume.
    let wrong_page = text.replace("1402-11", "2440");
    let a = assess(&Claim::from_text(wrong_page), &peitho());
    assert_eq!(a.page.agreement, Agreement::Mismatch);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");

    // Structured: the journal is compared by abbreviation rules. A record
    // in a journal, its type not given (Europe PMC's editorials and
    // letters; holdout-1 #478, #492), is held to it as an article is: the
    // wrong journal is plausible. For a work in proceedings, a different
    // name lowers the ranking without settling the verdict.
    let claim = Claim {
        title: Some("Fibrinolysis for patients with intermediate-risk pulmonary embolism".into()),
        authors: vec!["Meyer G".into()],
        year: Some(2014),
        container: Some("Lancet".into()),
        ..Claim::default()
    };
    let a = assess(&claim, &peitho());
    assert_eq!(a.container.agreement, Agreement::Mismatch);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    let mut proceedings = peitho();
    proceedings.work.work_type = Some(crate::work::WorkType::ConferencePaper);
    let a = assess(&claim, &proceedings);
    assert_eq!(a.container.agreement, Agreement::Mismatch);
    assert!(a.confidence >= VERIFIED && a.confidence < 1.0, "{a:?}");
}

#[test]
fn titleless_references_that_give_journal_volume_and_page_verify() {
    let a = assess(
        &Claim::from_text("Meyer G et al. N Engl J Med 2014;370:1402"),
        &peitho(),
    );
    assert_eq!(a.title.agreement, Agreement::Unknown);
    assert!(a.confidence >= VERIFIED, "{a:?}");
    // The right paper with a wrong page: plausible, not missing.
    let a = assess(
        &Claim::from_text("Meyer G et al. N Engl J Med 2014;370:2440"),
        &peitho(),
    );
    assert_eq!(a.page.agreement, Agreement::Mismatch);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    // Without the journal agreeing, the missing title still counts.
    let a = assess(
        &Claim::from_text("Meyer G et al. Lancet 2014;370:1402"),
        &peitho(),
    );
    assert_eq!(a.title.agreement, Agreement::Mismatch);
    assert!(a.confidence < PLAUSIBLE, "{a:?}");
}

#[test]
fn citing_the_exact_place_corroborates_like_authors() {
    // No authors, but title, year, volume and first page all agree.
    let claim = Claim {
        title: Some("Fibrinolysis for patients with intermediate-risk pulmonary embolism".into()),
        year: Some(2014),
        volume: Some("370".into()),
        page: Some("1402".into()),
        ..Claim::default()
    };
    assert!(assess(&claim, &peitho()).confidence >= VERIFIED);
    let claim = Claim {
        volume: None,
        page: None,
        ..claim
    };
    assert!(assess(&claim, &peitho()).confidence < VERIFIED);
}

/// A record with where it appeared.
fn placed(
    ids: &[&str],
    title: &str,
    authors: &[&str],
    year: i16,
    at: (&str, &str, &str),
) -> SourceRecord {
    let mut r = record(ids, title, authors, year);
    r.work.container_title = Some(at.0.into());
    r.work.volume = Some(at.1.into());
    r.work.page = Some(at.2.into());
    r
}

#[test]
fn journal_volume_and_first_page_name_a_work_without_a_title() {
    // #23: running headers and short forms.
    let laan = placed(
        &["doi:10.1016/j.injury.2015.11.045"],
        "Chest wall thickness and decompression failure: A systematic review and meta-analysis",
        &["Laan", "Vu", "Thiels"],
        2016,
        ("Injury", "47", "797-804"),
    );
    for text in [
        "Injury, Int. J. Care Injured 47 (2016) 797\u{2013}804",
        "Injury. 2016;47(4):797-804.",
        "Laan DV, et al. Injury 2016;47:797.",
    ] {
        let a = assess(&Claim::from_text(text), &laan);
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
    let lecun = placed(
        &[],
        "Deep learning",
        &["LeCun", "Bengio", "Hinton"],
        2015,
        ("Nature", "521", "436-444"),
    );
    let a = assess(
        &Claim::from_text("Nature 521, 436\u{2013}444 (2015)"),
        &lecun,
    );
    assert!(a.confidence >= VERIFIED, "{a:?}");
    // Online first, in print the next year: plausible.
    let a = assess(&Claim::from_text("Injury. 2017;47(4):797-804."), &laan);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    // Another page, or other authors: not this work.
    for text in [
        "Injury, Int. J. Care Injured 47 (2016) 805\u{2013}812",
        "Smith J, et al. Injury 2016;47:797.",
    ] {
        let a = assess(&Claim::from_text(text), &laan);
        assert!(a.confidence < PLAUSIBLE, "{text}: {a:?}");
    }
}

#[test]
fn a_reference_without_a_title_is_not_a_different_title() {
    // Dogfood (#33): authors, journal, year and the DOI, no title.
    let doi = "doi:10.1183/23120541.00489-2021";
    let work = placed(
        &[doi],
        "Community-acquired pneumonia subgroups and differential response to corticosteroids",
        &["Wittermans", "Vestjens", "Blum"],
        2021,
        ("ERJ Open Research", "8", "00489-2021"),
    );
    let claim = Claim::from_text(format!("Wittermans E, et al. ERJ Open Res. 2022. {doi}"));
    let a = assess(&claim, &work);
    assert_eq!(a.title.agreement, Agreement::Unknown, "{a:?}");
    assert!(a.confidence >= PLAUSIBLE, "{a:?}");
    // Without an identifier or a place, authors, journal and year fit too
    // many works.
    let a = assess(
        &Claim::from_text("Wittermans E, et al. ERJ Open Res. 2021."),
        &work,
    );
    assert!(a.confidence < PLAUSIBLE, "{a:?}");
    // A reference that does give a title still disagrees.
    let claim = Claim::from_text(format!(
        "Wittermans E, et al. Steroids in asthma. ERJ Open Res. 2021. {doi}"
    ));
    assert_eq!(assess(&claim, &work).title.agreement, Agreement::Mismatch);
}

#[test]
fn a_url_is_not_a_title() {
    // Dogfood: a Crossref placeholder titled with a notice page's URL,
    // against a citation whose DOI link Preview spaced out.
    let claim =
        Claim::from_text("Eur Respir J. 2021. https:// doi. org/ 10. 1183/ 23120 541. 00489- 2021");
    let placeholder = record(
        &[],
        "https://www.ejhbe.com/notification-about-the-doi",
        &[],
        2021,
    );
    let a = assess(&claim, &placeholder);
    assert_eq!(a.title.agreement, Agreement::Unknown, "{a:?}");
    assert!(a.confidence < PLAUSIBLE, "{a:?}");
    // Nor are a link's words a title's.
    let notice = record(&[], "DOI notice", &[], 2021);
    let a = assess(&claim, &notice);
    assert_ne!(a.title.agreement, Agreement::Match, "{a:?}");
}

#[test]
fn a_short_title_inside_text_is_not_enough() {
    // Dogfood: a one-word encyclopedia entry inside a chat message.
    let entry = record(&[], "Aspero", &[], 2012);
    for text in ["aspero 0.6.5 is in", "Aspero 2012"] {
        let a = assess(&Claim::from_text(text), &entry);
        assert_eq!(a.title.agreement, Agreement::Match);
        assert!(a.confidence < PLAUSIBLE, "{text}: {a:?}");
    }
    // With the authors, a short title is a match.
    let deep = record(&[], "Deep learning", &["LeCun", "Bengio", "Hinton"], 2015);
    let a = assess(
        &Claim::from_text("LeCun Y, Bengio Y, Hinton G. Deep learning. 2015"),
        &deep,
    );
    assert!(a.confidence >= PLAUSIBLE, "{a:?}");
    // As the claim's title, it is what was asked for.
    let claim = Claim {
        title: Some("Aspero".into()),
        ..Claim::default()
    };
    assert!(assess(&claim, &entry).confidence >= PLAUSIBLE);
}

#[test]
fn title_and_year_alone_cannot_verify() {
    // Dogfood: a generic title contained in the reference, no authors.
    let claim = Claim::from_text(
        "3CPO (NEJM 2008; Health Technol Assess 2009). New England Journal of Medicine; Health Technology Assessment 2009 2008",
    );
    let a = assess(
        &claim,
        &record(&[], "Health technology assessment", &[], 2009),
    );
    assert_eq!(a.title.agreement, Agreement::Match);
    assert!(a.confidence < VERIFIED, "{a:?}");

    // An unauthored letter with the same title loses to the article.
    let claim = Claim {
        text: Some("Fibrinolysis for patients with intermediate-risk pulmonary embolism".into()),
        authors: vec!["Meyer".into()],
        year: Some(2014),
        ..Claim::default()
    };
    let letter = assess(
        &claim,
        &record(
            &[],
            "Fibrinolysis for Intermediate-Risk Pulmonary Embolism",
            &[],
            2014,
        ),
    );
    let article = assess(
        &claim,
        &record(
            &[],
            "Fibrinolysis for Patients with Intermediate-Risk Pulmonary Embolism",
            &["Guy Meyer"],
            2014,
        ),
    );
    assert!(letter.confidence < VERIFIED && article.confidence >= VERIFIED);
    // A title in capitals names no one ("WOMEN ON ORAL" isn't a family
    // name and initials; holdout-1 #382): title and year, plausible.
    let title = "THE α-FETOPROTEIN TEST IN PREGNANT WOMEN, WOMEN ON ORAL CONTRACEPTIVES, NEWBORN \
                 BABIES, AND PYRIDOXINE-DEPRIVED BABOONS";
    let a = assess(
        &Claim::from_text(format!("“{title}”, 1970.")),
        &record(&[], title, &["J. M. Foy", "C. R. Wood"], 1970),
    );
    assert_eq!(a.authors.agreement, Agreement::Unknown, "{a:?}");
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    // Capitals with initials as references write them still name people.
    assert!(authors::names_given("SMITH JA, JONES B. A title. 2020."));
    assert!(authors::names_given("DUPONT, J. Un titre. 2020."));
}

#[test]
fn replies_and_corrections_are_not_the_original() {
    assert!(is_derivative(
        "Age-Adjusted D-Dimer Cutoff Levels and Pulmonary Embolism—Reply"
    ));
    assert!(is_derivative("Correction to: Attention is all you need"));
    assert!(is_derivative(
        "Comment on “Fibrinolysis for patients with intermediate-risk PE”"
    ));
    assert!(!is_derivative(
        "Age-Adjusted D-Dimer Cutoff Levels to Rule Out Pulmonary Embolism"
    ));
    assert!(is_derivative("Correction: Deep learning"));
    assert!(is_derivative("In response to: Fibrinolysis for PE"));
    // The words inside a title are not markers.
    for title in [
        "Community-acquired pneumonia subgroups and differential response to corticosteroids",
        "Correction of hyponatraemia: a systematic review",
        "Rapid correction of acidosis",
    ] {
        assert!(!is_derivative(title), "{title}");
    }

    let claim = Claim {
        title: Some("Age-adjusted D-dimer cutoff levels to rule out pulmonary embolism".into()),
        authors: vec!["Righini".into()],
        year: Some(2014),
        ..Claim::default()
    };
    let reply = assess(
        &claim,
        &record(
            &[],
            "Age-Adjusted D-Dimer Cutoff Levels and Pulmonary Embolism—Reply",
            &["Marc Righini"],
            2014,
        ),
    );
    assert!(reply.confidence < PLAUSIBLE, "{reply:?}");
    // Citing the reply itself is fine.
    let citing_reply = Claim {
        title: Some("Age-adjusted D-dimer cutoff levels and pulmonary embolism—reply".into()),
        ..claim
    };
    let a = assess(
        &citing_reply,
        &record(
            &[],
            "Age-Adjusted D-Dimer Cutoff Levels and Pulmonary Embolism—Reply",
            &["Marc Righini"],
            2014,
        ),
    );
    assert!(a.confidence >= VERIFIED, "{a:?}");
    // Nor is the original a correction cited (holdout-1 #185, corpus
    // #105): the correction's record may be missing, but the original
    // isn't verified in its place, its title and place quoted inside.
    let cloning = placed(
        &[],
        "Cloning of a cGMP-gated cation channel from mouse kidney inner medullary collecting \
         duct",
        &["K H Karlson", "F Ciampolillo-Bates", "D E McCoy"],
        1995,
        ("Biochimica et Biophysica Acta", "1236", "197-200"),
    );
    let corrigendum = "Karlson KH, Ciampolillo-Bates F, McCoy DE. Corrigendum to \"Cloning of a \
                       cGMP-gated cation channel from mouse kidney inner medullary collecting \
                       duct\" [Biochim. Biophys. Acta 1236 (1995) 197-200]. Biochim Biophys Acta. \
                       1995;1238(2):197.";
    let a = assess(&Claim::from_text(corrigendum), &cloning);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    let shamloul = placed(
        &[],
        "Complementary and Alternative Medicine (CAM) for Sexual Dysfunction",
        &["Rany Shamloul", "Anthony J. Bella"],
        2014,
        ("The Journal of Sexual Medicine", "11", "1097-1098"),
    );
    let corrections = "Corrections to “Shamloul R, Bella AJ. Complementary and alternative \
                       medicine (CAM) for sexual dysfunction. J Sex Med 2014;11:1097–8”. J Sexual \
                       Med. 2014;11:2126.";
    let a = assess(&Claim::from_text(corrections), &shamloul);
    assert!(a.confidence < VERIFIED, "{a:?}");
    // The correction's own record verifies.
    let mut notice = shamloul.clone();
    notice.work.title = Some(
        "Corrections to “Complementary and Alternative Medicine (CAM) for Sexual Dysfunction”"
            .into(),
    );
    notice.work.page = None;
    let a = assess(&Claim::from_text(corrections), &notice);
    assert!(a.confidence >= VERIFIED, "{a:?}");
}

#[test]
fn a_few_shared_topic_words_are_not_a_title_match() {
    // Dogfood control: a fabricated citation sharing "septic shock".
    let claim = Claim::from_text("Smith 2019 Ketamine reduces mortality in septic shock");
    let a = assess(
        &claim,
        &record(
            &[],
            "Leukotrienes in Endotoxic and Septic Shock",
            &["J Smith"],
            2019,
        ),
    );
    assert_eq!(a.title.agreement, Agreement::Mismatch);
    assert!(a.confidence < PLAUSIBLE, "{a:?}");
}

#[test]
fn dropped_subtitles_still_match() {
    let structured = title_similarity(
        "Age-adjusted D-dimer cutoff levels to rule out pulmonary embolism",
        "Age-Adjusted D-Dimer Cutoff Levels to Rule Out Pulmonary Embolism: The ADJUST-PE Study",
    );
    assert_eq!(structured, 1.0);
    let claim = Claim::from_text(
        "Righini M et al. Age-adjusted D-dimer cutoff levels to rule out pulmonary embolism. JAMA 2014",
    );
    let a = assess(
        &claim,
        &record(
            &[],
            "Age-Adjusted D-Dimer Cutoff Levels to Rule Out Pulmonary Embolism: The ADJUST-PE Study",
            &["Marc Righini"],
            2014,
        ),
    );
    assert_eq!(a.title.agreement, Agreement::Match);
}

#[test]
fn search_terms_drop_noise() {
    assert_eq!(
        search_terms("LeCun Y, Bengio Y, et al. Deep learning. Nature. 2015;521:436-444."),
        "lecun bengio deep learning nature"
    );
}

#[test]
fn the_probable_title_is_read_from_a_references_shape() {
    // Red team (#102): no matcher hit, so no title was searched.
    for (text, title) in [
        (
            "Radford A, Kim JW, Hallacy C, et al. Learning transferable visual models from \
             natural language supervision. In: Proceedings of the 38th International Conference \
             on Machine Learning (ICML). PMLR; 2021:8748-8763.",
            "Learning transferable visual models from natural language supervision",
        ),
        (
            "Dosovitskiy, A. et al. (2021) 'An image is worth 16x16 words: Transformers for image \
             recognition at scale', International Conference on Learning Representations (ICLR).",
            "An image is worth 16x16 words: Transformers for image recognition at scale",
        ),
        (
            "Lundberg SM, Lee SI. A unified approach to interpreting model predictions. Adv Neural \
             Inf Process Syst. 2017;30.",
            "A unified approach to interpreting model predictions",
        ),
        (
            "Said EW. Orientalism. New York: Pantheon Books; 1978.",
            "Orientalism",
        ),
        (
            "Pillow, K. (2003) 'Did Goodman's Distinction Survive LeWitt?', Journal of Aesthetics.",
            "Did Goodman's Distinction Survive LeWitt?",
        ),
        (
            "E. Dijkstra, \"A note on two problems in connexion with graphs,\" Numerische \
             Mathematik, vol. 1, 1959.",
            "A note on two problems in connexion with graphs",
        ),
        (
            "@article{d, author = {Doe, J}, title = {Sepsis in {mice}}, year = {2020}}",
            "Sepsis in mice",
        ),
    ] {
        assert_eq!(probable_title(text).as_deref(), Some(title), "{text}");
    }
    assert_eq!(probable_title("2020. 12"), None);
}

#[test]
fn finds_ids_in_running_text() {
    let text = "See (https://doi.org/10.1145/3368089.3409741), arXiv:1706.03762v5 and \
                https://arxiv.org/abs/hep-th/9901001; also arXiv:1706.03762.";
    let ids: Vec<String> = find_ids(text).iter().map(ToString::to_string).collect();
    assert_eq!(
        ids,
        [
            "doi:10.1145/3368089.3409741",
            "arxiv:1706.03762v5",
            "arxiv:hep-th/9901001"
        ]
    );
    let book = "Goodfellow I, et al. Deep Learning. MIT Press; 2016. ISBN 978-0-262-03561-3. Also ISBN-10: 0262035618.";
    let ids: Vec<String> = find_ids(book).iter().map(ToString::to_string).collect();
    assert_eq!(ids, ["isbn:9780262035613"], "both forms are one book");
    assert!(
        find_ids("pages 9780262035613 and 2016").is_empty(),
        "unlabelled digits ignored"
    );
    assert!(find_ids("Published in 2017, pages 10-20.").is_empty());
}

#[test]
fn a_claims_place_takes_its_fields_then_its_text() {
    let claim = Claim {
        volume: Some("47".into()),
        text: Some("Injury 47 (2016) 797–804".into()),
        ..Claim::default()
    };
    let place = claim.place().expect("placed");
    assert_eq!(
        (place.volume.as_str(), place.first_page.as_str()),
        ("47", "797")
    );
    let read = claim.as_read();
    assert_eq!(read.page.as_deref(), Some("797"));
}

/// #52: the ATLS manual, 9th edition (2012), cited by its organisation.
const ATLS: &str = "American College of Surgeons. Advanced trauma life support student \
                    course manual. 9th ed. Chicago, IL: American College of Surgeons; 2012.";

#[test]
fn a_periodicals_title_inside_the_author_is_no_match() {
    let mut journal = record(
        &[],
        "Journal of the American College of Surgeons.",
        &[],
        2000,
    );
    journal.work.issued = None;
    let a = assess(&Claim::from_text(ATLS), &journal);
    assert!(a.confidence < PLAUSIBLE, "{a:?}");
}

#[test]
fn another_edition_of_a_cited_book_is_plausible() {
    let mut book = record(
        &["isbn:9781880696514"],
        "Advanced Trauma Life Support®",
        &["American College of Surgeons Committee on Trauma"],
        2025,
    );
    book.work.work_type = Some(crate::work::WorkType::Book);
    let a = assess(&Claim::from_text(ATLS), &book);
    assert!(a.confidence >= PLAUSIBLE, "{a:?}");
    assert_eq!(a.year.agreement, Agreement::Mismatch);

    // Not for articles: a different year there is more often another work.
    book.work.work_type = Some(crate::work::WorkType::JournalArticle);
    assert!(assess(&Claim::from_text(ATLS), &book).confidence < PLAUSIBLE);

    // Nor lowered again after: a structured claim giving the publisher
    // as its container, for a book in a series.
    let mut book = record(
        &[],
        "Reinforcement Learning: An Introduction",
        &["Richard S. Sutton", "Andrew G. Barto"],
        1998,
    );
    book.work.work_type = Some(crate::work::WorkType::Book);
    book.work.container_title = Some("Adaptive Computation and Machine Learning".into());
    let claim = Claim {
        title: Some("Reinforcement Learning: An Introduction".into()),
        authors: vec!["Sutton RS".into(), "Barto AG".into()],
        year: Some(2018),
        container: Some("MIT Press".into()),
        ..Claim::default()
    };
    let a = assess(&claim, &book);
    assert!(a.confidence >= PLAUSIBLE, "{a:?}");
}

#[test]
fn a_chapter_is_not_another_edition_of_its_book() {
    // The book's editor is not the chapter's author.
    let mut book = record(&[], "Handbook of Sepsis", &["K. Jones"], 2018);
    book.work.work_type = Some(crate::work::WorkType::Book);
    for text in [
        "Smith J. Fluids in shock. In: Jones K, ed. Handbook of Sepsis. Springer; 2010:12-30.",
        "Smith, J. (2010). Fluids in shock. In K. Jones (Ed.), Handbook of sepsis (pp. 12-30). Springer.",
    ] {
        let a = assess(&Claim::from_text(text), &book);
        assert!(a.confidence < PLAUSIBLE, "{text}: {a:?}");
    }
}

#[test]
fn a_preprint_and_its_published_version_are_one_work() {
    // A shared arXiv ID: the published record lists its arXiv copy.
    let preprint = attention(
        &["W1", "arXiv:1706.03762", "10.48550/arXiv.1706.03762"],
        2017,
    );
    let published = attention(&["W2", "10.5555/neurips.2017", "arXiv:1706.03762"], 2017);
    assert!(same_work(&preprint, &published));

    // No shared identifier: the same title, first author and near years.
    let biorxiv = record(
        &["W3", "10.1101/2020.01.01.1"],
        "Sepsis in mice",
        &["Jane Doe"],
        2020,
    );
    let journal = record(
        &["W4", "10.1038/x.2021"],
        "Sepsis in Mice",
        &["J. Doe", "A. N. Other"],
        2021,
    );
    assert!(same_work(&biorxiv, &journal));

    // Years two apart, another first author, or a reply: not the work.
    let later = record(&["W5"], "Sepsis in mice", &["Jane Doe"], 2022);
    assert!(!same_work(&biorxiv, &later));
    let other = record(&["W6"], "Sepsis in mice", &["John Smith"], 2020);
    assert!(!same_work(&biorxiv, &other));
    let reply = record(&["W7"], "Reply to: Sepsis in mice", &["Jane Doe"], 2020);
    assert!(!same_work(&biorxiv, &reply));
}

/// #80: Ragusa et al., Plasticenta, in Environment International.
fn plasticenta() -> SourceRecord {
    let mut r = placed(
        &["10.1016/j.envint.2020.106274"],
        "Plasticenta: First evidence of microplastics in human placenta",
        &[
            "Antonio Ragusa",
            "Alessandro Svelato",
            "Criselda Santacroce",
        ],
        2021,
        ("Environment International", "146", "106274"),
    );
    r.work.work_type = Some(crate::work::WorkType::JournalArticle);
    r
}

const PLASTICENTA_VANCOUVER: &str = "Ragusa A, Svelato A, Santacroce C, et al. Plasticenta: \
    first evidence of microplastics in human placenta. Environ Int. 2021;146:106274.";
const PLASTICENTA_APA: &str = "Ragusa, A., Svelato, A., & Santacroce, C. (2021). Plasticenta: \
    First evidence of microplastics in human placenta. Environment International, 146, 106274.";

#[test]
fn a_reference_naming_another_journal_is_not_verified() {
    for (right, wrong) in [
        (PLASTICENTA_VANCOUVER, "Environ Sci Technol"),
        (PLASTICENTA_APA, "Environmental Science & Technology"),
    ] {
        let a = assess(&Claim::from_text(right), &plasticenta());
        assert_eq!(
            (a.container.agreement, a.volume.agreement, a.page.agreement),
            (Agreement::Match, Agreement::Match, Agreement::Match),
            "{right}"
        );
        assert!(a.confidence >= VERIFIED, "{right}: {a:?}");

        let text = right
            .replace("Environ Int", wrong)
            .replace("Environment International", wrong);
        let claim = Claim::from_text(&text);
        let a = assess(&claim, &plasticenta());
        assert_eq!(a.container.agreement, Agreement::Mismatch, "{text}");
        assert!(
            (PLAUSIBLE..VERIFIED).contains(&a.confidence),
            "{text}: {a:?}"
        );
        // The journal as read, for "cited in X, found in Y".
        assert_eq!(claim.as_read().container.as_deref(), Some(wrong));
    }
    // A structured claim naming another journal: the same.
    let claim = Claim {
        title: Some("Plasticenta: first evidence of microplastics in human placenta".into()),
        authors: vec!["Ragusa A".into()],
        year: Some(2021),
        container: Some("Environ Sci Technol".into()),
        ..Claim::default()
    };
    let a = assess(&claim, &plasticenta());
    assert_eq!(a.container.agreement, Agreement::Mismatch);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");

    // Its preprint, sharing the main title, is another version of a work
    // cited by journal, volume and page: plausible at most (live, it
    // outranked the capped article).
    let mut preprint = record(
        &["10.1101/2020.07.15.198325"],
        "Plasticenta: Microplastics in Human Placenta",
        &[
            "Antonio Ragusa",
            "Alessandro Svelato",
            "Criselda Santacroce",
        ],
        2020,
    );
    preprint.work.work_type = Some(crate::work::WorkType::Preprint);
    for text in [
        PLASTICENTA_VANCOUVER.to_owned(),
        PLASTICENTA_VANCOUVER.replace("Environ Int", "Environ Sci Technol"),
    ] {
        let a = assess(&Claim::from_text(&text), &preprint);
        assert_eq!(a.container.agreement, Agreement::Mismatch, "{text}");
        assert!(a.confidence < VERIFIED, "{text}: {a:?}");
    }
    // Cited without a journal and place, it may be the preprint.
    let text =
        "Ragusa A, Svelato A, Santacroce C. Plasticenta: microplastics in human placenta. 2020.";
    let a = assess(&Claim::from_text(text), &preprint);
    assert_eq!(a.container.agreement, Agreement::Unknown);
    assert!(a.confidence >= VERIFIED, "{a:?}");
}

#[test]
fn journals_abbreviated_with_words_left_out_are_no_mismatch() {
    let mut article = placed(
        &[],
        "Fluid resuscitation in experimental sepsis",
        &["Jane Doe", "Alan Roe"],
        2010,
        (
            "Proceedings of the Royal Society B: Biological Sciences",
            "277",
            "100-110",
        ),
    );
    article.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let text = "Doe J, Roe A. Fluid resuscitation in experimental sepsis. Proc Biol Sci. \
                2010;277:100-10.";
    let a = assess(&Claim::from_text(text), &article);
    assert_ne!(a.container.agreement, Agreement::Mismatch, "{a:?}");
    assert!(a.confidence >= VERIFIED, "{a:?}");
}

#[test]
fn a_journal_read_from_the_reference_outweighs_words_elsewhere_in_it() {
    // "SR" among the authors' initials is not Scientific Reports, nor
    // "mice" in the title Medicine, when the reference names another.
    for (text, title, author, year, at) in [
        (
            "Ragusa A, Rossi SR. Microplastics in human placenta. Nature. 2021;11:1234.",
            "Microplastics in human placenta",
            "Antonio Ragusa",
            2021,
            ("Scientific Reports", "11", "1234"),
        ),
        (
            "Doe J. Sepsis in mice. Lancet. 2020;99:e1-e9.",
            "Sepsis in mice",
            "Jane Doe",
            2020,
            ("Medicine", "99", "e1"),
        ),
    ] {
        let mut r = placed(&[], title, &[author], year, at);
        r.work.work_type = Some(crate::work::WorkType::JournalArticle);
        let a = assess(&Claim::from_text(text), &r);
        assert_eq!(a.container.agreement, Agreement::Mismatch, "{text}");
        assert!(a.confidence < VERIFIED, "{text}: {a:?}");
    }
    // With no journal read, nor do they count as the journal.
    let mut r = placed(
        &[],
        "Sepsis in mice",
        &["Jane Doe"],
        2020,
        ("Medicine", "99", "e1"),
    );
    r.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let a = assess(&Claim::from_text("Doe J. Sepsis in mice. 2020."), &r);
    assert_eq!(a.container.agreement, Agreement::Unknown, "{a:?}");
}

#[test]
fn structured_and_free_text_journals_compare_alike() {
    let pnas = placed(
        &[],
        "Mitochondrial dysfunction in experimental sepsis",
        &["Jane Doe", "Alan Roe"],
        2011,
        (
            "Proceedings of the National Academy of Sciences",
            "108",
            "1000-1005",
        ),
    );
    let journal = "Proc Natl Acad Sci U S A";
    let structured = Claim {
        title: Some("Mitochondrial dysfunction in experimental sepsis".into()),
        authors: vec!["Doe J".into(), "Roe A".into()],
        year: Some(2010),
        container: Some(journal.into()),
        ..Claim::default()
    };
    let text = Claim::from_text(format!(
        "Doe J, Roe A. Mitochondrial dysfunction in experimental sepsis. {journal}. 2010."
    ));
    for claim in [structured, text] {
        let a = assess(&claim, &pnas);
        assert_ne!(a.container.agreement, Agreement::Mismatch, "{claim:?}");
        assert!(a.confidence >= VERIFIED, "{claim:?}: {a:?}");
    }
}

#[test]
fn journals_named_otherwise_or_not_at_all_are_no_mismatch() {
    let mut nejm = peitho();
    nejm.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let title = "Fibrinolysis for patients with intermediate-risk pulmonary embolism";
    for journal in [
        "N Engl J Med",
        "New England Journal of Medicine",
        "The New England Journal of Medicine",
        "NEJM",
    ] {
        let text = format!("Meyer G, Vicaut E, et al. {title}. {journal}. 2014;370(15):1402-11.");
        let a = assess(&Claim::from_text(&text), &nejm);
        assert_eq!(a.container.agreement, Agreement::Match, "{text}");
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
    // "Lancet" for "The Lancet", and back.
    let mut lancet = nejm.clone();
    lancet.work.container_title = Some("The Lancet".into());
    let text = format!("Meyer G, et al. {title}. Lancet. 2014;370:1402-11.");
    let a = assess(&Claim::from_text(&text), &lancet);
    assert_eq!(a.container.agreement, Agreement::Match);
    lancet.work.container_title = Some("Lancet".into());
    let text = format!("Meyer G, et al. {title}. The Lancet, 370(15), 1402–1411.");
    assert_eq!(
        assess(&Claim::from_text(&text), &lancet)
            .container
            .agreement,
        Agreement::Match
    );
    // No journal in the reference.
    let text = format!("Meyer G, et al. {title}. 2014.");
    let a = assess(&Claim::from_text(&text), &nejm);
    assert_eq!(a.container.agreement, Agreement::Unknown);
    assert!(a.confidence >= VERIFIED, "{a:?}");
    // A book with its publisher, in a book series: no journal to differ.
    let mut book = record(
        &["isbn:9780262035613"],
        "Deep Learning",
        &["Ian Goodfellow", "Yoshua Bengio", "Aaron Courville"],
        2016,
    );
    book.work.work_type = Some(crate::work::WorkType::Book);
    book.work.container_title = Some("Adaptive Computation and Machine Learning".into());
    for text in [
        "Goodfellow I, Bengio Y, Courville A. Deep Learning. MIT Press; 2016.",
        "Goodfellow I, Bengio Y, Courville A. Deep Learning. Cambridge, MA: MIT Press; 2016:1-800.",
    ] {
        let a = assess(&Claim::from_text(text), &book);
        assert_eq!(a.container.agreement, Agreement::Unknown, "{text}");
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
    // Not a journal article: proceedings go by many names.
    let mut paper = nejm.clone();
    paper.work.work_type = Some(crate::work::WorkType::ConferencePaper);
    let text = format!("Meyer G, et al. {title}. Lancet. 2014;370:1402-11.");
    let a = assess(&Claim::from_text(&text), &paper);
    assert_eq!(a.container.agreement, Agreement::Unknown);
}

/// Red team (#102): a real work cited with co-authors it doesn't have.
fn egdt() -> SourceRecord {
    let mut r = placed(
        &[],
        "Early Goal-Directed Therapy in the Treatment of Severe Sepsis and Septic Shock",
        &["Emanuel Rivers", "Bryant Nguyen", "Suzanne Havstad"],
        2001,
        ("New England Journal of Medicine", "345", "1368-1377"),
    );
    r.work.work_type = Some(crate::work::WorkType::JournalArticle);
    r
}

const EGDT: &str = "Early goal-directed therapy in the treatment of severe sepsis and septic shock";

#[test]
fn invented_co_authors_are_not_an_author_match() {
    for text in [
        format!("Rivers E, Smith J, Patel R, et al. {EGDT}. N Engl J Med. 2001;345(19):1368-77."),
        format!("Rivers E, Quarshie-Holm B. {EGDT}. 2001."),
        format!(
            "Rivers, E., & Venkatesan-Moll, P. R. (2001). {EGDT}. New England Journal of \
             Medicine, 345(19), 1368–1377."
        ),
        format!(
            "@article{{r, author = {{Rivers, Emanuel and Okafor, Tunde}}, title = {{{EGDT}}}, \
             year = {{2001}}}}"
        ),
    ] {
        let a = assess(&Claim::from_text(&text), &egdt());
        assert_eq!(a.authors.agreement, Agreement::Partial, "{text}");
        assert!(
            (PLAUSIBLE..VERIFIED).contains(&a.confidence),
            "{text}: {a:?}"
        );
    }
    // A co-author with initials of their own is no given name beside the
    // next family name (holdout-1 #425: "Harrowgate, O., & Palmer, H.",
    // Palmer's given name Humphrey).
    let mut burrell = record(
        &[],
        "Analogy: A Study of Qualification and Argument in Theology",
        &["David B. Burrell", "Humphrey Palmer"],
        1976,
    );
    burrell.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let text = "Burrell, D., Harrowgate, O., & Palmer, H. (1976). Analogy: A Study of \
                Qualification and Argument in Theology. The Philosophical Review, 85(1), 107.";
    let a = assess(&Claim::from_text(text), &burrell);
    assert_eq!(a.authors.agreement, Agreement::Partial, "{text}");
    // Structured: one of two claimed authors invented.
    let claim = Claim {
        title: Some("Fibrinolysis for patients with intermediate-risk pulmonary embolism".into()),
        authors: vec!["Meyer, G.".into(), "Smith, J.".into()],
        year: Some(2014),
        ..Claim::default()
    };
    let a = assess(&claim, &peitho());
    assert_eq!(a.authors.agreement, Agreement::Partial);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
}

#[test]
fn a_byline_naming_none_of_the_records_authors_differs() {
    // Holdout-1: names written as IEEE ("F. Eskdale-Mora"), MLA ("Nadler,
    // Steven.") and BibTeX (`author`) do, none the record's: different
    // authors, not none given (#384, #543, #561).
    let baer = placed(
        &[],
        "Nonlinear regression and the solution of simultaneous equations",
        &["Robert M. Baer"],
        1962,
        ("Communications of the ACM", "5", "397-398"),
    );
    let mysticism = placed(
        &[],
        "A Philosophical Study of Mysticism",
        &["Irl Goldwin Whitchurch", "Charles A. Bennett"],
        1924,
        ("The Philosophical Review", "33", "406"),
    );
    let figueroa = placed(
        &[],
        "Maximum Entropy Context Models for Ranking Biographical Answers to Open-Domain \
         Definition Questions",
        &["Alejandro Figueroa", "John Atkinson"],
        2011,
        (
            "Proceedings of the AAAI Conference on Artificial Intelligence",
            "25",
            "1173-1179",
        ),
    );
    for (text, record) in [
        (
            "F. Eskdale-Mora, “Nonlinear regression and the solution of simultaneous \
             equations,” Commun. ACM, vol. 5, no. 7, pp. 397-398, 1962.",
            &baer,
        ),
        (
            "Nadler, Steven. “A Philosophical Study of Mysticism.” The Philosophical Review, \
             vol. 33, no. 4, 1924, p. 406.",
            &mysticism,
        ),
        (
            "@article{li2011, author = {Li, Haoyang and Bai, Li}, title = {Maximum Entropy \
             Context Models for Ranking Biographical Answers to Open-Domain Definition \
             Questions}, journal = {Proceedings of the AAAI Conference on Artificial \
             Intelligence}, year = {2011}, volume = {25}, pages = {1173--1179}}",
            &figueroa,
        ),
    ] {
        let a = assess(&Claim::from_text(text), record);
        assert_eq!(a.authors.agreement, Agreement::Mismatch, "{text}");
        assert!(a.confidence < VERIFIED, "{text}: {a:?}");
    }
    // Their own authors still verify.
    for (text, record) in [
        (
            "R. M. Baer, “Nonlinear regression and the solution of simultaneous equations,” \
             Commun. ACM, vol. 5, no. 7, pp. 397-398, 1962.",
            &baer,
        ),
        (
            "Whitchurch, Irl Goldwin. “A Philosophical Study of Mysticism.” The Philosophical \
             Review, vol. 33, no. 4, 1924, p. 406.",
            &mysticism,
        ),
    ] {
        let a = assess(&Claim::from_text(text), record);
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
}

#[test]
fn the_records_own_authors_however_written_are_no_strangers() {
    for text in [
        format!(
            "Rivers E, Nguyen B, Havstad S, et al. {EGDT}. N Engl J Med. 2001;345(19):1368-77."
        ),
        format!("Rivers E, et al. {EGDT}. N Engl J Med. 2001;345:1368."),
        // Given names written out.
        format!("Emanuel Rivers, Bryant Nguyen and Suzanne Havstad. {EGDT}. 2001."),
        format!(
            "@article{{r, author = {{Rivers, Emanuel and Nguyen, Bryant and others}}, title = \
             {{{EGDT}}}, year = {{2001}}}}"
        ),
        // Transliterated or misspelt by a letter.
        format!("Rivers E, Nguyen B, Havstadt S. {EGDT}. 2001."),
        // Prose: the byline ends where the sentence goes on.
        format!(
            "Rivers et al. described in The New England Journal of Medicine (2001) — \"{EGDT}\"."
        ),
    ] {
        let a = assess(&Claim::from_text(&text), &egdt());
        assert_eq!(a.authors.agreement, Agreement::Match, "{text}");
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
    // Given names the record gives as initials.
    let mut initials = egdt();
    initials.work.authors = vec![Person::new("E", "Rivers"), Person::new("B", "Nguyen")];
    let text = format!("Emanuel Rivers, Bryant Nguyen. {EGDT}. 2001.");
    assert!(assess(&Claim::from_text(&text), &initials).confidence >= VERIFIED);
    // Particles, hyphens, accents.
    let r = record(
        &[],
        "Climatic forcing of vegetation in the late Quaternary",
        &[
            "Anna van der Berg",
            "Jennifer H. Ahn-Jarvis",
            "Bernhard Schölkopf",
        ],
        2003,
    );
    for text in [
        "van der Berg A, Ahn-Jarvis JH, Schoelkopf B. Climatic forcing of vegetation in the late \
         Quaternary. 2003.",
        "A. van der Berg, J. H. Ahn-Jarvis and B. Schölkopf, \"Climatic forcing of vegetation in \
         the late Quaternary,\" 2003.",
    ] {
        let a = assess(&Claim::from_text(text), &r);
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
    let claim = Claim {
        title: Some("Climatic forcing of vegetation in the late Quaternary".into()),
        authors: vec![
            "A. van der Berg".into(),
            "Ahn-Jarvis, J.".into(),
            "others".into(),
        ],
        year: Some(2003),
        ..Claim::default()
    };
    assert!(assess(&claim, &r).confidence >= VERIFIED);
}

#[test]
fn bylines_that_may_name_others_are_not_held_to_the_record() {
    // A consortium cited by name, its members on the record.
    let atlas = record(
        &[],
        "Observation of a new particle in the search for the Standard Model Higgs boson",
        &["Georges Aad", "Tatevik Abajyan", "Brad Abbott"],
        2012,
    );
    let text = "ATLAS Collaboration, Aad G, Smith J. Observation of a new particle in the search \
                for the Standard Model Higgs boson. 2012.";
    let a = assess(&Claim::from_text(text), &atlas);
    assert_eq!(a.authors.agreement, Agreement::Match, "{a:?}");
    // A byline cut short (OpenAlex lists 100): the rest may be cited.
    let mut long = egdt();
    long.work.authors = std::iter::once(Person::new("Emanuel", "Rivers"))
        .chain((0..99).map(|i| Person::new("A", format!("Author{i}"))))
        .collect();
    let text = format!("Rivers E, Smith J. {EGDT}. 2001.");
    assert!(assess(&Claim::from_text(&text), &long).confidence >= VERIFIED);
    // An organisation's byline, cited by its members' names.
    let mut org = egdt();
    org.work.authors = vec![
        Person::new("Emanuel", "Rivers"),
        Person::literal("Early Goal-Directed Therapy Collaborative Group"),
    ];
    assert!(assess(&Claim::from_text(&text), &org).confidence >= VERIFIED);
    // A book's record listing its first author only (Open Library).
    let mut book = record(
        &[],
        "Reinforcement Learning: An Introduction",
        &["Richard S. Sutton"],
        2018,
    );
    book.work.work_type = Some(crate::work::WorkType::Book);
    let claim = Claim {
        title: Some("Reinforcement Learning: An Introduction".into()),
        authors: vec!["Sutton, Richard S.".into(), "Barto, Andrew G.".into()],
        year: Some(2018),
        ..Claim::default()
    };
    assert!(assess(&claim, &book).confidence >= VERIFIED);
    // But people listed beside it are the ones cited.
    let mut listed = egdt();
    listed.work.authors.push(Person::literal(
        "Early Goal-Directed Therapy Collaborative Group",
    ));
    let a = assess(&Claim::from_text(&text), &listed);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
}

fn lecun() -> SourceRecord {
    let mut r = placed(
        &["10.1038/nature14539"],
        "Deep learning",
        &["Yann LeCun", "Yoshua Bengio", "Geoffrey Hinton"],
        2015,
        ("Nature", "521", "436-444"),
    );
    r.work.work_type = Some(crate::work::WorkType::JournalArticle);
    r
}

fn hallmarks() -> SourceRecord {
    let mut r = placed(
        &[],
        "Hallmarks of Cancer: The Next Generation",
        &["Douglas Hanahan", "Robert A. Weinberg"],
        2011,
        ("Cell", "144", "646-674"),
    );
    r.work.work_type = Some(crate::work::WorkType::JournalArticle);
    r
}

#[test]
fn a_title_with_words_added_is_not_the_records() {
    // Red team (#102): another work's title containing this one's, or a
    // subtitle it lacks or doesn't have.
    for (text, record) in [
        (
            "LeCun Y, Bengio Y, Hinton G. Deep learning for medical image analysis. Nature. \
             2015;521:436-444.",
            lecun(),
        ),
        (
            "LeCun Y, Bengio Y, Hinton G. Deep learning: a primer for clinicians. Nature. 2015.",
            lecun(),
        ),
        (
            "Hanahan D, Weinberg RA. Hallmarks of cancer: the next generation of targeted \
             therapies. Cell. 2011;144(5):646-674.",
            hallmarks(),
        ),
        (
            "Hanahan D, Weinberg RA. Hallmarks of cancer: implications for therapy. Cell. 2011.",
            hallmarks(),
        ),
        (
            "Hanahan D, Weinberg RA. Colorectal hallmarks of cancer: the next generation. Cell. \
             2011.",
            hallmarks(),
        ),
        (
            "Hanahan D, Weinberg RA. Hallmarks of cancer: the next generation enables therapy. \
             Cell. 2011.",
            hallmarks(),
        ),
    ] {
        let a = assess(&Claim::from_text(text), &record);
        assert_eq!(a.title.agreement, Agreement::Partial, "{text}");
        assert!(
            (PLAUSIBLE..VERIFIED).contains(&a.confidence),
            "{text}: {a:?}"
        );
    }
    let claim = Claim {
        title: Some("Deep learning: a primer for clinicians".into()),
        authors: vec!["LeCun, Y.".into(), "Bengio, Y.".into()],
        year: Some(2015),
        container: Some("Nature".into()),
        ..Claim::default()
    };
    let a = assess(&claim, &lecun());
    assert_eq!(a.title.agreement, Agreement::Partial);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
}

#[test]
fn a_title_cited_as_is_or_without_its_subtitle_is_the_records() {
    for (text, record) in [
        (
            "LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015;521:436-444.",
            lecun(),
        ),
        (
            "LeCun, Y., Bengio, Y. & Hinton, G. Deep learning. Nature 521, 436–444 (2015).",
            lecun(),
        ),
        (
            "LeCun, Y. et al. (2015) 'Deep learning', Nature, 521, pp. 436-444.",
            lecun(),
        ),
        (
            "Y. LeCun, Y. Bengio, and G. Hinton, \"Deep learning,\" Nature, vol. 521, 2015.",
            lecun(),
        ),
        ("LeCun et al. Deep learning in Nature 2015", lecun()),
        (
            "LeCun and Hinton Deep learning doi:10.1038/nature14539 2015",
            lecun(),
        ),
        (
            "LeCun et al. described in Nature (2015) — \"Deep learning\".",
            lecun(),
        ),
        (
            "Hanahan D, Weinberg RA. Hallmarks of cancer. Cell. 2011;144:646.",
            hallmarks(),
        ),
        (
            "Hanahan D, Weinberg RA. The hallmarks of cancer: the next generation. Cell. 2011.",
            hallmarks(),
        ),
    ] {
        let a = assess(&Claim::from_text(text), &record);
        assert_eq!(a.title.agreement, Agreement::Match, "{text}");
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
    let a = assess(
        &Claim::from_text("Vaswani A, Shazeer N. Attention is all you need NeurIPS 2017"),
        &attention(&[], 2017),
    );
    assert!(a.confidence >= VERIFIED, "{a:?}");
    let claim = Claim {
        title: Some("Hallmarks of cancer".into()),
        authors: vec!["Hanahan, D.".into()],
        year: Some(2011),
        ..Claim::default()
    };
    assert!(assess(&claim, &hallmarks()).confidence >= VERIFIED);
    // A book's record without the subtitle cited (OpenAlex's "Imagined
    // communities"); an article's must have it.
    let mut book = record(&[], "Imagined communities", &["Benedict Anderson"], 1983);
    book.work.work_type = Some(crate::work::WorkType::Book);
    let text = "Anderson, B. (1983) Imagined Communities: Reflections on the Origin and Spread of \
                Nationalism. London: Verso.";
    assert!(assess(&Claim::from_text(text), &book).confidence >= VERIFIED);
    let claim = Claim {
        title: Some(
            "Imagined Communities: Reflections on the Origin and Spread of Nationalism".into(),
        ),
        authors: vec!["Anderson, B.".into()],
        year: Some(1983),
        ..Claim::default()
    };
    assert!(assess(&claim, &book).confidence >= VERIFIED);
    book.work.work_type = Some(crate::work::WorkType::JournalArticle);
    assert!(assess(&claim, &book).confidence < VERIFIED);
}

#[test]
fn a_year_in_the_title_is_not_the_year_cited() {
    // Red team (#102): "Cancer statistics, 2020" cited as of 2018.
    let mut stats = placed(
        &[],
        "Cancer statistics, 2020",
        &["Rebecca L. Siegel", "Kimberly D. Miller", "Ahmedin Jemal"],
        2020,
        ("CA: A Cancer Journal for Clinicians", "70", "7-30"),
    );
    stats.work.work_type = Some(crate::work::WorkType::JournalArticle);
    for text in [
        "Siegel RL, Miller KD, Jemal A. Cancer statistics, 2020. CA Cancer J Clin. 2018;70(1):7-30.",
        "Siegel RL, Miller KD, Jemal A. Cancer statistics, 2020. 2017.",
    ] {
        let claim = Claim::from_text(text);
        let a = assess(&claim, &stats);
        assert_eq!(a.year.agreement, Agreement::Mismatch, "{text}");
        assert!(a.confidence < VERIFIED, "{text}: {a:?}");
        assert!(
            !claim
                .cited_years_beside(stats.work.title.as_deref())
                .contains(&2020)
        );
    }
    // Its title (year and all), journal, volume and pages name it: the
    // year cited is the citation's error, plausible. The 2018 report, the
    // year cited and in its title, is another title.
    let text = "Siegel RL, Miller KD, Jemal A. Cancer statistics, 2020. CA Cancer J Clin. 2018;70(1):7-30.";
    let a2020 = assess(&Claim::from_text(text), &stats);
    assert!(
        (PLAUSIBLE..VERIFIED).contains(&a2020.confidence),
        "{a2020:?}"
    );
    let mut stats2018 = placed(
        &[],
        "Cancer statistics, 2018",
        &["Rebecca L. Siegel", "Kimberly D. Miller", "Ahmedin Jemal"],
        2018,
        ("CA: A Cancer Journal for Clinicians", "68", "7-30"),
    );
    stats2018.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let a2018 = assess(&Claim::from_text(text), &stats2018);
    assert_ne!(a2018.title.agreement, Agreement::Match, "{a2018:?}");
    assert!(a2018.confidence < PLAUSIBLE, "{a2018:?}");
    // Without the place, the year alone decides.
    let text = "Siegel RL, Miller KD, Jemal A. Cancer statistics, 2020. 2017.";
    assert!(assess(&Claim::from_text(text), &stats).confidence < PLAUSIBLE);
    for text in [
        "Siegel RL, Miller KD, Jemal A. Cancer statistics, 2020. CA Cancer J Clin. 2020;70(1):7-30.",
        "Siegel RL, Miller KD, Jemal A. Cancer statistics, 2020.",
    ] {
        let a = assess(&Claim::from_text(text), &stats);
        assert_ne!(a.year.agreement, Agreement::Mismatch, "{text}");
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
}

#[test]
fn a_journal_after_a_capitalised_title_end_is_read() {
    // Red team (#102): "… Wuhan, China. Lancet." read as a dotted name
    // that may have begun earlier, so no journal, and the wrong one passed.
    let mut wang = placed(
        &[],
        "Clinical Characteristics of 138 Hospitalized Patients With 2019 Novel \
         Coronavirus–Infected Pneumonia in Wuhan, China",
        &["Dawei Wang", "Bo Hu", "Chang Hu"],
        2020,
        ("JAMA", "323", "1061-1069"),
    );
    wang.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let text = "Wang D, Hu B, Hu C, et al. Clinical characteristics of 138 hospitalized patients \
                with 2019 novel coronavirus-infected pneumonia in Wuhan, China. Lancet. \
                2020;323(11):1061-1069.";
    let a = assess(&Claim::from_text(text), &wang);
    assert_eq!(a.container.agreement, Agreement::Mismatch, "{a:?}");
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    let a = assess(&Claim::from_text(text.replace("Lancet", "JAMA")), &wang);
    assert_eq!(a.container.agreement, Agreement::Match, "{a:?}");
    assert!(a.confidence >= VERIFIED, "{a:?}");
}

#[test]
fn a_journal_cited_without_volume_and_page_is_read() {
    // Red team (#102): "… JAMA. 2020." gave no journal, so the wrong one
    // passed; with the year one off as well, two small errors verified.
    let lstm = {
        let mut r = placed(
            &[],
            "Long Short-Term Memory",
            &["Sepp Hochreiter", "Jürgen Schmidhuber"],
            1997,
            ("Neural Computation", "9", "1735-1780"),
        );
        r.work.work_type = Some(crate::work::WorkType::JournalArticle);
        r
    };
    for text in [
        "Hochreiter S, Schmidhuber J. Long short-term memory. Neural Netw. 1997.",
        "Hochreiter S, Schmidhuber J. Long short-term memory. Neural Netw. 1998.",
        "S. Hochreiter and J. Schmidhuber, \"Long short-term memory,\" Neural Networks, vol. 9, \
         1997.",
        "Hochreiter, S. and Schmidhuber, J. (1997) 'Long short-term memory', Neural Networks.",
    ] {
        let a = assess(&Claim::from_text(text), &lstm);
        assert_eq!(a.container.agreement, Agreement::Mismatch, "{text}");
        assert!(
            (PLAUSIBLE..VERIFIED).contains(&a.confidence),
            "{text}: {a:?}"
        );
    }
    for text in [
        "Hochreiter S, Schmidhuber J. Long short-term memory. Neural Comput. 1997.",
        "S. Hochreiter and J. Schmidhuber, \"Long short-term memory,\" Neural Computation, vol. 9, \
         1997.",
    ] {
        let a = assess(&Claim::from_text(text), &lstm);
        assert_eq!(a.container.agreement, Agreement::Match, "{text}");
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
    // A title in title case before the year is the title, not a journal.
    let mut gravitons = placed(
        &[],
        "Hot Gravitons From Kerr Black Holes",
        &["Dan Hooper", "Gordan Krnjaic"],
        2020,
        ("Physical Review D", "101", "1"),
    );
    gravitons.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let text = "Hooper D, Krnjaic G. Hot Gravitons From Kerr Black Holes. 2020.";
    let a = assess(&Claim::from_text(text), &gravitons);
    assert_eq!(a.container.agreement, Agreement::Unknown, "{a:?}");
    assert!(a.confidence >= VERIFIED, "{a:?}");
}

#[test]
fn a_copy_without_journal_or_place_cannot_confirm_the_journal_cited() {
    // Red team (#102): a Zenodo deposit of Ioannidis's essay, no journal,
    // volume or page, verified a reference to the article in another
    // journal.
    let mut copy = record(
        &["10.5281/zenodo.13525448"],
        "Why Most Published Research Findings Are False",
        &["John P. A. Ioannidis"],
        2005,
    );
    copy.work.work_type = Some(crate::work::WorkType::JournalArticle);
    for text in [
        "Ioannidis JP. Why most published research findings are false. BMJ. 2005.",
        "Ioannidis JP. Why most published research findings are false. PLoS Med. \
         2005;2(8):e124.",
    ] {
        let a = assess(&Claim::from_text(text), &copy);
        assert!(
            (PLAUSIBLE..VERIFIED).contains(&a.confidence),
            "{text}: {a:?}"
        );
    }
    // Unless the claim's identifier names it, or no journal is cited.
    for text in [
        "Ioannidis JP. Why most published research findings are false. Zenodo. 2005. \
         doi:10.5281/zenodo.13525448",
        "Ioannidis JP. Why most published research findings are false. 2005.",
    ] {
        let a = assess(&Claim::from_text(text), &copy);
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
    // A preprint has no journal to give.
    let mut preprint = copy.clone();
    preprint.work.work_type = Some(crate::work::WorkType::Preprint);
    let text = "Ioannidis JP. Why most published research findings are false. medRxiv. 2005.";
    assert!(assess(&Claim::from_text(text), &preprint).confidence >= VERIFIED);
    // Nor does a record in another series, without volume or page, confirm
    // a journal, volume and page all cited (PsycEXTRA's record of a paper
    // Science printed).
    let mut psycextra = record(
        &["10.1037/e301492005-001"],
        "Judgment Under Uncertainty: Heuristics and Biases",
        &["Amos Tversky", "Daniel Kahneman"],
        1974,
    );
    psycextra.work.work_type = Some(crate::work::WorkType::Dataset);
    psycextra.work.container_title = Some("PsycEXTRA Dataset".into());
    let text = "Tversky A, Kahneman D. Judgment under uncertainty: heuristics and biases. Science. \
                1974;185:1124.";
    let a = assess(&Claim::from_text(text), &psycextra);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
}

#[test]
fn another_title_at_the_right_place_is_a_different_title() {
    // Red team (#102): the title was treated as missing whenever journal
    // and volume agreed, and the place verified another work's title.
    let mut niwa = placed(
        &[],
        "Lytic enzyme, labiase for a broad range of Gram-positive bacteria",
        &["Tomoko Niwa", "Yoshiaki Kawamura"],
        2005,
        ("Journal of Microbiological Methods", "61", "251-260"),
    );
    niwa.work.work_type = Some(crate::work::WorkType::JournalArticle);
    for (text, record) in [
        (
            "Niwa T. Transfection of plasmid DNA by nanocarriers. J Microbiol Methods. 2005;61:251.",
            niwa,
        ),
        (
            "Rivers E, et al. Vasopressin versus norepinephrine for septic shock. N Engl J Med. \
             2001;345(19):1368-1377.",
            egdt(),
        ),
    ] {
        let a = assess(&Claim::from_text(text), &record);
        assert_eq!(a.title.agreement, Agreement::Mismatch, "{text}");
        assert!(
            (PLAUSIBLE..VERIFIED).contains(&a.confidence),
            "{text}: {a:?}"
        );
    }
    // Without a title, the place still names the work.
    for text in [
        "Rivers E, et al. N Engl J Med. 2001;345:1368.",
        "Rivers E, Nguyen B. NEJM 2001;345(19):1368-77. Epub 2001 Nov 8.",
    ] {
        let a = assess(&Claim::from_text(text), &egdt());
        assert_eq!(a.title.agreement, Agreement::Unknown, "{text}");
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
}

#[test]
fn bibtex_harvard_and_single_page_locators_are_checked() {
    // Red team (#102): the wrong numbers went unread in these styles.
    let bibtex = |volume: &str| {
        format!(
            "@article{{l, author = {{LeCun, Yann and Bengio, Yoshua}}, title = {{Deep learning}}, \
             journal = {{Nature}}, year = {{2015}}, volume = {{{volume}}}, pages = {{436--444}}}}"
        )
    };
    for (text, right) in [
        (bibtex("521"), true),
        (bibtex("597"), false),
        (
            "LeCun, Y., Bengio, Y. & Hinton, G. Deep learning. Nature 521, 436 (2015).".to_owned(),
            true,
        ),
        (
            "LeCun, Y., Bengio, Y. & Hinton, G. Deep learning. Nature 521, 999 (2015).".to_owned(),
            false,
        ),
        (
            "LeCun, Y. et al. (2015) 'Deep learning', Nature, 521, pp. 436-444.".to_owned(),
            true,
        ),
        (
            "LeCun, Y. et al. (2015) 'Deep learning', Science, 521, pp. 436-444.".to_owned(),
            false,
        ),
    ] {
        let a = assess(&Claim::from_text(&text), &lecun());
        if right {
            assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
        } else {
            assert!(
                (PLAUSIBLE..VERIFIED).contains(&a.confidence),
                "{text}: {a:?}"
            );
        }
    }
}

#[test]
fn a_preprint_and_a_journal_article_are_other_versions_of_each_other() {
    // Red team (#102): an arXiv preprint cited in a journal by name alone,
    // and a journal's article cited as a preprint, verified.
    let mut preprint = record(
        &["arxiv:2410.02479"],
        "Cross-Embodiment Dexterous Grasping with Reinforcement Learning",
        &["Haoqi Yuan", "Bohan Zhou"],
        2024,
    );
    preprint.work.work_type = Some(crate::work::WorkType::Preprint);
    let text = "Yuan H, Zhou B. Cross-embodiment dexterous grasping with reinforcement learning. \
                Science Robotics. 2024.";
    let a = assess(&Claim::from_text(text), &preprint);
    assert_eq!(a.container.agreement, Agreement::Mismatch, "{a:?}");
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    // Cited as the preprint it is, or by its identifier: verified.
    for text in [
        "Yuan H, Zhou B. Cross-embodiment dexterous grasping with reinforcement learning. arXiv. \
         2024.",
        "Yuan H, Zhou B. Cross-embodiment dexterous grasping with reinforcement learning. Science \
         Robotics. 2024. arXiv:2410.02479",
    ] {
        let a = assess(&Claim::from_text(text), &preprint);
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
    for text in [
        format!("Rivers E, Nguyen B. {EGDT}. medRxiv preprint, 2001."),
        format!("Rivers E, Nguyen B. {EGDT}. arXiv preprint. 2001."),
    ] {
        let a = assess(&Claim::from_text(&text), &egdt());
        assert_eq!(a.container.agreement, Agreement::Mismatch, "{text}");
        assert!(
            (PLAUSIBLE..VERIFIED).contains(&a.confidence),
            "{text}: {a:?}"
        );
    }
}

#[test]
fn reprints_and_versions_cited_with_another_year_are_plausible() {
    // Red team (#102) real misses: a book reprinted whole in a reader, and
    // a dataset whose DOI's record carries its latest version's year.
    let mut reprint = record(
        &["10.4324/9781315680347-10"],
        "Distinction: A Social Critique of the Judgement of Taste",
        &["Pierre Bourdieu"],
        2018,
    );
    reprint.work.work_type = Some(crate::work::WorkType::Chapter);
    let text = "Bourdieu P. Distinction: A Social Critique of the Judgement of Taste. Cambridge, \
                MA: Harvard University Press; 1984.";
    let a = assess(&Claim::from_text(text), &reprint);
    assert_eq!(a.year.agreement, Agreement::Mismatch);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    // Not a chapter cited as an article with the wrong year.
    let text = "Bourdieu P. Distinction: a social critique of the judgement of taste. Soc Sci Inf. \
                1984;13:1-20.";
    assert!(assess(&Claim::from_text(text), &reprint).confidence < PLAUSIBLE);

    let mut mimic = record(
        &["10.13026/c2xw26"],
        "MIMIC-III Clinical Database",
        &["Alistair Johnson", "Tom Pollard", "Roger Mark"],
        2023,
    );
    mimic.work.work_type = Some(crate::work::WorkType::Dataset);
    let text = "Johnson A, Pollard T, Mark R. MIMIC-III Clinical Database (version 1.4). PhysioNet. \
                2016. doi:10.13026/C2XW26";
    let a = assess(&Claim::from_text(text), &mimic);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    // Without the identifier, the year still tells another version.
    let text = "Johnson A, Pollard T, Mark R. MIMIC-III Clinical Database. PhysioNet. 2016.";
    assert!(assess(&Claim::from_text(text), &mimic).confidence < PLAUSIBLE);
}

#[test]
fn the_report_titled_with_its_year_and_place_outranks_its_namesakes() {
    // Live (app pre-test): "Cancer statistics, 2020 … 2018;70(1):7-30" chose
    // "Colorectal cancer statistics, 2020" (its title partial, its page
    // another), taking the title's 2020 for the year cited.
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/resolve/cancer-statistics-cited-2018.json"
    ))
    .unwrap();
    let claim = Claim::from_text(fixture["claim"].as_str().unwrap());
    assert_eq!(claim.as_read().year, Some(2018), "the year cited");
    let field = |c: &serde_json::Value, k: &str| c[k].as_str().unwrap().to_owned();
    let assessed: Vec<(String, Assessment)> = fixture["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let authors: Vec<String> = c["authors"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a.as_str().unwrap().to_owned())
                .collect();
            let authors: Vec<&str> = authors.iter().map(String::as_str).collect();
            let year = i16::try_from(c["year"].as_i64().unwrap()).unwrap();
            let at = (field(c, "container"), field(c, "volume"), field(c, "page"));
            let record = article(&field(c, "title"), &authors, year, (&at.0, &at.1, &at.2));
            (field(c, "id"), assess(&claim, &record))
        })
        .collect();
    let (best, a) = assessed
        .iter()
        .max_by(|x, y| x.1.confidence.total_cmp(&y.1.confidence))
        .unwrap();
    assert_eq!(best, fixture["cited"].as_str().unwrap(), "{assessed:#?}");
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    assert_eq!(
        (a.title.agreement, a.container.agreement, a.year.agreement),
        (Agreement::Match, Agreement::Match, Agreement::Mismatch)
    );
    for (id, other) in &assessed {
        if id != best {
            assert!(other.confidence < a.confidence, "{id}: {other:?}");
        }
    }
}

#[test]
fn a_title_with_words_inserted_is_not_the_records() {
    // App pre-test: words inserted in the middle of a real title.
    let sepsis3 = article(
        "The Third International Consensus Definitions for Sepsis and Septic Shock (Sepsis-3)",
        &[
            "Mervyn Singer",
            "Clifford S. Deutschman",
            "Christopher Warren Seymour",
        ],
        2016,
        ("JAMA", "315", "801-810"),
    );
    let cite = |title: &str| {
        Claim::from_text(format!(
            "Singer M, Deutschman CS, Seymour CW, et al. {title}. JAMA. 2016;315(8):801-810."
        ))
    };
    let a = assess(
        &cite(
            "The Third International Consensus Definitions for Sepsis and Septic Shock in Adults \
             and Children (Sepsis-3)",
        ),
        &sepsis3,
    );
    assert_eq!(a.title.agreement, Agreement::Partial, "{a:?}");
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    // Words dropped, or the title as it is: the same title.
    for title in [
        "The Third International Consensus Definitions for Sepsis and Septic Shock (Sepsis-3)",
        "The third international consensus definitions for sepsis and septic shock",
        "Third International Consensus Definitions for Sepsis (Sepsis-3)",
    ] {
        let a = assess(&cite(title), &sepsis3);
        assert!(a.confidence >= VERIFIED, "{title}: {a:?}");
    }
}

#[test]
fn a_title_with_a_word_replaced_is_not_the_records() {
    // Red team (#102): another drug's trial at RECOVERY's place verified.
    let mut recovery = placed(
        &[],
        "Dexamethasone in Hospitalized Patients with Covid-19",
        &["Peter Horby", "Wei Shen Lim", "Jonathan R. Emberson"],
        2021,
        ("New England Journal of Medicine", "384", "693-704"),
    );
    recovery.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let text = "Horby P, Lim WS, Emberson JR, et al. Hydroxychloroquine in hospitalized patients \
                with Covid-19. N Engl J Med. 2021;384(8):693-704.";
    let a = assess(&Claim::from_text(text), &recovery);
    assert_eq!(a.title.agreement, Agreement::Partial, "{a:?}");
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    // A misspelt or respelt word is the same title.
    for text in [
        "Horby P, Lim WS, Emberson JR, et al. Dexamethazone in hospitalized patients with \
         Covid-19. N Engl J Med. 2021;384(8):693-704.",
        "Horby P, Lim WS, Emberson JR, et al. Dexamethasone in hospitalized patients with \
         Covid-19. N Engl J Med. 2021;384(8):693-704.",
    ] {
        let a = assess(&Claim::from_text(text), &recovery);
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
}

/// A journal article's record for the red-team table.
fn article(title: &str, authors: &[&str], year: i16, at: (&str, &str, &str)) -> SourceRecord {
    let mut r = placed(&[], title, authors, year, at);
    r.work.work_type = Some(crate::work::WorkType::JournalArticle);
    r
}

#[test]
fn red_team_minimised_failures_stay_fixed() {
    // Minimised false VERIFIEDs from bench/redteam/failures.jsonl (#102),
    // against the records they verified: none may verify now. Each with
    // its id there.
    let nature = |title: &str, authors: &[&str], year, volume, pages| {
        article(title, authors, year, ("Nature", volume, pages))
    };
    let alphafold = nature(
        "Highly accurate protein structure prediction with AlphaFold",
        &["John Jumper", "Richard Evans", "Alexander Pritzel"],
        2021,
        "596",
        "583-589",
    );
    let lstm = article(
        "Long Short-Term Memory",
        &["Sepp Hochreiter", "Jürgen Schmidhuber"],
        1997,
        ("Neural Computation", "9", "1735-1780"),
    );
    let colchicine = article(
        "Efficacy and Safety of Low-Dose Colchicine after Myocardial Infarction",
        &["Jean-Claude Tardif", "Simon Kouz", "David D. Waters"],
        2019,
        ("New England Journal of Medicine", "381", "2497-2505"),
    );
    let huang = article(
        "Clinical features of patients infected with 2019 novel coronavirus in Wuhan, China",
        &["Chaolin Huang", "Yeming Wang", "Xingwang Li"],
        2020,
        ("The Lancet", "395", "497-506"),
    );
    let mut zenodo = record(
        &["10.5281/zenodo.13525448"],
        "Why Most Published Research Findings Are False",
        &["John P. A. Ioannidis"],
        2005,
    );
    zenodo.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let mut arxiv = record(
        &["arxiv:2410.02479"],
        "Cross-Embodiment Dexterous Grasping with Reinforcement Learning",
        &["Haoqi Yuan", "Bohan Zhou"],
        2024,
    );
    arxiv.work.work_type = Some(crate::work::WorkType::Preprint);
    let cases: Vec<(&str, Claim, SourceRecord)> = vec![
        (
            "corpus-174",
            Claim::from_text(
                "Tuyls K, Rourke-Ellison D. Evolutionary game theory and multi-agent \
                 reinforcement learning. 2005.",
            ),
            article(
                "Evolutionary game theory and multi-agent reinforcement learning",
                &["Karl Tuyls", "Ann Nowé"],
                2005,
                ("The Knowledge Engineering Review", "20", "63-90"),
            ),
        ),
        (
            "adv-18",
            Claim::from_text(format!(
                "Rivers E, Smith J, Patel R, et al. {EGDT}. N Engl J Med. 2001;345(19):1368-77."
            )),
            egdt(),
        ),
        (
            "adv-21",
            Claim {
                title: Some(
                    "Fibrinolysis for patients with intermediate-risk pulmonary embolism".into(),
                ),
                authors: vec!["Meyer, G.".into(), "Smith, J.".into()],
                year: Some(2014),
                ..Claim::default()
            },
            peitho(),
        ),
        (
            "adv-01",
            Claim::from_text(
                "LeCun Y, Bengio Y, Hinton G. Deep learning for medical image analysis. Nature. \
                 2015;521:436-444.",
            ),
            lecun(),
        ),
        (
            "adv-13",
            Claim {
                title: Some("Deep learning: a primer for clinicians".into()),
                authors: vec!["LeCun, Y.".into(), "Bengio, Y.".into(), "Hinton, G.".into()],
                year: Some(2015),
                container: Some("Nature".into()),
                ..Claim::default()
            },
            lecun(),
        ),
        (
            "adv-12",
            Claim::from_text(
                "Hanahan D, Weinberg RA. Hallmarks of cancer: the next generation of targeted \
                 therapies. Cell. 2011;144(5):646-674.",
            ),
            hallmarks(),
        ),
        (
            "adv-08",
            Claim::from_text(
                "Jumper J, Evans R, Pritzel A, et al. Highly accurate protein structure \
                 prediction with AlphaFold enables structure-based drug discovery. Nature. 2021.",
            ),
            alphafold.clone(),
        ),
        (
            "adv-33",
            Claim::from_text(
                "Siegel RL, Miller KD, Jemal A. Cancer statistics, 2020. CA Cancer J Clin. \
                 2018;70(1):7-30.",
            ),
            article(
                "Cancer statistics, 2020",
                &["Rebecca L. Siegel", "Kimberly D. Miller", "Ahmedin Jemal"],
                2020,
                ("CA: A Cancer Journal for Clinicians", "70", "7-30"),
            ),
        ),
        (
            "adv-39",
            Claim::from_text(
                "Hindricks G, Potpara T, Dagres N, et al. 2020 ESC Guidelines for the diagnosis \
                 and management of atrial fibrillation developed in collaboration with the \
                 European Association for Cardio-Thoracic Surgery (EACTS). Eur Heart J. 2017.",
            ),
            article(
                "2020 ESC Guidelines for the diagnosis and management of atrial fibrillation \
                 developed in collaboration with the European Association for Cardio-Thoracic \
                 Surgery (EACTS)",
                &["Gerhard Hindricks", "Tatjana Potpara", "Nikolaos Dagres"],
                2020,
                ("European Heart Journal", "42", "373-498"),
            ),
        ),
        (
            "adv-27",
            Claim::from_text(
                "Jumper J, Evans R, Pritzel A, et al. Highly accurate protein structure \
                 prediction with AlphaFold. Science. 2021;596(7873):583-589.",
            ),
            alphafold.clone(),
        ),
        (
            "corpus-203",
            Claim::from_text(
                "Huang C, et al. Clinical features of patients infected with 2019 novel \
                 coronavirus in Wuhan, China. JAMA. 2020.",
            ),
            huang,
        ),
        (
            "adv-56",
            Claim::from_text(
                "Hochreiter S, Schmidhuber J. Long short-term memory. Neural Netw. 1998.",
            ),
            lstm.clone(),
        ),
        (
            "corpus-202",
            Claim::from_text(
                "Altschul, S.F. et al. (1990) 'Basic local alignment search tool', American \
                 Journal of Epidemiology, 215(3), pp. 403-410.",
            ),
            article(
                "Basic local alignment search tool",
                &["Stephen F. Altschul", "Warren Gish", "Webb Miller"],
                1990,
                ("Journal of Molecular Biology", "215", "403-410"),
            ),
        ),
        (
            "adv-52",
            Claim::from_text(
                "Silver, D. et al. Mastering the game of Go with deep neural networks and tree \
                 search. Nature 530, 484 (2016).",
            ),
            nature(
                "Mastering the game of Go with deep neural networks and tree search",
                &["David Silver", "Aja Huang", "Chris J. Maddison"],
                2016,
                "529",
                "484-489",
            ),
        ),
        (
            "adv-53",
            Claim::from_text(
                "@article{jumper2021, author = {Jumper, John and Evans, Richard}, title = \
                 {Highly accurate protein structure prediction with {AlphaFold}}, journal = \
                 {Nature}, year = {2021}, volume = {597}, pages = {583--589}}",
            ),
            alphafold,
        ),
        (
            "corpus-219",
            Claim::from_text(
                "Niwa T. Transfection of plasmid DNA by nanocarriers. J Microbiol Methods. \
                 2005;61:251.",
            ),
            article(
                "Lytic enzyme, labiase for a broad range of Gram-positive bacteria",
                &["Tomoko Niwa", "Yoshiaki Kawamura"],
                2005,
                ("Journal of Microbiological Methods", "61", "251-260"),
            ),
        ),
        (
            "adv-44",
            Claim::from_text(
                "Horby P, Lim WS, Emberson JR, et al. Hydroxychloroquine in hospitalized \
                 patients with Covid-19. N Engl J Med. 2021;384(8):693-704.",
            ),
            article(
                "Dexamethasone in Hospitalized Patients with Covid-19",
                &["Peter Horby", "Wei Shen Lim", "Jonathan R. Emberson"],
                2021,
                ("New England Journal of Medicine", "384", "693-704"),
            ),
        ),
        (
            "adv-57",
            Claim::from_text(
                "Ioannidis JP. Why most published research findings are false. BMJ. 2006.",
            ),
            zenodo,
        ),
        (
            "corpus-271",
            Claim::from_text(
                "Yuan H. Cross-embodiment dexterous grasping with reinforcement learning. \
                 Science Robotics. 2024.",
            ),
            arxiv,
        ),
        (
            "corpus-277",
            Claim::from_text(
                "Tardif JC. Efficacy and safety of low-dose colchicine after myocardial \
                 infarction. medRxiv. 2019.",
            ),
            colchicine.clone(),
        ),
        (
            "corpus-275",
            Claim::from_text("Hochreiter S. Long short-term memory. arXiv preprint. 1997."),
            lstm,
        ),
        (
            "adv-45",
            Claim::from_text(
                "World Medical Association. Declaration of Geneva: the physician's pledge. JAMA. \
                 2013;310(20):2191-2194.",
            ),
            {
                let mut r = placed(
                    &[],
                    "World Medical Association Declaration of Helsinki",
                    &[],
                    2013,
                    ("JAMA", "310", "2191-2194"),
                );
                r.work.work_type = Some(crate::work::WorkType::JournalArticle);
                r
            },
        ),
        (
            "corpus-193",
            Claim::from_text(
                "Shiomi and SUGIURA reported in Gastroenterology (1984) — \"\
                 ヒメフタテンヨコバイが媒介するマイコプラズマ様微生物の寄主範囲による類別\".",
            ),
            {
                let mut r = record(
                    &["10.3186/jjphytopath.50.149"],
                    "ヒメフタテンヨコバイが媒介するマイコプラズマ様微生物の寄主範囲による類別",
                    &["Toshiki Shiomi", "Miyoji SUGIURA"],
                    1984,
                );
                r.work.work_type = Some(crate::work::WorkType::JournalArticle);
                r
            },
        ),
        // The app's pre-test of the fixes: the report of the year cited
        // chosen over the one titled, volume and pages; words inserted
        // inside a title.
        (
            "app-cancer-statistics",
            Claim::from_text(
                "Siegel RL, Miller KD, Jemal A. Cancer statistics, 2020. CA Cancer J Clin. \
                 2018;70(1):7-30.",
            ),
            article(
                "Cancer statistics, 2018",
                &["Rebecca L. Siegel", "Kimberly D. Miller", "Ahmedin Jemal"],
                2018,
                ("CA: A Cancer Journal for Clinicians", "68", "7-30"),
            ),
        ),
        (
            "app-sepsis-3",
            Claim::from_text(
                "Singer M, Deutschman CS, Seymour CW, et al. The Third International Consensus \
                 Definitions for Sepsis and Septic Shock in Adults and Children (Sepsis-3). JAMA. \
                 2016;315(8):801-810.",
            ),
            article(
                "The Third International Consensus Definitions for Sepsis and Septic Shock \
                 (Sepsis-3)",
                &[
                    "Mervyn Singer",
                    "Clifford S. Deutschman",
                    "Christopher Warren Seymour",
                ],
                2016,
                ("JAMA", "315", "801-810"),
            ),
        ),
    ];
    for (id, claim, record) in &cases {
        let a = assess(claim, record);
        assert!(a.confidence < VERIFIED, "{id}: {a:?}");
    }
    // The same works cited right still verify.
    for (text, record) in [
        (
            "Tardif JC, Kouz S, Waters DD, et al. Efficacy and safety of low-dose colchicine \
             after myocardial infarction. N Engl J Med. 2019;381(26):2497-2505.",
            colchicine,
        ),
        (
            "Hanahan D, Weinberg RA. Hallmarks of cancer: the next generation. Cell. \
             2011;144(5):646-674.",
            hallmarks(),
        ),
        (
            "LeCun, Y., Bengio, Y. & Hinton, G. Deep learning. Nature 521, 436–444 (2015).",
            lecun(),
        ),
    ] {
        let a = assess(&Claim::from_text(text), &record);
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
}

#[test]
fn held_out_red_team_failures_stay_fixed() {
    // Minimised false VERIFIEDs from bench/redteam/holdout-1.jsonl (round
    // 2), against the records they verified: none may verify now. Each
    // with its line there.
    let ids = |mut r: SourceRecord, id: &str| {
        r.ids = vec![id.parse().unwrap()];
        r
    };
    let jcp = article(
        "Anisotropic Rayleigh scattering investigations in smectic A liquid crystal phases",
        &["O. Mondain-Monval", "H. J. Coles", "T. Claverie"],
        1994,
        ("The Journal of Chemical Physics", "101", "6301"),
    );
    let tsunami = article(
        "After the Tsunami: Legal Implications of Mass Burials of Unidentified Victims in Sri \
         Lanka",
        &["Clifford Perera"],
        2005,
        ("PLoS Medicine", "2", "e185"),
    );
    let khandekar = article(
        "Graph partitioning using single commodity flows",
        &["Rohit Khandekar", "Satish Rao", "Umesh Vazirani"],
        2009,
        ("Journal of the ACM", "56", "1-15"),
    );
    let mut eclinm = record(
        &["10.1016/j.eclinm.2020.100519"],
        "Sex-bias in COVID-19-associated illness severity and mortality in cancer patients: A \
         systematic review and meta-analysis",
        &["Robin Park", "Anusha Chidharla", "Kathan Mehta"],
        2020,
    );
    eclinm.work.work_type = Some(crate::work::WorkType::JournalArticle);
    eclinm.work.container_title = Some("EClinicalMedicine".into());
    // Europe PMC's record, its type not given.
    let mut nursing = record(
        &["10.1016/j.ijnurstu.2011.09.008"],
        "Addressing the complexity conundrum in and of nursing",
        &["David R Thompson", "Alexander M Clark"],
        2011,
    );
    nursing.work.container_title = Some("International journal of nursing studies".into());
    nursing.work.other_years = vec![2012];
    let laris = placed(
        &["10.1038/s41467-021-25141-1"],
        "On the problems and promises of savanna fire regime change",
        &["Paul Laris"],
        2021,
        ("Nature communications", "12", "4891"),
    );
    let mut brain = record(
        &["10.1093/brain/awaf429"],
        "Differential selectivity of microglia and astrocytes in HIV-1 gp120-induced synaptic \
         pruning",
        &[
            "Zachary T Watson",
            "Silvana Valdebenito-Silva",
            "Michael S. Spurgat",
        ],
        2025,
    );
    brain.work.work_type = Some(crate::work::WorkType::JournalArticle);
    brain.work.container_title = Some("Brain".into());
    let mut tpami = record(
        &["10.1109/tpami.1982.4767218"],
        "Adaptive Detection and Removal of Non-Gaussian Spikes from Gaussian Data",
        &["Rachel Boucher", "J.P. Noonan"],
        1982,
    );
    tpami.work.work_type = Some(crate::work::WorkType::JournalArticle);
    tpami.work.container_title =
        Some("IEEE Transactions on Pattern Analysis and Machine Intelligence".into());
    let goldman = ids(
        article(
            "Cellular specificity of serotonin storage and axonal transport in identified \
             neurones of Aplysia californica",
            &["James E. Goldman", "James H. Schwartz"],
            1974,
            ("The Journal of Physiology", "242", "61-76"),
        ),
        "10.1113/jphysiol.1974.sp010694",
    );
    let cases: Vec<(&str, Claim, SourceRecord)> = vec![
        (
            "h1-475",
            Claim::from_text(
                "Mondain-Monval, O. et al. Anisotropic Rayleigh scattering investigations in \
                 smectic A liquid crystal phases. J. Org. Chem. 101, 6301 (1994).",
            ),
            jcp.clone(),
        ),
        (
            "h1-478",
            Claim {
                title: Some("Addressing the complexity conundrum in and of nursing".into()),
                authors: vec!["Thompson, D.R.".into(), "Clark, A.M.".into()],
                year: Some(2012),
                container: Some("Annals of Internal Medicine".into()),
                ..Claim::default()
            },
            nursing,
        ),
        (
            "h1-483",
            Claim::from_text(
                "Perera C. After the Tsunami: Legal Implications of Mass Burials of Unidentified \
                 Victims in Sri Lanka. Cad. Saúde Pública. 2005;2(6):e185.",
            ),
            tsunami.clone(),
        ),
        (
            "h1-493",
            Claim::from_text(
                "R. Khandekar, S. Rao, and U. Vazirani, “Graph partitioning using single \
                 commodity flows,” IEEE Trans. Pattern Anal. Mach. Intell., vol. 56, no. 4, pp. \
                 1-15, 2009.",
            ),
            khandekar.clone(),
        ),
        (
            "h1-494",
            Claim::from_text(
                "Park, Robin, Anusha Chidharla, and Kathan Mehta. “Sex-bias in \
                 COVID-19-associated illness severity and mortality in cancer patients: A \
                 systematic review and meta-analysis.” American Journal of Epidemiology (2020). \
                 https://doi.org/10.1016/j.eclinm.2020.100519",
            ),
            eclinm.clone(),
        ),
        (
            "h1-503",
            Claim::from_text(
                "Laris, P. On the problems and promises of savanna fire regime change. Sci Rep \
                 12, 4891 (2021).",
            ),
            laris.clone(),
        ),
        (
            "h1-476",
            Claim::from_text(
                "Watson ZT, Valdebenito-Silva S, Spurgat M, et al. Differential selectivity of \
                 microglia and astrocytes in HIV-1 gp120-induced synaptic pruning. JAMA: The \
                 Journal of the American Medical Association. 2025. doi:10.1093/brain/awaf429",
            ),
            brain,
        ),
        (
            "h1-484",
            Claim::from_text(
                "Boucher, R. E., & Noonan, J. P. (1982). Adaptive Detection and Removal of \
                 Non-Gaussian Spikes from Gaussian Data. Proceedings of the AAAI Symposium \
                 Series. https://doi.org/10.1109/tpami.1982.4767218",
            ),
            tpami,
        ),
        (
            "h1-505",
            Claim::from_text(
                "Mondain-Monval, O. et al. Anisotropic Rayleigh scattering investigations in \
                 smectic A liquid crystal phases. J. Chem. Phys. 102, 6301 (1994).",
            ),
            jcp.clone(),
        ),
        (
            "h1-469",
            Claim::from_text(
                "Goldman, J. E., & Schwartz, J. H. (1972). Cellular specificity of serotonin \
                 storage and axonal transport in identified neurones of Aplysia californica. \
                 The Journal of Physiology, 242(1), 61-76. \
                 https://doi.org/10.1113/jphysiol.1974.sp010694",
            ),
            goldman.clone(),
        ),
        (
            "h1-738",
            Claim::from_text(
                "C. Brukner, M. Zukowski, and A. Zeilinger, “The essence of entanglement,” Phys. \
                 Rev. A, vol. 63, no. 10, pp. 644-661, 2001.",
            ),
            article(
                "Conceptual inadequacy of the Shannon information in quantum measurements",
                &["Časlav Brukner", "Anton Zeilinger"],
                2001,
                ("Physical Review A", "63", "644"),
            ),
        ),
        (
            "h1-694",
            Claim::from_text(
                "@inproceedings{k2022, author = {Kingsley-Odhiambo, Henrik and Merriweather-Sung, \
                 Ilse}, title = {Federated contrastive pretraining for clinical note \
                 de-identification}, booktitle = {Findings of the Association for Computational \
                 Linguistics: EMNLP 2022}, year = {2022}, pages = {3321--3334}}",
            ),
            {
                let mut r = record(
                    &["isbn:9781959429432"],
                    "Findings of the Association for Computational Linguistics",
                    &["Association for Computational Linguistics"],
                    2022,
                );
                r.work.work_type = Some(crate::work::WorkType::Book);
                r
            },
        ),
        (
            "h1-496",
            Claim::from_text(
                "@article{levitt1996, author = {Levitt, S. D.}, title = {The Effect of Prison \
                 Population Size on Crime Rates: Evidence from Prison Overcrowding Litigation}, \
                 journal = {Social Science & Medicine}, year = {1996}}",
            ),
            {
                let mut r = record(
                    &[],
                    "The effect of prison population size on crime rates: evidence from prison \
                     overcrowding litigation",
                    &["Steven D. Levitt"],
                    1996,
                );
                r.work.work_type = Some(crate::work::WorkType::Book);
                r
            },
        ),
        (
            "h1-491",
            Claim::from_text(
                "Berry, Christopher J. “Adam Smith's Considerations on Language.” Revue \
                 historique 35, no. 1 (1974): 130.",
            ),
            article(
                "Adam Smith's Considerations on Language",
                &["Christopher J. Berry"],
                1974,
                ("Journal of the History of Ideas", "35", "130-138"),
            ),
        ),
        (
            "h1-458",
            Claim::from_text(
                "@article{goldman1972, author = {Goldman, J. E. and Schwartz, J. H.}, title = \
                 {Cellular specificity of serotonin storage and axonal transport in identified \
                 neurones of Aplysia californica}, journal = {The Journal of Physiology}, year = \
                 {1972}, volume = {242}, pages = {61--76}, doi = \
                 {10.1113/jphysiol.1974.sp010694}}",
            ),
            goldman.clone(),
        ),
    ];
    for (id, claim, record) in &cases {
        let a = assess(claim, record);
        assert!(a.confidence < VERIFIED, "{id}: {a:?}");
    }
    // The same works cited right still verify.
    for (claim, record) in [
        (
            "Mondain-Monval, O. et al. Anisotropic Rayleigh scattering investigations in smectic \
             A liquid crystal phases. J. Chem. Phys. 101, 6301 (1994).",
            jcp,
        ),
        (
            "Perera C. After the Tsunami: Legal Implications of Mass Burials of Unidentified \
             Victims in Sri Lanka. PLoS Med. 2005;2(6):e185.",
            tsunami,
        ),
        (
            "R. Khandekar, S. Rao, and U. Vazirani, “Graph partitioning using single commodity \
             flows,” J. ACM, vol. 56, no. 4, pp. 1-15, 2009.",
            khandekar,
        ),
        (
            "Park, Robin, Anusha Chidharla, and Kathan Mehta. “Sex-bias in COVID-19-associated \
             illness severity and mortality in cancer patients: A systematic review and \
             meta-analysis.” EClinicalMedicine (2020). \
             https://doi.org/10.1016/j.eclinm.2020.100519",
            eclinm,
        ),
        (
            "Laris, P. On the problems and promises of savanna fire regime change. Nat Commun \
             12, 4891 (2021).",
            laris,
        ),
        (
            "Goldman, J. E., & Schwartz, J. H. (1974). Cellular specificity of serotonin \
             storage and axonal transport in identified neurones of Aplysia californica. The \
             Journal of Physiology, 242(1), 61-76. https://doi.org/10.1113/jphysiol.1974.sp010694",
            goldman,
        ),
    ] {
        let a = assess(&Claim::from_text(claim), &record);
        assert!(a.confidence >= VERIFIED, "{claim}: {a:?}");
    }
}

/// A conference paper's record: its proceedings as the container.
fn proceedings(title: &str, authors: &[&str], year: i16, venue: &str, pages: &str) -> SourceRecord {
    let mut r = record(&[], title, authors, year);
    r.work.work_type = Some(crate::work::WorkType::ConferencePaper);
    r.work.container_title = Some(venue.into());
    r.work.page = Some(pages.into());
    r
}

/// An arXiv preprint's record, as OpenAlex gives it: in no venue.
fn arxiv_preprint(id: &str, title: &str, authors: &[&str], year: i16) -> SourceRecord {
    let mut r = record(&[id], title, authors, year);
    r.work.work_type = Some(crate::work::WorkType::Preprint);
    r.work.container_title = Some("arXiv (Cornell University)".into());
    r
}

#[test]
fn second_held_out_red_team_failures_stay_fixed() {
    // Minimised false VERIFIEDs from bench/redteam/holdout-2.jsonl (round
    // 3), against the records they verified: none may verify now. Each
    // with its line there.
    let pix2pix = proceedings(
        "Image-to-Image Translation with Conditional Adversarial Networks",
        &["Phillip Isola", "Jun-Yan Zhu", "Tinghui Zhou"],
        2017,
        "2017 IEEE Conference on Computer Vision and Pattern Recognition (CVPR)",
        "5967-5976",
    );
    let deepwalk = proceedings(
        "DeepWalk: online learning of social representations",
        &["Bryan Perozzi", "Rami Al-Rfou", "Steven Skiena"],
        2014,
        "Proceedings of the 20th ACM SIGKDD international conference on Knowledge discovery \
         and data mining",
        "701-710",
    );
    let tpu = proceedings(
        "In-Datacenter Performance Analysis of a Tensor Processing Unit",
        &["Norman P. Jouppi", "Cliff Young", "Nishant Patil"],
        2017,
        "Proceedings - International Symposium on Computer Architecture/Proceedings",
        "1-12",
    );
    let mut attention = arxiv_preprint(
        "arxiv:1706.03762",
        "Attention Is All You Need",
        &VASWANI,
        2017,
    );
    attention
        .ids
        .push("doi:10.48550/arxiv.1706.03762".parse().unwrap());
    // Its place pooled from the proceedings' record.
    attention.work.volume = Some("30".into());
    let gan = arxiv_preprint(
        "arxiv:1406.2661",
        "Generative Adversarial Networks",
        &["Ian J. Goodfellow", "Jean Pouget-Abadie", "Mehdi Mirza"],
        2014,
    );
    let rmp = article(
        "Odd triplet superconductivity and related phenomena in superconductor-ferromagnet \
         structures",
        &["F. S. Bergeret", "A. F. Volkov", "K. B. Efetov"],
        2005,
        ("Reviews of Modern Physics", "77", "1321-1373"),
    );
    let aer = article(
        "Consumption Disasters in the Twentieth Century",
        &["Robert J. Barro", "José F. Ursúa"],
        2008,
        ("American Economic Review", "98", "58-63"),
    );
    let prb = article(
        "Spin pumping in strongly coupled magnon-photon systems",
        &["H. Maier-Flaig", "Michael Harder", "Rudolf Gross"],
        2016,
        ("Physical Review B", "94", "054433"),
    );
    let nonaka = article(
        "A Dynamic Theory of Organizational Knowledge Creation",
        &["Ikujiro Nonaka"],
        1994,
        ("Organization Science", "5", "14-37"),
    );
    let jones = article(
        "Instrumental Stakeholder Theory: A Synthesis of Ethics and Economics",
        &["Thomas M. Jones"],
        1995,
        ("Academy of Management Review", "20", "404-437"),
    );
    let bar_haim = article(
        "Threat-related attentional bias in anxious and nonanxious individuals: A \
         meta-analytic study.",
        &["Yair Bar-Haim", "Dominique Lamy", "Lee Pergamin"],
        2007,
        ("Psychological Bulletin", "133", "1-24"),
    );
    let wu = article(
        "Risk Factors Associated With Acute Respiratory Distress Syndrome and Death in Patients \
         With Coronavirus Disease 2019 Pneumonia in Wuhan, China",
        &["Chaomin Wu", "Xiaoyan Chen", "Yanping Cai"],
        2020,
        ("JAMA Internal Medicine", "180", "934-943"),
    );
    let hastings = article(
        "Monte Carlo sampling methods using Markov chains and their applications",
        &["W. K. Hastings"],
        1970,
        ("Biometrika", "57", "97-109"),
    );
    let methadone = article(
        "Methadone maintenance treatment and mortality in people with criminal convictions: A \
         population-based retrospective cohort study from Canada",
        &["Angela Russolillo", "Akm Moniruzzaman", "Julian M. Somers"],
        2018,
        ("PLoS Medicine", "15", "e1002625"),
    );
    let varia = article(
        "Intraperitoneal Radioactive Phosphorus (32P) Versus Observation After Negative \
         Second-Look Laparotomy for Stage III Ovarian Carcinoma: A Randomized Trial of the \
         Gynecologic Oncology Group",
        &["Mark A. Varia", "Frederick B. Stehman", "Brian N. Bundy"],
        2003,
        ("Journal of Clinical Oncology", "21", "2849-2855"),
    );
    let unplaced = |mut r: SourceRecord| {
        r.work.volume = None;
        r.work.page = None;
        r
    };
    let bae = unplaced(article(
        "Resolving Conflicts between Informed Consent and Patients' Privacy in Medical \
         Emergencies",
        &["Hyuna Bae", "Seok-Bae LEE", "Hye-Young JANG"],
        2007,
        ("Korean Journal of Medical Ethics", "10", "73-82"),
    ));
    let trieling = unplaced(article(
        "Monopolar vortices in an irrotational annular shear flow",
        &["R. R. Trieling", "A. H. LINSSEN", "G. J. F. van Heijst"],
        1998,
        ("Journal of Fluid Mechanics", "360", "273-294"),
    ));
    let rangel = unplaced(article(
        "La enseñanza de la accesibilidad en los cursos de Archivística en Brasil: un análisis \
         formativo de los planes de estudio",
        &[
            "Thayron Rodrigues Rangel",
            "Lucía María Velloso de Oliveira",
        ],
        2024,
        ("Inclusão Social", "16", "1"),
    ));
    let mut autophagy = record(
        &["10.4161/auto.19496"],
        "Guidelines for the use and interpretation of assays for monitoring autophagy",
        &["Daniel J. Klionsky", "Fabio C. Abdalla", "Hagai Abeliovich"],
        2012,
    );
    autophagy.work.work_type = Some(crate::work::WorkType::JournalArticle);
    autophagy.work.container_title = Some("Autophagy".into());
    // OpenAlex types it a preprint (bioRxiv's DOI prefix), in a journal.
    let mut cytoscape = record(
        &["10.1101/gr.1239303", "pmid:14597658"],
        "Cytoscape: A Software Environment for Integrated Models of Biomolecular Interaction \
         Networks",
        &["Paul Shannon", "Andrew Markiel", "Owen Ozier"],
        2003,
    );
    cytoscape.work.work_type = Some(crate::work::WorkType::Preprint);
    cytoscape.work.container_title = Some("Genome Research".into());
    let panel = arxiv_preprint(
        "arxiv:2610.36176",
        "Probabilistic Causal Panel Analysis",
        &["Carl Bonander", "Mats Ekman"],
        2026,
    );
    let noma = article(
        "On the Performance of Non-Orthogonal Multiple Access in 5G Systems with Randomly \
         Deployed Users",
        &["Zhiguo Ding", "Zheng Yang", "Pingzhi Fan"],
        2014,
        ("IEEE Signal Processing Letters", "21", "1501-1505"),
    );
    let cases: Vec<(&str, Claim, &SourceRecord)> = vec![
        // k: a conference paper at another conference.
        (
            "h2-747",
            Claim::from_text(
                "Phillip Isola, Jun-Yan Zhu, Tinghui Zhou, and Alexei A. Efros. 2017. \
                 Image-to-Image Translation with Conditional Adversarial Networks. In IEEE \
                 International Conference on Computer Vision (ICCV), 5967–5976.",
            ),
            &pix2pix,
        ),
        (
            "h2-751",
            Claim::from_text(
                "@inproceedings{perozzi2014deepwalk, title={DeepWalk: online learning of social \
                 representations}, author={Perozzi, Bryan and Al-Rfou, Rami and Skiena, \
                 Steven}, booktitle={Proceedings of the International Conference on Machine \
                 Learning (ICML)}, pages={701--710}, year={2014}, publisher={ACM}}",
            ),
            &deepwalk,
        ),
        (
            "h2-753",
            Claim::from_text(
                "Norman P. Jouppi, Cliff Young, and Nishant Patil. 2017. In-Datacenter \
                 Performance Analysis of a Tensor Processing Unit. In IEEE/ACM International \
                 Symposium on Microarchitecture (MICRO), 1–12.",
            ),
            &tpu,
        ),
        // k: a preprint's record can't confirm the conference cited.
        (
            "h2-758",
            Claim::from_text(
                "A. Vaswani et al., \"Attention Is All You Need,\" in Proceedings of the 34th \
                 International Conference on Machine Learning (ICML 2017), 2017.",
            ),
            &attention,
        ),
        (
            "h2-761",
            Claim::from_text(
                "@inproceedings{goodfellow2014generative, title={Generative Adversarial \
                 Networks}, author={Goodfellow, Ian J. and Pouget-Abadie, Jean and Mirza, \
                 Mehdi}, booktitle={International Conference on Learning Representations (ICLR \
                 2014)}, year={2014}}",
            ),
            &gan,
        ),
        // c: locators and journals in styles not read before.
        (
            "h2-562",
            Claim::from_text(
                "Bergeret, F. S., et al. \"Odd triplet superconductivity and related phenomena \
                 in superconductor-ferromagnet structures.\" Physical Review B 77.4 (2005): \
                 1321-1373.",
            ),
            &rmp,
        ),
        (
            "h2-594",
            Claim::from_text(
                "Barro, R.J. and Ursúa, J.F., 2008. Consumption Disasters in the Twentieth \
                 Century. American Economic Review, 100(2), pp.58-63.",
            ),
            &aer,
        ),
        (
            "h2-564",
            Claim::from_text(
                "Maier-Flaig, H., Harder, M., Gross, R., 2016. Spin pumping in strongly coupled \
                 magnon-photon systems. Nature Physics, 94(5), p.054433.",
            ),
            &prb,
        ),
        (
            "h2-566",
            Claim::from_text(
                "25. Nonaka, I. A Dynamic Theory of Organizational Knowledge Creation. Psychol. \
                 Sci. 5, 14–37 (1994).",
            ),
            &nonaka,
        ),
        (
            "h2-595",
            Claim::from_text(
                "T. M. Jones, \"Instrumental Stakeholder Theory: A Synthesis of Ethics and \
                 Economics,\" Acad Management Rev, vol. 20, no. 2, Art. no. 615, 1995.",
            ),
            &jones,
        ),
        (
            "h2-557",
            Claim::from_text(
                "the 2007 Bar-Haim paper in Psychol Sci (Threat-related attentional bias in \
                 anxious and nonanxious individuals: A meta-analytic study.)",
            ),
            &bar_haim,
        ),
        // j: a title with words swapped for synonyms.
        (
            "h2-720",
            Claim::from_text(
                "Wu, Chaomin, et al. \"Likelihood Determinants Associated With Acute \
                 Respiratory Distress Syndrome and Death in Patients With Coronavirus Illness \
                 2019 Pneumonia in Wuhan, China.\" JAMA Internal Medicine 180.7 (2020): 934.",
            ),
            &wu,
        ),
        (
            "h2-745",
            Claim::from_text(
                "39. Hastings, W. K. Monte Carlo sampling approaches with Markov chains and their \
                 applications. Biometrika 57, 97–109 (1970).",
            ),
            &hastings,
        ),
        (
            "h2-732",
            Claim::from_text(
                "Russolillo, A., Moniruzzaman, A. & Somers, J. M. Methadone maintenance therapy \
                 and mort- ality in people with criminal convictions: A population-driven \
                 retrospective cohort study from Canada. PLoS Med. 15, e1002625 (2018).",
            ),
            &methadone,
        ),
        (
            "h2-731",
            Claim {
                title: Some(
                    "Intraperitoneal Radioactive Phosphorus (32P) Versus Observation After \
                     Negative Second-Look Laparotomy for Stage III Ovarian Carcinoma: A \
                     Controlled Study of the Gynecologic Oncology Group"
                        .into(),
                ),
                authors: vec!["Varia, Mark A.".into(), "Stehman, Frederick B.".into()],
                year: Some(2003),
                container: Some("Journal of Clinical Oncology".into()),
                volume: Some("21".into()),
                page: Some("2849-2855".into()),
                ..Claim::default()
            },
            &varia,
        ),
        // b: co-authors added beside the real first author.
        (
            "h2-482",
            Claim::from_text(
                "Bae, H-A., LEE, S-B., Corvastine, E. and JANG, H-Y., 2007. Resolving Conflicts \
                 between Informed Consent and Patients' Privacy in Medical Emergencies. Korean \
                 Journal of Medical Ethics, 10(1), pp.73-82.",
            ),
            &bae,
        ),
        (
            "h2-496",
            Claim::from_text(
                "TRIELING, R.R. and Corveshaw, C., 1998. Monopolar vortices in an irrotational \
                 annular shear flow. Journal of Fluid Mechanics, 360, pp.273-294.",
            ),
            &trieling,
        ),
        (
            "h2-495",
            Claim::from_text(
                "Rangel TR, Thalubrecht HL, María Velloso de Oliveira L. La enseñanza de la \
                 accesibilidad en los cursos de Archivística en Brasil: un análisis formativo de \
                 los planes de estudio. Inclusão Soc. 2024;16(1).",
            ),
            &rangel,
        ),
        // h: a journal's article cited as a preprint, a preprint in a journal.
        (
            "h2-718",
            Claim::from_text(
                "Klionsky, D. J., Abdalla, F. C., & Abeliovich, H. (2011). Guidelines for the \
                 use and interpretation of assays for monitoring autophagy. bioRxiv [Preprint].",
            ),
            &autophagy,
        ),
        (
            "h2-715",
            Claim::from_text(
                "Shannon, P., Markiel, A., & Ozier, O. (2002). Cytoscape: A Software \
                 Environment for Integrated Models of Biomolecular Interaction Networks. bioRxiv \
                 [Preprint].",
            ),
            &cytoscape,
        ),
        (
            "h2-699",
            Claim::from_text(
                "Bonander, Carl, et al. \"Probabilistic Causal Panel Analysis.\" Journal of \
                 Econometrics 251.1 (2026): 743-752.",
            ),
            &panel,
        ),
        // d: another work's place under a title in Chinese.
        (
            "h2-624",
            Claim::from_text(
                "[26] 周毅艺. 化学分析仪器计量检定分析[J]. IEEE Signal Processing Letters, 2014, \
                 21(12): 1501-1505.",
            ),
            &noma,
        ),
    ];
    let failing: Vec<String> = cases
        .iter()
        .filter_map(|(id, claim, record)| {
            let a = assess(claim, record);
            (a.confidence >= VERIFIED).then(|| format!("{id}: {a:?}"))
        })
        .collect();
    assert!(failing.is_empty(), "{failing:#?}");
    // f: BibTeX's arXiv eprint is the identifier claimed.
    let eprint = Claim::from_text(
        "@misc{shanu2017, title={A Computational Approach for Designing Tiger Corridors in \
         India}, author={Shanu, Saurabh}, year={2017}, eprint={1710.64837}, \
         archivePrefix={arXiv}, primaryClass={cs.CY}}",
    );
    assert_eq!(eprint.ids, ["arxiv:1710.64837".parse::<WorkId>().unwrap()]);

    // The same works cited right still verify.
    let right: Vec<(Claim, &SourceRecord)> = vec![
        (
            Claim::from_text(
                "Phillip Isola, Jun-Yan Zhu, and Tinghui Zhou. 2017. Image-to-Image Translation \
                 with Conditional Adversarial Networks. In Proceedings of the IEEE Conference on \
                 Computer Vision and Pattern Recognition (CVPR), 5967–5976.",
            ),
            &pix2pix,
        ),
        (
            Claim::from_text(
                "@inproceedings{perozzi2014deepwalk, title={DeepWalk: online learning of social \
                 representations}, author={Perozzi, Bryan and Al-Rfou, Rami and Skiena, \
                 Steven}, booktitle={Proceedings of the 20th ACM SIGKDD International Conference \
                 on Knowledge Discovery and Data Mining (KDD)}, pages={701--710}, year={2014}}",
            ),
            &deepwalk,
        ),
        (
            Claim::from_text(
                "Norman P. Jouppi, Cliff Young, and Nishant Patil. 2017. In-Datacenter \
                 Performance Analysis of a Tensor Processing Unit. In Proceedings of the 44th \
                 Annual International Symposium on Computer Architecture (ISCA), 1–12.",
            ),
            &tpu,
        ),
        (
            Claim::from_text(
                "Vaswani A, Shazeer N, Parmar N. Attention is all you need. arXiv:1706.03762. \
                 2017.",
            ),
            &attention,
        ),
        (
            Claim::from_text(
                "Bergeret, F. S., et al. \"Odd triplet superconductivity and related phenomena \
                 in superconductor-ferromagnet structures.\" Reviews of Modern Physics 77.4 \
                 (2005): 1321-1373.",
            ),
            &rmp,
        ),
        (
            Claim::from_text(
                "Barro, R.J. and Ursúa, J.F., 2008. Consumption Disasters in the Twentieth \
                 Century. American Economic Review, 98(2), pp.58-63.",
            ),
            &aer,
        ),
        (
            Claim::from_text(
                "Maier-Flaig, H., Harder, M., Gross, R., 2016. Spin pumping in strongly coupled \
                 magnon-photon systems. Physical Review B, 94(5), p.054433.",
            ),
            &prb,
        ),
        (
            Claim::from_text(
                "25. Nonaka, I. A Dynamic Theory of Organizational Knowledge Creation. Organ. \
                 Sci. 5, 14–37 (1994).",
            ),
            &nonaka,
        ),
        (
            Claim::from_text(
                "T. M. Jones, \"Instrumental Stakeholder Theory: A Synthesis of Ethics and \
                 Economics,\" Acad Management Rev, vol. 20, no. 2, Art. no. 404, 1995.",
            ),
            &jones,
        ),
        (
            Claim::from_text(
                "Wu, Chaomin, et al. \"Risk Factors Associated With Acute Respiratory Distress \
                 Syndrome and Death in Patients With Coronavirus Disease 2019 Pneumonia in Wuhan, \
                 China.\" JAMA Internal Medicine 180.7 (2020): 934.",
            ),
            &wu,
        ),
        (
            Claim::from_text(
                "Russolillo, A., Moniruzzaman, A. & Somers, J. M. Methadone maintenance \
                 treatment and mort- ality in people with criminal convictions: A \
                 population-based retrospective cohort study from Canada. PLoS Med. 15, \
                 e1002625 (2018).",
            ),
            &methadone,
        ),
        (
            Claim::from_text(
                "Bae, H-A., LEE, S-B. and JANG, H-Y., 2007. Resolving Conflicts between Informed \
                 Consent and Patients' Privacy in Medical Emergencies. Korean Journal of Medical \
                 Ethics, 10(1), pp.73-82.",
            ),
            &bae,
        ),
        (
            Claim::from_text(
                "Rangel TR, María Velloso de Oliveira L. La enseñanza de la accesibilidad en los \
                 cursos de Archivística en Brasil: un análisis formativo de los planes de \
                 estudio. Inclusão Soc. 2024;16(1).",
            ),
            &rangel,
        ),
        (
            Claim::from_text(
                "Klionsky, D. J., Abdalla, F. C., & Abeliovich, H. (2012). Guidelines for the \
                 use and interpretation of assays for monitoring autophagy. Autophagy, 8(4), \
                 445-544.",
            ),
            &autophagy,
        ),
    ];
    let failing: Vec<String> = right
        .iter()
        .filter_map(|(claim, record)| {
            let a = assess(claim, record);
            (a.confidence < VERIFIED).then(|| format!("{claim:?}: {a:?}"))
        })
        .collect();
    assert!(failing.is_empty(), "{failing:#?}");
}

#[test]
fn a_paper_merged_with_its_preprint_is_at_the_conference_its_doi_names() {
    // h2-747: the CVPR paper's record merged with its arXiv preprint's,
    // without a venue; its IEEE DOI names the conference.
    let mut merged = arxiv_preprint(
        "doi:10.1109/cvpr.2017.632",
        "Image-to-Image Translation with Conditional Adversarial Networks",
        &["Phillip Isola", "Jun-Yan Zhu", "Tinghui Zhou"],
        2017,
    );
    merged.work.container_title = None;
    merged.ids.push("arxiv:1611.07004".parse().unwrap());
    let cite = |venue: &str| {
        Claim::from_text(format!(
            "Phillip Isola, Jun-Yan Zhu, and Tinghui Zhou. 2017. Image-to-Image Translation with \
             Conditional Adversarial Networks. In {venue}, 5967–5976."
        ))
    };
    let iccv = assess(
        &cite("IEEE International Conference on Computer Vision (ICCV)"),
        &merged,
    );
    assert_eq!(iccv.container.agreement, Agreement::Mismatch, "{iccv:?}");
    assert!(iccv.confidence < VERIFIED, "{iccv:?}");
    let cvpr = assess(
        &cite("Proceedings of the IEEE Conference on Computer Vision and Pattern Recognition"),
        &merged,
    );
    assert!(cvpr.confidence >= VERIFIED, "{cvpr:?}");
    // h2-760: a preprint merged with another repository's copy (a DataCite
    // DOI naming no venue) can't confirm the conference cited.
    let mut batchnorm = arxiv_preprint(
        "arxiv:1502.03167",
        "Batch Normalization: Accelerating Deep Network Training by Reducing Internal \
         Covariate Shift",
        &["Sergey Ioffe", "Christian Szegedy"],
        2015,
    );
    batchnorm.work.container_title = None;
    batchnorm.ids.push("doi:10.57702/o9raffed".parse().unwrap());
    let text = "S. Ioffe and C. Szegedy, \"Batch Normalization: Accelerating Deep Network \
                Training by Reducing Internal Covariate Shift,\" in Advances in Neural \
                Information Processing Systems 28 (NeurIPS 2015), 2015.";
    let a = assess(&Claim::from_text(text), &batchnorm);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
}

#[test]
fn a_title_reworded_with_synonyms_is_not_the_records() {
    // h2-726: words swapped for synonyms, given apart.
    for (claimed, actual) in [
        (
            "Function of Insulin Resistance in Human Illness",
            "Role of Insulin Resistance in Human Disease",
        ),
        (
            "Breast cancer screening uptake and barriers among females in Hue, Vietnam: A \
             cross-sectional review.",
            "Breast cancer screening uptake and barriers among women in Hue, Vietnam: A \
             cross-sectional survey.",
        ),
    ] {
        assert!(text::replaces(claimed, actual), "{claimed}");
    }
    // The same title written otherwise is not reworded: respelt, misspelt,
    // abbreviated, a number or Greek letter spelt out, a word broken at a
    // line's end, words dropped.
    for (claimed, actual) in [
        (
            "Randomised trial of tumour necrosis factor alpha blockade vs placebo",
            "Randomized trial of tumor necrosis factor α blockade versus placebo",
        ),
        (
            "Type II diabetes in the programme: a mort ality study",
            "Type 2 diabetes in the program: a mortality study",
        ),
        (
            "Role of Insulin Resistance in Disease",
            "Role of Insulin Resistance in Human Disease",
        ),
        // A Roman numeral for a digit, years that differ by a digit aligned
        // as they are (holdout-1 #277).
        (
            "Midlife in the United States (MIDUS 2), 2004-2006",
            "National Survey of Midlife Development in the United States (MIDUS II), 2004-2006",
        ),
        (
            "Role of Insulin Resistance in Human Disease",
            "Role of Insulin Resistance in Human Disease",
        ),
        // Another title altogether isn't reworded: it differs.
        (
            "Deep residual learning for image recognition",
            "Role of Insulin Resistance in Human Disease",
        ),
    ] {
        assert!(!text::replaces(claimed, actual), "{claimed}");
    }
}

#[test]
fn a_paper_cited_at_another_meeting_is_told_by_its_subject() {
    // h3-890, h3-896, h3-912: no named conference on either side.
    let cite = |authors: &str, title: &str, venue: &str| {
        Claim::from_text(format!("{authors}. {title}. In {venue}"))
    };
    for (n, claim, record) in [
        (
            "h3-890",
            cite(
                "MacCallum, K., & Parsons, D. (2017)",
                "Evaluating a Mobile Toolkit for Designing Mobile Learning Activities",
                "Proceedings of the 2nd International Workshop on User Interface Test \
                 Automation (pp. 1–6).",
            ),
            proceedings(
                "Evaluating a Mobile Toolkit for Designing Mobile Learning Activities",
                &["Kathryn MacCallum", "David Parsons"],
                2017,
                "Proceedings of the 16th World Conference on Mobile and Contextual Learning",
                "1-6",
            ),
        ),
        (
            "h3-896",
            cite(
                "James Kennedy, Paul Baxter, and Tony Belpaeme. 2015",
                "The Robot Who Tried Too Hard",
                "The 26th International ACM SIGACCESS Conference on Computers and \
                 Accessibility (ASSETS '15). 67–74.",
            ),
            proceedings(
                "The Robot Who Tried Too Hard",
                &["James Kennedy", "Paul Baxter", "Tony Belpaeme"],
                2015,
                "Proceedings of the Tenth Annual ACM/IEEE International Conference on \
                 Human-Robot Interaction",
                "67-74",
            ),
        ),
        (
            "h3-912",
            cite(
                "Xiaoyu Wang, Eric Bier, Thomas Butkiewicz, William Ribarsky, and Wenwen Dou. \
                 2011",
                "Designing visual analytics systems for organizational environments",
                "Interaction Design and Children (IDC '11). 1–9.",
            ),
            proceedings(
                "Designing visual analytics systems for organizational environments",
                &[
                    "Xiaoyu Wang",
                    "Eric Bier",
                    "Thomas Butkiewicz",
                    "William Ribarsky",
                    "Wenwen Dou",
                ],
                2011,
                "Proceedings of the 2011 Visual Information Communication - International \
                 Symposium",
                "1-9",
            ),
        ),
    ] {
        let a = assess(&claim, &record);
        assert_eq!(a.container.agreement, Agreement::Mismatch, "{n}: {a:?}");
        assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{n}: {a:?}");
    }

    // Names compared by subject, acronyms and furniture aside.
    for (cited, actual, expected) in [
        (
            "Proceedings of the 9th International Conference on Information Communication \
             and Management (ICICM 2013). 129–130.",
            "Proceedings of the 2013 Conference on the Theory of Information Retrieval",
            Some(false),
        ),
        (
            "Fifteenth ACM Conference on Recommender Systems (RecSys '26). 1803–1820.",
            "Proceedings of the 31st International Conference on Intelligent User Interfaces",
            Some(false),
        ),
        (
            "Companion of the 2025 ACM/IEEE International Conference on Human-Robot \
             Interaction (HRI '25). 544–548.",
            "Proceedings of the 2025 International Conference on Artificial Intelligence and \
             Smart Manufacturing",
            Some(false),
        ),
        // An acronym alone, against a name with its own.
        (
            "RecSys '26",
            "CUI 2021 - 3rd Conference on Conversational User Interfaces",
            Some(false),
        ),
        // The same meeting, written otherwise.
        (
            "Proc. IEEE Int. Conf. Robot. Autom., pp. 1–6",
            "2015 IEEE International Conference on Robotics and Automation (ICRA)",
            Some(true),
        ),
        (
            "Proceedings of the 2019 CHI Conference on Human Factors in Computing Systems \
             (CHI '19). Association for Computing Machinery, New York, NY, USA, 1–12.",
            "Proceedings of the 2019 CHI Conference on Human Factors in Computing Systems",
            Some(true),
        ),
        (
            "IEEE/RSJ Int. Conf. Intell. Robots Syst. (IROS)",
            "2021 IEEE/RSJ International Conference on Intelligent Robots and Systems (IROS)",
            Some(true),
        ),
        (
            "HRI '15",
            "Proceedings of the Tenth Annual ACM/IEEE International Conference on \
             Human-Robot Interaction (HRI)",
            Some(true),
        ),
        // An acronym against a name without one: can't be told.
        (
            "ASSETS '15",
            "Proceedings of the 17th International ACM SIGACCESS Conference on Computers & \
             Accessibility",
            None,
        ),
        (
            "Proc. UIST",
            "Proceedings of the 28th Annual ACM Symposium on User Interface Software & \
             Technology",
            None,
        ),
    ] {
        assert_eq!(
            publication::proceedings_agree(cited, actual),
            expected,
            "{cited} | {actual}"
        );
    }

    // Cited at its own meeting, however written, it still verifies.
    let hri = proceedings(
        "The Robot Who Tried Too Hard",
        &["James Kennedy", "Paul Baxter", "Tony Belpaeme"],
        2015,
        "Proceedings of the Tenth Annual ACM/IEEE International Conference on Human-Robot \
         Interaction",
        "67-74",
    );
    for venue in [
        "Proceedings of the Tenth Annual ACM/IEEE International Conference on Human-Robot \
         Interaction (HRI '15). 67–74.",
        "Proc. ACM/IEEE Int. Conf. Human-Robot Interaction, pp. 67–74, 2015.",
        "HRI '15. 67–74.",
    ] {
        let claim = cite(
            "James Kennedy, Paul Baxter, and Tony Belpaeme. 2015",
            "The Robot Who Tried Too Hard",
            venue,
        );
        let a = assess(&claim, &hri);
        assert!(a.confidence >= VERIFIED, "{venue}: {a:?}");
    }
    // A journal called "Proceedings of …" is held as a journal.
    let pnas = article(
        "Neural networks and physical systems with emergent collective computational abilities",
        &["J J Hopfield"],
        1982,
        (
            "Proceedings of the National Academy of Sciences",
            "79",
            "2554-2558",
        ),
    );
    let a = assess(
        &Claim::from_text(
            "Hopfield JJ. Neural networks and physical systems with emergent collective \
             computational abilities. Proc Natl Acad Sci U S A. 1982;79(8):2554-8.",
        ),
        &pnas,
    );
    assert!(a.confidence >= VERIFIED, "{a:?}");
}

#[test]
fn a_journal_article_cited_in_a_meetings_proceedings_is_another_venue() {
    // h3-908: the HRI paper's namesake in a journal, found for a citation
    // of a symposium's proceedings.
    let journal = article(
        "Is a Robot better than Video for Initiating Remote Social Connections among Children?",
        &["Nuri Kim", "Jeonghye Han", "Wendy Ju"],
        2014,
        (
            "Journal of Institute of Control, Robotics and Systems",
            "20",
            "208-209",
        ),
    );
    let claim = Claim::from_text(
        "Kim, N., Han, J., & Ju, W. (2014). Is a robot better than video for initiating remote \
         social connections among children? In Proceedings of the 8th International Symposium \
         on Visual Information Communication and Interaction (pp. 208–209).",
    );
    let a = assess(&claim, &journal);
    assert_eq!(a.container.agreement, Agreement::Mismatch, "{a:?}");
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    // Cited in its journal, it verifies.
    let claim = Claim::from_text(
        "Kim, N., Han, J., & Ju, W. (2014). Is a robot better than video for initiating remote \
         social connections among children? Journal of Institute of Control, Robotics and \
         Systems, 20, 208–209.",
    );
    let a = assess(&claim, &journal);
    assert!(a.confidence >= VERIFIED, "{a:?}");
    // A journal called "Proceedings of …", abbreviated, isn't a meeting.
    let prsb = article(
        "Rapid evolution of a sexually selected trait",
        &["Alice Smith-Holloway"],
        2003,
        (
            "Proceedings of the Royal Society B: Biological Sciences",
            "270",
            "1234-1240",
        ),
    );
    let claim = Claim::from_text(
        "Smith-Holloway A. Rapid evolution of a sexually selected trait. In Proc. R. Soc. B \
         270, 1234–1240 (2003).",
    );
    let a = assess(&claim, &prsb);
    assert!(a.confidence >= VERIFIED, "{a:?}");
}

/// A book chapter's record, in no volume: its book as the container.
fn chapter(title: &str, authors: &[&str], year: i16, book: &str) -> SourceRecord {
    let mut r = record(&[], title, authors, year);
    r.work.work_type = Some(crate::work::WorkType::Chapter);
    r.work.container_title = Some(book.into());
    r
}

#[test]
fn a_chapter_cited_as_a_journals_article_is_plausible() {
    let methane = chapter(
        "Methane Cycling and Climate",
        &["Han Dolman"],
        2019,
        "Biogeochemical Cycles and Climate",
    );
    let psychotherapy = chapter(
        "Integrative Psychotherapy with Children",
        &["Athena A. Drewes", "John W. Seymour"],
        2019,
        "Handbook of Psychotherapy Integration",
    );
    let culpability = chapter(
        "Law, Neuroscience, and Criminal Culpability",
        &["Lisa Claydon"],
        2011,
        "Law and Neuroscience",
    );
    let structured = Claim {
        title: Some("Law, Neuroscience, and Criminal Culpability".into()),
        authors: vec!["Lisa Claydon".into()],
        year: Some(2011),
        container: Some("Journal of Neuroscience".into()),
        volume: Some("60".into()),
        page: Some("141-170".into()),
        ..Claim::default()
    };
    for (n, claim, record, venue) in [
        // The book's title as the journal, with a volume and issue.
        (
            "h3-947",
            Claim::from_text(
                "Dolman H. Methane Cycling and Climate. Biogeochemical Cycles and Climate. \
                 2019;23(5):159-175.",
            ),
            &methane,
            Agreement::Match,
        ),
        (
            "h3-948 (APA)",
            Claim::from_text(
                "Dolman, H. (2019). Methane Cycling and Climate. Biogeochemical Cycles and \
                 Climate, 23(5), 159–175.",
            ),
            &methane,
            Agreement::Match,
        ),
        // A real journal of the field.
        (
            "h3-964",
            Claim::from_text(
                "Drewes AA, Seymour JW. Integrative Psychotherapy with Children. Psychotherapy \
                 Research. 2019;54(12):341-356.",
            ),
            &psychotherapy,
            Agreement::Mismatch,
        ),
        ("h3-959", structured, &culpability, Agreement::Mismatch),
    ] {
        let a = assess(&claim, record);
        assert_eq!(a.container.agreement, venue, "{n}: {a:?}");
        assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{n}: {a:?}");
    }
    // Cited as a chapter, or in its series' volume, it verifies.
    let mut lncs = chapter(
        "U-Net: Convolutional Networks for Biomedical Image Segmentation",
        &["Olaf Ronneberger", "Philipp Fischer", "Thomas Brox"],
        2015,
        "Lecture Notes in Computer Science",
    );
    lncs.work.page = Some("234-241".into());
    for (claim, record) in [
        (
            "Dolman H. Methane Cycling and Climate. In: Dolman H, ed. Biogeochemical Cycles and \
             Climate. Oxford University Press; 2019:159-175.",
            &methane,
        ),
        (
            "Ronneberger O, Fischer P, Brox T. U-Net: Convolutional Networks for Biomedical \
             Image Segmentation. Lecture Notes in Computer Science, vol. 9351, pp. 234–241. \
             Springer, 2015.",
            &lncs,
        ),
    ] {
        let a = assess(&Claim::from_text(claim), record);
        assert!(a.confidence >= VERIFIED, "{claim}: {a:?}");
    }
    // Nor does the book, its title where the journal stands.
    let mut book = record(
        &[],
        "Biogeochemical Cycles and Climate",
        &["Han Dolman"],
        2019,
    );
    book.work.work_type = Some(crate::work::WorkType::Book);
    let a = assess(
        &Claim::from_text(
            "Dolman H. Methane Cycling and Climate. Biogeochemical Cycles and Climate. \
             2019;23(5):159-175.",
        ),
        &book,
    );
    assert!(a.confidence < VERIFIED, "{a:?}");
    let a = assess(
        &Claim::from_text("Dolman H. Biogeochemical Cycles and Climate. Oxford: OUP; 2019."),
        &book,
    );
    assert!(a.confidence >= VERIFIED, "{a:?}");
    assert!(publication::cites_issue(
        "Psychotherapy Research. 2019;54(12):341-356."
    ));
    assert!(publication::cites_issue(
        "Psychotherapy Research, 54(12), 341–356."
    ));
    assert!(!publication::cites_issue("Injury 47 (2016) 797–804"));
    assert!(!publication::cites_issue("N Engl J Med. 2017;376:2545-55."));
}

#[test]
fn an_identified_work_with_other_authors_is_plausible() {
    // h3-138: the record's byline garbled into one name, cited as the
    // record gives it; h3-622: every author invented. The DOI, title and
    // place name the work: plausible, never verified, nor a mismatch.
    let mut ruff = placed(
        &["doi:10.1080/026990599120963"],
        "In search of a unified definition for mild traumatic brain injury",
        &["Ronald M. Ruff, Paul Jurica"],
        1999,
        ("Brain Injury", "13", "943-952"),
    );
    ruff.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let claim = |authors: &[&str]| Claim {
        title: Some("In search of a unified definition for mild traumatic brain injury".into()),
        authors: authors.iter().map(|a| (*a).to_owned()).collect(),
        container: Some("Brain Injury".into()),
        volume: Some("13".into()),
        page: Some("943-952".into()),
        ids: vec!["doi:10.1080/026990599120963".parse().unwrap()],
        ..Claim::default()
    };
    for authors in [
        &["Ruff, Paul Jurica, Ronald M"][..],
        &["Mervimbe, Maren L."],
    ] {
        let a = assess(&claim(authors), &ruff);
        assert!(
            (PLAUSIBLE..VERIFIED).contains(&a.confidence),
            "{authors:?}: {a:?}"
        );
    }
    // Without the place, other authors still differ.
    let mut bare = claim(&["Mervimbe, Maren L."]);
    bare.volume = None;
    bare.page = None;
    assert!(assess(&bare, &ruff).confidence < PLAUSIBLE);
}

#[test]
fn a_journals_words_in_the_title_are_the_titles() {
    // h3-594: a reworded title whose words are the journal's too, at the
    // work's place with its DOI, was read as no title at all.
    let mut gh = placed(
        &["doi:10.1016/s1096-6374(02)00018-7"],
        "The role of growth hormone in fetal development",
        &["Michael J. Waters", "P. L. Kaye"],
        2002,
        ("Growth Hormone & IGF Research", "12", "137-146"),
    );
    gh.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let reworded = Claim::from_text(
        "27. Waters MJ, Kaye PL. The contribution of growth hormone in fetal design. Growth \
         Hormone & IGF Research. 2002;12(3):137-146. doi:10.1016/s1096-6374(02)00018-7",
    );
    let a = assess(&reworded, &gh);
    assert_ne!(a.title.agreement, Agreement::Unknown, "{a:?}");
    assert!(a.confidence < VERIFIED, "{a:?}");
    // Without a title, the journal's words are the journal's.
    let titleless =
        Claim::from_text("Waters MJ, Kaye PL. Growth Hormone & IGF Research. 2002;12(3):137-146.");
    let a = assess(&titleless, &gh);
    assert!(a.confidence >= VERIFIED, "{a:?}");
}
