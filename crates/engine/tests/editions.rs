//! `verify` of a book cited by its ISBN, beside another edition of it:
//! wild-4's model-written references.

mod common;

use std::sync::Arc;

use aspero_core::{Claim, IdKind, SourceRecord, WorkType};
use aspero_engine::{Engine, Reason, Verdict};
use common::*;

/// One source that looks up ISBNs and searches titles, as Open Library.
fn engine_with(works: Vec<SourceRecord>) -> Engine {
    let mut source = Scripted::new("openlibrary").keyword().titles().with(works);
    source.caps.lookup_kinds.push(IdKind::Isbn);
    engine(vec![Arc::new(source)])
}

fn book(ids: &[&str], title: &str, authors: &[&str], year: i16) -> SourceRecord {
    let mut r = work("openlibrary", ids, title, authors, year);
    r.work.work_type = Some(WorkType::Book);
    r
}

const PHARMACOLOGY: &str = "Principles of Clinical Pharmacology";

/// The 3rd edition's record, its first author listed last, with a suffix.
fn third_edition() -> SourceRecord {
    book(
        &["isbn:9780123854711"],
        PHARMACOLOGY,
        &[
            "Shiew-Mei Huang",
            "Juan J.L. Lertora",
            "Sanford P. Markey",
            "Arthur J. Atkinson  Jr.",
        ],
        2012,
    )
}

fn second_edition() -> SourceRecord {
    book(
        &["isbn:9780123694171"],
        PHARMACOLOGY,
        &["Arthur J. Atkinson"],
        2007,
    )
}

/// wild-4 #325: the ISBN's record has the title and year cited; another
/// edition matching the byline better is no reason to call the ISBN
/// another work's.
#[tokio::test]
async fn an_isbn_whose_record_has_the_title_and_year_cited_is_not_another_editions() {
    let engine = engine_with(vec![third_edition(), second_edition()]);
    let claim = Claim::from_text(format!(
        "{PHARMACOLOGY} | Atkinson et al. | 3rd (2012) | 978-0123854711"
    ));

    let v = engine.verify(&claim, 5).await;
    assert_ne!(v.verdict, Verdict::Mismatch, "{}", v.reason_text);
    let best = &v.report.candidates[v.best_match.unwrap()];
    assert_eq!(
        best.record.ids,
        ["isbn:9780123854711".parse::<aspero_core::WorkId>().unwrap()]
    );
    // The other edition was found, and passed over.
    assert!(
        v.report
            .candidates
            .iter()
            .any(|c| c.record.ids == second_edition().ids),
        "{:?}",
        v.report.candidates
    );
}

/// Another author's book of the same title and year, at the ISBN cited,
/// is still another work: the citation's book is the one its byline
/// matches.
#[tokio::test]
async fn an_isbn_of_another_authors_book_of_the_same_title_is_a_mismatch() {
    let engine = engine_with(vec![
        book(
            &["isbn:9780123854711"],
            PHARMACOLOGY,
            &[
                "Shiew-Mei Huang",
                "Juan J.L. Lertora",
                "Sanford P. Markey",
                "Peter Jones",
            ],
            2012,
        ),
        book(
            &["isbn:9780123694171"],
            PHARMACOLOGY,
            &["Arthur J. Atkinson"],
            2012,
        ),
    ]);
    let claim = Claim::from_text(format!(
        "{PHARMACOLOGY} | Atkinson et al. | 3rd (2012) | 978-0123854711"
    ));

    let v = engine.verify(&claim, 5).await;
    assert_eq!(v.verdict, Verdict::Mismatch, "{}", v.reason_text);
    assert!(
        matches!(v.reason, Reason::IdElsewhere { .. }),
        "{:?}",
        v.reason
    );
}
