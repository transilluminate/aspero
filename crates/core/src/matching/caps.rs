//! The policy numbers [`super::assess()`] applies: field weights, penalties
//! (multipliers), confidence caps (ceilings) and floors, and the verdict
//! thresholds; and [`combine`], which weighs the fields into a confidence.

use super::{Agreement, FieldCheck};

/// Field weights in the confidence (before penalties).
const W_TITLE: f64 = 0.5;
const W_AUTHORS: f64 = 0.25;
const W_YEAR: f64 = 0.25;

/// Multiplier when the year is off by two or more.
const YEAR_MISMATCH_PENALTY: f64 = 0.5;
/// Multiplier when the title clearly differs.
const TITLE_MISMATCH_PENALTY: f64 = 0.5;
/// Multiplier when the authors clearly differ.
const AUTHORS_MISMATCH_PENALTY: f64 = 0.5;
/// Confidence ceiling when fewer than two fields could be compared.
pub(super) const SINGLE_FIELD_CAP: f64 = 0.7;
/// Confidence ceiling without corroborating authors or identifiers.
pub(super) const UNCORROBORATED_CAP: f64 = 0.8;
/// Author score that corroborates a match: the first author present
/// (0.75 for structured claims, 0.9 in free text).
pub(super) const CORROBORATING_AUTHORS: f64 = 0.75;
/// Confidence when the journal, volume and first page all agree and
/// nothing contradicts them: verified.
pub(super) const PLACED: f64 = 0.9;
/// The same with the year off by one (online first, then in print):
/// plausible.
pub(super) const PLACED_YEAR_OFF: f64 = 0.8;
/// The title, authors, volume and first page agreeing with the year two or
/// more off: plausible, the year to check, and as high as a partial match
/// can be (a partial title with another page is capped there too), so
/// that the work confirming more of the citation ranks first.
pub(super) const PLACED_YEAR_WRONG: f64 = PLACED_YEAR_OFF;
/// Title score, a mismatch, for another title of a series: the title's
/// own year replaced ([`super::text::Normalised::substitutes_in`]).
pub(super) const OTHER_IN_SERIES: f64 = 0.3;
/// Confidence ceiling, below plausible, without evidence of which work is
/// cited: only a year or journal agreeing, or a short title found inside
/// free text.
pub(super) const NO_EVIDENCE_CAP: f64 = 0.5;
/// Another edition of a cited book: plausible, with the year a difference.
pub(super) const EDITION: f64 = PLAUSIBLE;
/// A dataset's or software's version, its identifier claimed, cited with
/// another year: plausible, with the year a difference.
pub(super) const VERSION: f64 = PLAUSIBLE;
/// Content words below which a title found inside free text is short: one
/// or two words turn up in any text ("Aspero", "Deep learning").
pub(super) const SHORT_TITLE_WORDS: usize = 3;
/// Multiplier for replies, corrections, errata and commentaries.
pub(super) const DERIVATIVE_PENALTY: f64 = 0.5;
/// Confidence ceiling for the original when the claim cites a reply,
/// correction or comment on it.
pub(super) const DERIVATIVE_CITED_CAP: f64 = LOCATION_MISMATCH_CAP;
/// Confidence ceiling when the claimed volume or first page differs.
pub(super) const LOCATION_MISMATCH_CAP: f64 = 0.8;
/// Multiplier when the claim names a journal clearly another
/// ([`crate::publication::other_journal`]): lowers the ranking.
pub(super) const JOURNAL_MISMATCH_PENALTY: f64 = 0.9;
/// Confidence ceiling when a journal article is cited in a journal that is
/// clearly another ([`crate::publication::other_journal`]): a citation error, as a wrong volume
/// is, and a tell of invented references.
pub(super) const JOURNAL_MISMATCH_CAP: f64 = LOCATION_MISMATCH_CAP;
/// Confidence ceiling when the claim names an author the record's complete
/// byline lacks: invented co-authors beside a real first author.
pub(super) const STRANGERS_CAP: f64 = LOCATION_MISMATCH_CAP;
/// Title score, partial, when the claim's title is the record's with words
/// added ([`super::titles::extends`]).
pub(super) const EXTENDED_TITLE: f64 = 0.75;
/// Confidence ceiling then: the claim may cite another work whose title
/// contains this one's, or this one with a subtitle its record lacks.
pub(super) const EXTENDED_TITLE_CAP: f64 = LOCATION_MISMATCH_CAP;
/// Confidence ceiling for a work with no journal, volume or page when the
/// claim cites a journal: a lookalike copy can't confirm it.
pub(super) const UNPLACED_CAP: f64 = LOCATION_MISMATCH_CAP;

/// Verdict thresholds on confidence.
pub const VERIFIED: f64 = 0.85;
pub const PLAUSIBLE: f64 = 0.6;

pub(super) fn combine(title: FieldCheck, authors: FieldCheck, year: FieldCheck) -> f64 {
    let parts = [(title, W_TITLE), (authors, W_AUTHORS), (year, W_YEAR)];
    let known: Vec<(f64, f64)> = parts
        .iter()
        .filter_map(|(check, w)| check.score.map(|s| (s, *w)))
        .collect();
    let weight: f64 = known.iter().map(|(_, w)| w).sum();
    if weight == 0.0 {
        return 0.0;
    }
    let mut confidence = known.iter().map(|(s, w)| s * w).sum::<f64>() / weight;
    if year.agreement == Agreement::Mismatch {
        confidence *= YEAR_MISMATCH_PENALTY;
    }
    if title.agreement == Agreement::Mismatch {
        confidence *= TITLE_MISMATCH_PENALTY;
    }
    // Different authors usually mean a different work (a similarly titled
    // book or survey), even when title and year agree.
    if authors.agreement == Agreement::Mismatch {
        confidence *= AUTHORS_MISMATCH_PENALTY;
    }
    if known.len() < 2 {
        confidence = confidence.min(SINGLE_FIELD_CAP);
    }
    confidence
}
