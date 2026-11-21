//! Preprint servers: told by name or DOI prefix, and a preprint's DOI cited
//! for a journal's article (another version of the work).

use super::Claim;
use super::same_work::same_work_id;
use super::text::words;
use crate::id::{IdKind, WorkId};
use crate::record::SourceRecord;

/// Words naming a preprint server, or a preprint.
const PREPRINT_WORDS: &[&str] = &[
    "arxiv",
    "biorxiv",
    "medrxiv",
    "chemrxiv",
    "psyarxiv",
    "socarxiv",
    "techrxiv",
    "ssrn",
    "preprint",
    "preprints",
];

/// Whether a venue is a preprint server's: "arXiv (Cornell University)",
/// "medRxiv", "Research Square", "SSRN Electronic Journal".
pub(super) fn preprint_server(name: &str) -> bool {
    names_server(&words(name))
}

/// Whether words name a preprint server.
fn names_server(words: &[String]) -> bool {
    words.iter().any(|w| PREPRINT_WORDS.contains(&w.as_str()))
        || words.windows(2).any(|w| w == ["research", "square"])
}

/// Whether a reference's words name a preprint server or a version before
/// publication: a preprint, or a working paper ("NBER Working Paper",
/// "NBER WP"), which a journal's article is not. A series of working
/// papers as a record's venue is no preprint server.
pub(super) fn names_preprint(words: &[String]) -> bool {
    names_server(words)
        || words.windows(2).any(|w| {
            (w[0] == "working" && matches!(w[1].as_str(), "paper" | "papers"))
                || w == ["nber", "wp"]
        })
}

/// Preprint servers by their DOIs' prefixes (and, for bioRxiv's and
/// medRxiv's, the digits after it: Cold Spring Harbor's journals share it,
/// "10.1101/gr.…"). arXiv's isn't here: physics and computing cite a
/// journal's article with its arXiv identifier as a matter of course.
const PREPRINT_DOIS: &[(&str, &str)] = &[
    ("10.1101/", "bioRxiv or medRxiv"),
    ("10.21203/", "Research Square"),
    ("10.20944/", "Preprints.org"),
    ("10.2139/", "SSRN"),
    ("10.31234/", "PsyArXiv"),
    ("10.31219/", "OSF Preprints"),
    ("10.31235/", "SocArXiv"),
    ("10.26434/", "ChemRxiv"),
    ("10.36227/", "TechRxiv"),
    ("10.22541/", "Authorea"),
];

/// The preprint server whose DOI `id` is, if it is one.
pub fn preprint_server_of(id: &WorkId) -> Option<&'static str> {
    if id.kind() != IdKind::Doi {
        return None;
    }
    let doi = id.to_string();
    let doi = doi.strip_prefix("doi:").unwrap_or(&doi);
    PREPRINT_DOIS.iter().find_map(|(prefix, server)| {
        let rest = doi.strip_prefix(prefix)?;
        (*prefix != "10.1101/" || rest.starts_with(|c: char| c.is_ascii_digit())).then_some(*server)
    })
}

/// The preprint server whose version of the work a citation's identifier
/// names, when the citation gives a journal for it: a journal article's
/// record carrying its preprint's DOI too (OpenAlex merges the two), cited
/// by the preprint's DOI with the journal's name ("… Journal of
/// Cardiothoracic Surgery. 2024;19(1):2. doi:10.21203/rs.3.rs-2791419/v1").
/// The identifier is another version's than the one cited: plausible, the
/// journal to check against the server's.
pub fn preprint_version_claimed(claim: &Claim, record: &SourceRecord) -> Option<&'static str> {
    let journals = claim.cited_containers();
    if journals.is_empty() || journals.iter().any(|j| preprint_server(j)) {
        return None;
    }
    let held = |id: &WorkId| record.ids.iter().any(|r| same_work_id(r, id));
    // The record's own DOI, a journal's.
    let published = record
        .ids
        .iter()
        .any(|id| id.kind() == IdKind::Doi && preprint_server_of(id).is_none());
    let claimed_published = claim
        .ids
        .iter()
        .any(|id| held(id) && preprint_server_of(id).is_none());
    if !published || claimed_published || record.work.volume.is_none() {
        return None;
    }
    claim
        .ids
        .iter()
        .filter(|id| held(id))
        .find_map(preprint_server_of)
}
