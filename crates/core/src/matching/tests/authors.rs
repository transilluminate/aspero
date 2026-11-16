//! Authors: invented co-authors (strangers), bylines that name others, and
//! a record's own authors however written.

use super::*;

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

#[test]
fn a_stranger_among_names_in_a_script_without_capitals_is_seen() {
    // h3-646: an invented co-author among Arabic names, which can't be
    // told a person's by their capitals: the byline was not taken as
    // complete, so the stranger passed.
    let mut r = record(
        &["doi:10.59743/jaf.v8i2.405"],
        "تأثير هرمونات الغدة الدرقية وفيتامين د والسكر التراكمي على متلازمة تكيس المبايض",
        &[
            "امحمد عثمان فيدان",
            "سليمان مصطفى الأطيرش",
            "أسماء محمد القنيدي",
            "رنيم محمد الأطرش",
            "ريان علي بيت المال",
        ],
        2024,
    );
    r.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let claim = |authors: &[&str]| Claim {
        title: r.work.title.clone(),
        authors: authors.iter().map(|a| (*a).to_owned()).collect(),
        year: Some(2024),
        ..Claim::default()
    };
    let named = [
        "فيدان, ا. ع.",
        "سليمان مصطفى الأطيرش",
        "القنيدي, أ. م.",
        "الأطرش, ر. م.",
        "بيت المال, ر. ع.",
    ];
    assert!(assess(&claim(&named), &r).confidence >= VERIFIED);
    let mut stranger = named.to_vec();
    stranger.insert(4, "Paldekwe, B. L.");
    let a = assess(&claim(&stranger), &r);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
}
