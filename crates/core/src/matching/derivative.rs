//! Replies, corrections and commentaries: records *about* a work, which
//! often repeat its title.

use super::Claim;
use super::text::normalise;
use crate::id::starts_with_ci;

/// Words marking a record as *about* another work rather than the work
/// itself: replies, corrections, errata, commentaries.
pub(super) const DERIVATIVE_MARKERS: &[&str] = &[
    "reply",
    "erratum",
    "errata",
    "corrigendum",
    "correction",
    "comment on",
    "response to",
    "in response",
    "faculty opinions recommendation",
    "retraction of",
    "expression of concern",
];

/// Markers that are ordinary words inside a title ("differential response
/// to corticosteroids", "correction of hyponatraemia"), so mark a
/// derivative only where it says what it is about: at the start, and
/// "correction" only as "Correction:" or "Correction to".
const LEADING_MARKERS: &[&str] = &["comment on", "response to", "in response", "correction"];

/// Whether a title marks a derivative record: "…—Reply", "Correction to:
/// …", "Comment on “…”", "Faculty Opinions recommendation of …".
pub fn is_derivative(title: &str) -> bool {
    derivative_kind(title).is_some()
}

/// What a derivative record is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WorkKind {
    Reply,
    Correction,
    Comment,
    Retraction,
    ExpressionOfConcern,
    Recommendation,
}

impl WorkKind {
    /// "reply", "correction", "comment", "retraction", "expression of
    /// concern" or "recommendation".
    pub fn as_str(self) -> &'static str {
        match self {
            WorkKind::Reply => "reply",
            WorkKind::Correction => "correction",
            WorkKind::Comment => "comment",
            WorkKind::Retraction => "retraction",
            WorkKind::ExpressionOfConcern => "expression of concern",
            WorkKind::Recommendation => "recommendation",
        }
    }
}

/// What a derivative record is, by its title, as [`WorkKind::as_str`].
pub fn derivative_kind(title: &str) -> Option<&'static str> {
    derivative_work_kind(title).map(WorkKind::as_str)
}

/// What a derivative record is, by its title.
pub fn derivative_work_kind(title: &str) -> Option<WorkKind> {
    let norm = normalise(title);
    let words = format!(" {norm} ");
    let leading = |m: &str| match m {
        "correction" => {
            norm == "correction"
                || norm.starts_with("correction to ")
                || starts_with_ci(title.trim_start(), "correction:")
        }
        _ => words.starts_with(&format!(" {m} ")),
    };
    let marker = DERIVATIVE_MARKERS
        .iter()
        .find(|m| match LEADING_MARKERS.contains(m) {
            true => leading(m),
            false => words.contains(&format!(" {m} ")),
        })?;
    Some(match *marker {
        "reply" | "response to" | "in response" => WorkKind::Reply,
        "erratum" | "errata" | "corrigendum" | "correction" => WorkKind::Correction,
        "comment on" => WorkKind::Comment,
        "retraction of" => WorkKind::Retraction,
        "expression of concern" => WorkKind::ExpressionOfConcern,
        _ => WorkKind::Recommendation,
    })
}

/// Whether the claim itself cites a derivative (its text or title uses a
/// marker), in which case derivatives are fair matches.
pub fn claim_mentions_derivative(claim: &Claim) -> bool {
    [claim.text.as_deref(), claim.title.as_deref()]
        .into_iter()
        .flatten()
        .any(|text| {
            let words = format!(" {} ", normalise(text));
            DERIVATIVE_MARKERS
                .iter()
                .any(|m| words.contains(&format!(" {m} ")))
        })
}
