//! A book's first edition, as a citation states it: "(Original work
//! published 1969)", "(1st ed.)".
//!
//! A reprint cited with its original's year is the same work, and the
//! book's record knows its editions' years ([`crate::Work::other_years`],
//! Open Library's). A year given as the original's, or as the first
//! edition's, that is an edition's the record holds, with one more than
//! three years earlier, is a later edition's year: the citation's error
//! (holdout-6's `book-edition-year`), though the edition cited exists.

use std::ops::Range;

use super::Claim;
use super::text::Normalised;
use crate::record::SourceRecord;

/// The year `claim` says its work, `record`'s, first appeared: the year
/// after "original work published", "originally published" or "first
/// published", else, beside "1st ed." or "first edition", the earliest
/// year cited. `None` when it says nothing of it.
pub fn first_year_cited(claim: &Claim, record: &SourceRecord) -> Option<i16> {
    let text = Normalised::new(claim.text.as_deref()?);
    let title = record.work.title.as_deref();
    let span = title.and_then(|t| text.title_span(t));
    let cited = claim.cited_years_beside(title);
    claimed_first_year(&text, span.as_ref(), &cited)
}

/// The year a reference says its work first appeared: the year after
/// "original work published", "originally published" or "first
/// published" ("in" between allowed); else, for a reference citing its
/// first edition ("1st ed.", "1st edn", "first edition"), the earliest
/// year it cites (`cited`). Words of the title (at `title`) don't count:
/// a book may be titled "… : First Edition".
pub(super) fn claimed_first_year(
    text: &Normalised,
    title: Option<&Range<usize>>,
    cited: &[i16],
) -> Option<i16> {
    let words: Vec<&str> = text
        .words
        .iter()
        .enumerate()
        .map(|(i, w)| {
            if title.is_some_and(|t| t.contains(&i)) {
                ""
            } else {
                w.as_str()
            }
        })
        .collect();
    let at = |i: usize| words.get(i).copied().unwrap_or_default();
    for i in 0..words.len() {
        let after = match (at(i), at(i + 1), at(i + 2)) {
            ("original", "work", "published") => i + 3,
            ("originally" | "first", "published", _) => i + 2,
            _ => continue,
        };
        let year = if at(after) == "in" { after + 1 } else { after };
        if let Some(year) = Some(at(year))
            .filter(|y| y.len() == 4)
            .and_then(|y| y.parse::<i16>().ok())
        {
            return Some(year);
        }
    }
    let first_edition = words.windows(2).any(|pair| {
        matches!(pair[0], "1st" | "first") && matches!(pair[1], "ed" | "edn" | "edition")
    });
    first_edition.then(|| cited.iter().copied().min()).flatten()
}

/// Years within which a book's first edition may be dated otherwise: its
/// UK and US editions, its copyright and printing years, a catalogue's
/// slip.
const FIRST_YEAR_SLACK: i32 = 3;

/// The earliest of `actual`, a book's editions' years (its record's year
/// and other editions'), when the citation's `first`, the year it says
/// the book first appeared, is that of an edition the record holds (or
/// one year off it), and the earliest is more than [`FIRST_YEAR_SLACK`]
/// years before: the year given is a later edition's. A record that
/// doesn't hold the edition cited may be another book's, or misdated.
pub(super) fn earlier_edition(first: i16, actual: &[i16]) -> Option<i16> {
    let holds = actual.iter().any(|y| y.abs_diff(first) <= 1);
    let earliest = actual.iter().copied().min()?;
    (holds && i32::from(earliest) + FIRST_YEAR_SLACK < i32::from(first)).then_some(earliest)
}

/// The year `claim` says its book first appeared, and the earliest
/// edition of `record`'s (its year and other editions'), when that is a
/// later edition's year: one the record holds, with one more than three
/// years earlier.
pub fn first_year_contradicted(claim: &Claim, record: &SourceRecord) -> Option<(i16, i16)> {
    let first = first_year_cited(claim, record)?;
    let w = &record.work;
    let years: Vec<i16> = w
        .issued
        .map(|d| d.year())
        .into_iter()
        .chain(w.other_years.iter().copied())
        .collect();
    Some((first, earlier_edition(first, &years)?))
}
