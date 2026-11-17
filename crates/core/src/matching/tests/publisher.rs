//! A book's publisher (holdout-6, #140): another publisher alone is an
//! imprint or a reissue; with another year too, it may be another edition.

use super::*;
use crate::work::WorkType;

fn book(title: &str, authors: &[&str], year: i16, publisher: &str) -> SourceRecord {
    let mut r = record(&[], title, authors, year);
    r.work.work_type = Some(WorkType::Book);
    r.work.publisher = Some(publisher.into());
    r
}

fn cronin() -> SourceRecord {
    book(
        "Colour and Experience in Nineteenth-Century Poetry",
        &["Richard Cronin"],
        1988,
        "Palgrave Macmillan UK",
    )
}

const COLOUR: &str = "Colour and Experience in Nineteenth-Century Poetry";

fn verifies(claim: &Claim, record: &SourceRecord) -> Assessment {
    let a = assess(claim, record);
    assert!(a.confidence >= VERIFIED, "{claim:?}: {a:?}");
    a
}

fn plausible(claim: &Claim, record: &SourceRecord) -> Assessment {
    let a = assess(claim, record);
    assert!(
        (PLAUSIBLE..VERIFIED).contains(&a.confidence),
        "{claim:?}: {a:?}"
    );
    a
}

#[test]
fn another_publisher_alone_is_an_imprint_or_a_reissue() {
    for text in [
        format!("Cronin, Richard. 1988. {COLOUR}. Routledge."),
        format!("Cronin, R. (1988). {COLOUR}. Oxford University Press."),
        format!("Cronin R. {COLOUR}. London: Routledge; 1988."),
    ] {
        verifies(&Claim::from_text(text), &cronin());
    }
}

#[test]
fn another_publisher_and_year_may_be_another_edition() {
    for text in [
        // A year off by one, which alone verifies.
        format!("Cronin, R. (1989). {COLOUR}. Oxford University Press."),
        format!("Cronin R. {COLOUR}. London: Routledge; 1989."),
        format!("Cronin, Richard. {COLOUR}. New York: Garland, 1989."),
        // Two or more off: plausible already, as another edition.
        format!("Cronin, R. (1995). {COLOUR}. Cambridge University Press."),
    ] {
        let a = plausible(&Claim::from_text(text), &cronin());
        assert_eq!(a.publisher.agreement, Agreement::Mismatch, "{a:?}");
    }
    // As fields, and in a BibTeX entry.
    let claim = Claim {
        title: Some(COLOUR.into()),
        authors: vec!["Cronin, R.".into()],
        year: Some(1989),
        publisher: Some("Elsevier".into()),
        ..Claim::default()
    };
    plausible(&claim, &cronin());
    let bibtex = format!(
        "@book{{cronin, author = {{Cronin, Richard}}, title = {{{COLOUR}}}, \
         publisher = {{Elsevier}}, year = {{1989}}}}"
    );
    plausible(&Claim::from_text(bibtex), &cronin());
}

#[test]
fn a_year_alone_keeps_todays_verdict() {
    // The record's publisher, however written, and a year off by one.
    for text in [
        format!("Cronin, R. (1989). {COLOUR}. Palgrave Macmillan."),
        format!("Cronin, R. (1989). {COLOUR}. Palgrave."),
        format!("Cronin, R. (1989). {COLOUR}."),
    ] {
        let a = verifies(&Claim::from_text(text), &cronin());
        assert_ne!(a.publisher.agreement, Agreement::Mismatch, "{a:?}");
    }
}

#[test]
fn publishers_compare_by_their_names_not_their_furniture() {
    let rows = [
        ("Routledge, Taylor & Francis Group", "Routledge"),
        ("Oxford University Press", "OUP"),
        ("Oxford University Press", "Oxford UP"),
        ("Oxford University Press (OUP)", "Oxford Univ. Press"),
        (
            "University of Chicago Press",
            "The University of Chicago Press",
        ),
        ("Springer International Publishing", "Springer"),
        ("Springer Science and Business Media LLC", "Springer-Verlag"),
        ("The Macmillan Company", "Macmillan"),
        ("Wiley-Blackwell", "John Wiley & Sons"),
    ];
    for (record, cited) in rows {
        assert!(
            !crate::matching::publishers_differ(cited, record),
            "{cited} vs {record}"
        );
    }
    for (record, cited) in [
        ("Palgrave Macmillan UK", "Routledge"),
        ("Springer International Publishing", "Elsevier"),
        ("Oxford University Press", "Cambridge University Press"),
        ("CRC Press", "Wiley"),
    ] {
        assert!(
            crate::matching::publishers_differ(cited, record),
            "{cited} vs {record}"
        );
    }
}

#[test]
fn a_reissue_with_the_years_of_its_editions_verifies() {
    // Open Library's work: any edition's year is cited, from any publisher.
    let mut r = book(
        "Field projects in anthropology",
        &["Julia G. Crane"],
        1974,
        "Longman",
    );
    r.work.other_years = vec![1984, 1992];
    let text = "Crane, J. G. (1992). Field projects in anthropology: a student handbook. \
                Waveland Press.";
    verifies(&Claim::from_text(text), &r);
}

#[test]
fn a_publisher_is_read_only_after_the_title() {
    // "Press" in the title is no publisher.
    let r = book(
        "The Press and the Public in Victorian Britain",
        &["Jane Smith"],
        2001,
        "Routledge",
    );
    let text = "Smith J. The Press and the Public in Victorian Britain. Routledge; 2002.";
    let a = verifies(&Claim::from_text(text), &r);
    assert_ne!(a.publisher.agreement, Agreement::Mismatch, "{a:?}");
}

#[test]
fn never_a_mismatch_nor_for_articles() {
    let mut r = cronin();
    let text = format!("Cronin, R. (1989). {COLOUR}. Oxford University Press.");
    assert!(assess(&Claim::from_text(&text), &r).confidence >= PLAUSIBLE);
    // A journal's article has no publisher to hold against it.
    r.work.work_type = Some(WorkType::JournalArticle);
    assert_eq!(
        assess(&Claim::from_text(&text), &r).publisher.agreement,
        Agreement::Unknown
    );
}
