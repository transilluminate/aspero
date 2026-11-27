//! Red-team wild-6: books and reports not found, and flawed citations of
//! real works that scored as none. A book's title read from its layout
//! with its edition, its translators or authors after it, or an editor's
//! byline; a report cited without its organisation; a title quoted by
//! its opening words or with a word put in.

use super::super::record_title::record_title;
use super::*;

fn book(title: &str, authors: &[&str], year: i16) -> SourceRecord {
    let mut r = record(&[], title, authors, year);
    r.work.work_type = Some(crate::work::WorkType::Book);
    r
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
