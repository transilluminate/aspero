//! Replies, corrections and commentaries: records *about* a work, which
//! often repeat its title.

use super::Claim;
use super::text::normalise;

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

/// Whether a title marks a derivative record: "…—Reply", "Correction to:
/// …", "Comment on “…”", "Faculty Opinions recommendation of …".
pub fn is_derivative(title: &str) -> bool {
    let words = format!(" {} ", normalise(title));
    DERIVATIVE_MARKERS
        .iter()
        .any(|m| words.contains(&format!(" {m} ")))
}

/// Whether the claim itself cites a derivative (its text or title uses a
/// marker), in which case derivatives are fair matches.
pub(super) fn claim_mentions_derivative(claim: &Claim) -> bool {
    [claim.text.as_deref(), claim.title.as_deref()]
        .into_iter()
        .flatten()
        .any(is_derivative)
}
