//! A verified book checked against its other records: the year its
//! citation says it first appeared (holdout-6's `book-edition-year`).
//!
//! A citation saying its book first appeared in a year ("(Original work
//! published 1969)", "(1st ed.)") gives a later edition's year when a
//! record of the same book among the candidates (same title and authors)
//! holds that edition and one more than three years earlier. Free: no
//! request.

use aspero_core::matching;
use aspero_core::{Agreement, Claim, WorkType};

use crate::resolve::Candidate;
use crate::verdict::{CitedField, Difference, Verdict, Verification, demote};

/// Checks a verified book's first year (see the module docs), making it
/// plausible when its records disagree.
pub(crate) fn check_edition(claim: &Claim, v: &mut Verification) {
    if v.verdict != Verdict::Verified {
        return;
    }
    let Some(best) = v.best_match else { return };
    if let Some(year) = first_year_differs(claim, &v.report.candidates, best) {
        demote(claim, v, vec![year]);
    }
}

/// Whether a candidate is a record of the book cited: a book whose title
/// and authors match.
fn same_book(c: &Candidate) -> bool {
    c.record.work.work_type == Some(WorkType::Book)
        && c.assessment.title.agreement == Agreement::Match
        && c.assessment.authors.agreement == Agreement::Match
}

/// The year the citation says its book first appeared, against a record
/// of the book among the candidates (same title and authors) that holds
/// the edition cited and one more than a few years earlier
/// ([`matching::first_year_contradicted`]): a later edition's year given.
fn first_year_differs(claim: &Claim, candidates: &[Candidate], best: usize) -> Option<Difference> {
    if candidates[best].assessment.title.agreement != Agreement::Match {
        return None;
    }
    let (first, earliest) = candidates
        .iter()
        .filter(|c| same_book(c))
        .find_map(|c| matching::first_year_contradicted(claim, &c.record))?;
    Some(Difference {
        field: CitedField::Year,
        actual: Some(earliest.to_string()),
        cited: Some(first.to_string()),
        kind: None,
    })
}
