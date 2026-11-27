//! Red-team wild-6: books and reports not found, and flawed citations of
//! real works that scored as none. A book's title read from its layout
//! with its edition, its translators or authors after it, or an editor's
//! byline; a report cited without its organisation; a title quoted by
//! its opening words or with a word put in.

use super::super::record_title::record_title;
use super::*;

fn assert_titles(cases: &[(&str, &str)]) {
    for (text, title) in cases {
        assert_eq!(probable_title(text).as_deref(), Some(*title), "{text}");
    }
}

fn book(title: &str, authors: &[&str], year: i16) -> SourceRecord {
    let mut r = record(&[], title, authors, year);
    r.work.work_type = Some(crate::work::WorkType::Book);
    r
}

#[test]
fn a_books_edition_authors_and_translators_are_no_part_of_its_title() {
    assert_titles(&[
        // wild-6 #92: "4th ed." is no editors' "eds", nor the title's.
        (
            "Miller, W. R., & Rollnick, S. Motivational Interviewing: Helping People Change and \
             Grow, 4th ed. (Guilford Press, 2023).",
            "Motivational Interviewing: Helping People Change and Grow",
        ),
        // wild-6 #247: the authors named after the title.
        (
            "Introducing Palliative Care by Richard Twycross and Andrew Wilcock.",
            "Introducing Palliative Care",
        ),
    ]);
    // Editors' names still end no title (wild-5's "Ragep, eds").
    assert_ne!(
        probable_title("Jamil Ragep, eds. Tradition, Transmission, Transformation.").as_deref(),
        Some("Jamil Ragep, eds")
    );
    // "by" before words that aren't people's names, or more after them,
    // is the title's.
    assert_titles(&[(
        "Smith, J. (2019). Learning by doing in the laboratory. Oxford: Clarendon, 2019.",
        "Learning by doing in the laboratory",
    )]);
    // A translator's initials no longer split the reference: the title
    // isn't "translated by J" (wild-6 #703).
    assert_ne!(
        probable_title(
            "Ptolemy, Geography, translated by J. Lennart Berggren and Alexander Jones (2000)."
        )
        .as_deref(),
        Some("translated by J")
    );
}

#[test]
fn a_bylines_name_before_a_comma_is_no_part_of_the_title() {
    assert_titles(&[
        // wild-6 #265: a family name alone, before a title with an article.
        (
            "Snowdon, The Spirit Level Delusion (2010).",
            "The Spirit Level Delusion",
        ),
        // wild-6 #347: an editor's byline.
        (
            "James Gairdner, ed., The Paston Letters, 1422–1509 (6 vols, 1872–75)",
            "The Paston Letters, 1422–1509",
        ),
        // wild-6 #519: a note's place, publisher and year in brackets,
        // and its page after them.
        (
            "François Blancpain, Un siècle de relations financières entre Haïti et la France \
             (1825–1922) (Paris: L'Harmattan, 2001), [page].",
            "Un siècle de relations financières entre Haïti et la France",
        ),
        // wild-6 #677: a title's first part ends in a full stop.
        (
            "Beccaria, Gian Luigi, Italiano. Antico e nuovo, Milano, Garzanti, 1988.",
            "Italiano. Antico e nuovo",
        ),
        // wild-6 #217: organisations listed before a colon.
        (
            "HPA / College of Radiographers / RCR: Protection of Pregnant Patients during \
             Diagnostic Medical Exposures to Ionising Radiation (2009, report RCE-9).",
            "Protection of Pregnant Patients during Diagnostic Medical Exposures to Ionising \
             Radiation",
        ),
    ]);
    // A title's own acronym before its colon stays.
    assert_titles(&[(
        "Devlin J, Chang MW. BERT: Pre-training of Deep Bidirectional Transformers for Language \
         Understanding. NAACL 2019.",
        "BERT: Pre-training of Deep Bidirectional Transformers for Language Understanding",
    )]);
}

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

/// wild-6 #600: an organisation's report found by its whole title, cited
/// without the organisation: no other byline.
#[test]
fn an_organisations_report_cited_without_it_names_no_one_else() {
    let memo = book(
        "From Swimming in Sand to High and Sustainable Growth",
        &["World Bank Group"],
        2022,
    );
    let text = "Pakistan Country Economic Memorandum: From Swimming in Sand to High and \
                Sustainable Growth | 2022 | Covers productivity and informality constraints.";
    let a = assess(&Claim::from_text(text), &memo);
    assert_eq!(a.authors.agreement, Agreement::Unknown, "{a:?}");
    assert!(
        a.confidence >= PLAUSIBLE && a.confidence < VERIFIED,
        "{a:?}"
    );
    // A person named is still another byline.
    let text = "Smith, J. (2022). From Swimming in Sand to High and Sustainable Growth.";
    let a = assess(&Claim::from_text(text), &memo);
    assert_eq!(a.authors.agreement, Agreement::Mismatch, "{a:?}");
    // Nor is another country's memorandum, its title only in part, the
    // organisation's absence.
    let other = book(
        "Zimbabwe Country Economic Memorandum",
        &["World Bank"],
        2022,
    );
    let text = "Pakistan Country Economic Memorandum: From Swimming in Sand to High and \
                Sustainable Growth | 2022";
    let a = assess(&Claim::from_text(text), &other);
    assert!(a.confidence < PLAUSIBLE, "{a:?}");
}

/// wild-6 #347: a library's era marker in a record's title ("A.D.") is
/// no word the citation lacks.
#[test]
fn an_era_marker_beside_a_year_is_no_word_of_the_title() {
    let paston = book(
        "The Paston letters, A.D. 1422-1509",
        &["James Gairdner"],
        1904,
    );
    let text = "James Gairdner, ed., The Paston Letters, 1422–1509 (6 vols, 1872–75)";
    let a = assess(&Claim::from_text(text), &paston);
    assert_eq!(a.title.agreement, Agreement::Match, "{a:?}");
    // Another edition: plausible, the year a difference.
    assert!(
        a.confidence >= PLAUSIBLE && a.confidence < VERIFIED,
        "{a:?}"
    );
    assert_eq!(
        record_title(
            "The Paston letters 1422-1509 A.D",
            "The Paston Letters",
            true
        ),
        "The Paston letters 1422-1509"
    );
    assert_eq!(
        record_title("Paston Letters, A. D. 1422-1509", "Paston Letters", true),
        "Paston Letters, 1422-1509"
    );
    // Kept where the citation gives it, or not beside a year.
    assert_eq!(
        record_title(
            "The Viking-age gold and silver of Scotland, AD 850-1100",
            "The Viking-age gold and silver of Scotland (AD 850–1100)",
            true
        ),
        "The Viking-age gold and silver of Scotland, AD 850-1100"
    );
    assert_eq!(
        record_title("AD and ADHD in adults", "AD and ADHD", false),
        "AD and ADHD in adults"
    );
}

/// wild-6 #464: a quoted title that is the work's main title with one word
/// put in place of one: partly the work's, plausible beside its authors,
/// year and journal.
#[test]
fn a_quoted_main_title_with_a_word_replaced_is_partly_the_works() {
    let coal = article(
        "The political economy of coal in Poland: Drivers and barriers for a shift away from \
         fossil fuels",
        &["Hanna Brauers", "Pao-Yu Oei"],
        2020,
        ("Energy Policy", "144", "111621"),
    );
    let text = "Brauers, H. und Oei, P.-Y. (2020) 'The political economy of coal in Germany', Energy Policy.";
    let a = assess(&Claim::from_text(text), &coal);
    assert_eq!(a.title.agreement, Agreement::Partial, "{a:?}");
    assert!(
        a.confidence >= PLAUSIBLE && a.confidence < VERIFIED,
        "{a:?}"
    );
    let main = "The political economy of coal in Poland: Drivers and barriers";
    assert!(super::super::titles::main_title_replaced(
        "The political economy of coal in Germany",
        main
    ));
    // Not two words, a number, a title without a subtitle, nor a short one.
    for cited in [
        "The political economy of oil in Germany",
        "The political economy of coal in 2019",
        "The political economy of coal",
    ] {
        assert!(
            !super::super::titles::main_title_replaced(cited, main),
            "{cited}"
        );
    }
    assert!(!super::super::titles::main_title_replaced(
        "The political economy of coal in Germany",
        "The political economy of coal in Poland"
    ));
    assert!(!super::super::titles::main_title_replaced(
        "Deep thinking",
        "Deep learning: a review"
    ));
}
