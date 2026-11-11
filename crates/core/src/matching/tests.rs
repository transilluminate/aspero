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
    let right = assess(
        &claim,
        &record(&["10.1038/nature14539"], "Deep learning", &authors, 2015),
    );
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

    // Structured: the journal is compared by abbreviation rules; a
    // different one lowers the ranking without settling the verdict.
    let claim = Claim {
        title: Some("Fibrinolysis for patients with intermediate-risk pulmonary embolism".into()),
        authors: vec!["Meyer G".into()],
        year: Some(2014),
        container: Some("Lancet".into()),
        ..Claim::default()
    };
    let a = assess(&claim, &peitho());
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
