//! A verified book checked against its other records: the year its
//! citation says it first appeared. Cases from holdout-6's
//! `book-edition-year`.

mod common;

use std::sync::Arc;

use aspero_core::{Claim, Person, WorkType};
use aspero_engine::{CitedField, Reason, Verdict};
use common::*;

fn book(
    doi: &str,
    title: &str,
    authors: &[(&str, &str)],
    year: i16,
    container: Option<&str>,
) -> aspero_core::SourceRecord {
    let mut r = work("crossref", &[doi], title, &[], year);
    r.work.authors = authors
        .iter()
        .map(|(given, family)| {
            let mut p = Person::default();
            p.given = Some((*given).into());
            p.family = Some((*family).into());
            p
        })
        .collect();
    r.work.work_type = Some(WorkType::Book);
    r.work.container_title = container.map(str::to_owned);
    r
}

#[tokio::test]
async fn a_later_editions_year_given_as_the_originals_is_plausible() {
    // holdout-6 #486: Crossref's record of the 1959 third edition
    // verifies the year cited as the original's, but the book's record
    // beside it was out in 1940.
    let third = book(
        "10.1037/14685-000",
        "The psychology of exceptional children (3rd ed.).",
        &[("Karl C.", "Garrison"), ("Dewey G.", "Force")],
        1959,
        None,
    );
    let mut first = work(
        "openlibrary",
        &[],
        "The psychology of exceptional children",
        &["Karl Claudius Garrison", "Dewey George Force"],
        1940,
    );
    first.work.work_type = Some(WorkType::Book);
    first.work.other_years = vec![1943, 1959, 1965];
    let crossref = Arc::new(Scripted::new("crossref").matcher(vec![third, first]));
    let engine = common::engine(vec![crossref]);
    let text = "Garrison, K. C., & Force, D. G. (1965). *The psychology of exceptional \
                children*. Ronald Press Co. (Original work published 1959)";
    let v = engine.verify(&Claim::from_text(text), 5).await;
    assert_eq!(v.verdict, Verdict::Plausible, "{}", v.reason_text);
    let Reason::PartlyMatches { differs } = &v.reason else {
        panic!("{:?}", v.reason);
    };
    assert_eq!(differs[0].field, CitedField::Year);
    assert_eq!(differs[0].actual.as_deref(), Some("1940"));
    assert_eq!(differs[0].cited.as_deref(), Some("1959"));

    // The original's year as its first edition's: the work.
    let text = "Garrison, K. C., & Force, D. G. (1965). *The psychology of exceptional \
                children*. Ronald Press Co. (Original work published 1940)";
    let v = engine.verify(&Claim::from_text(text), 5).await;
    assert_eq!(v.verdict, Verdict::Verified, "{}", v.reason_text);
}

#[tokio::test]
async fn a_record_without_the_edition_cited_says_nothing_of_the_first() {
    // holdout-6 #882: Open Library's work of the 1934 book with its 1983
    // reprint, beside another of its works dated 1926 that holds no
    // edition of 1934 (misdated, or another book's).
    let ol = |years: (i16, &[i16])| {
        let mut r = work(
            "openlibrary",
            &[],
            "The philosophy of Spinoza",
            &["Harry Austryn Wolfson"],
            years.0,
        );
        r.work.work_type = Some(WorkType::Book);
        r.work.other_years = years.1.to_vec();
        r
    };
    let crossref =
        Arc::new(Scripted::new("crossref").matcher(vec![ol((1934, &[1983])), ol((1926, &[]))]));
    let engine = common::engine(vec![crossref]);
    let text = "Wolfson, H. A. (1983). *The philosophy of Spinoza*. Harvard University Press. \
                (Original work published 1934)";
    let v = engine.verify(&Claim::from_text(text), 5).await;
    assert_eq!(v.verdict, Verdict::Verified, "{}", v.reason_text);
}
