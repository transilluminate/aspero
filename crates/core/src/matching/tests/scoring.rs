//! Combining the fields into a confidence: corroboration, single-field and
//! no-evidence caps, and the basics of a match against its lookalikes.

use super::*;

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
    assert!(crate::matching::authors::names_given(
        "SMITH JA, JONES B. A title. 2020."
    ));
    assert!(crate::matching::authors::names_given(
        "DUPONT, J. Un titre. 2020."
    ));
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
