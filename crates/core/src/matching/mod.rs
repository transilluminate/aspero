//! Does a record match what a citation claims? Pure scoring, no I/O.
//!
//! A [`Claim`] is what someone (often an agent) asserted: a free-text
//! reference, structured fields, identifiers, or a mix. [`assess()`] compares
//! one candidate record against it, field by field, and combines the fields
//! into a confidence in `0.0..=1.0`.
//!
//! Lessons from real data shape the rules (see DESIGN "Provider notes and
//! gotchas"):
//! - **Lookalikes exist**: re-deposits with the real title, authors and
//!   abstract under a different year and publisher. A wrong year is a strong
//!   penalty, never a detail.
//! - **One field is not proof**: a title alone ("Deep learning") matches
//!   many works, so confidence is capped unless at least two fields agree.
//! - Free-text references are not parsed into fields. Instead the record's
//!   fields are looked for *in* the text: its title as a phrase, its authors'
//!   family names, its year. This is robust to citation styles.
//!
//! Where the rules live:
//! - `claim.rs`: reading a claim: its identifiers, year, journal, place,
//!   byline and venue.
//! - `assess.rs`: scoring one candidate record against a claim, field by
//!   field.
//! - `caps.rs`: the policy numbers: field weights, penalties, confidence
//!   caps and floors, and the verdict thresholds.
//! - `authors.rs`: author names; `titles.rs`: comparing titles; `text.rs`:
//!   words, and finding a record's fields inside free text.
//! - `edition.rs`: a book's first edition as a citation states it ("(1st
//!   ed.)", "(Original work published 1969)").
//! - `publisher.rs`: the publisher a book's citation names, and whether its
//!   editions know it.
//! - `record_title.rs`: a record's title cleaned of markup and provider
//!   furniture before it is compared.
//! - `preprint.rs`: preprint servers, and a preprint's DOI cited for a
//!   journal's article.
//! - `derivative.rs`: replies, corrections and notices about a work.
//! - `same_work.rs`: identifier agreement, and whether two records are one
//!   work.
//! - `title_layout.rs`: a book's reference around its title: a
//!   publisher's name, notes of an editor or translator, a byline's
//!   initials; and "Name, Title, Publisher, City Year" references.
//! - `venue.rs`: the venue cited against a record's: a chapter cited as an
//!   article or the reverse, another conference or meeting, a preprint at
//!   one; and the record's kind against the work cited (a book or copy for
//!   an article, a chapter for its book, a proceedings for its paper).
//! - `version.rs`: versions of one work told apart: a Cochrane review's
//!   earlier version's byline, or a newer version's year merged in.
//! - `written.rs`: a free-text reference's title and authors as written,
//!   for the claim as read.

mod assess;
mod authors;
mod caps;
mod claim;
mod derivative;
mod edition;
mod preprint;
mod publisher;
mod record_title;
mod same_work;
mod text;
mod title_layout;
mod titles;
mod venue;
mod version;
mod written;

use serde::{Deserialize, Serialize};

pub use assess::assess;
pub use authors::share_an_author;
pub use caps::{ID_CONFLICT_CAP, PLAUSIBLE, VERIFIED, WHOLE_BYLINE};
pub use claim::{Alternative, Claim, find_ids};
pub use derivative::{
    WorkKind, claim_mentions_derivative, derivative_kind, derivative_work_kind, is_derivative,
    record_derivative_kind,
};
pub use edition::{first_year_cited, first_year_contradicted};
pub use preprint::{preprint_server_of, preprint_version_claimed};
pub use publisher::{
    EDITIONS_ENOUGH, PublisherCheck, check_publisher, cited_publisher, known_publisher,
    same_publisher,
};
pub use same_work::{conflicting_id, id_agreement, same_work, same_work_id};
pub use text::normalise;
pub use titles::{
    given_title, probable_title, search_terms, shared_title, title_similarity, unmarked,
    whole_title_similarity,
};
pub use version::earlier_id_claimed;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Agreement {
    Match,
    Partial,
    Mismatch,
    /// The claim or the record doesn't say.
    Unknown,
}

/// One field's comparison: a score in `0.0..=1.0` when comparable.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FieldCheck {
    pub agreement: Agreement,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
}

impl FieldCheck {
    const UNKNOWN: FieldCheck = FieldCheck {
        agreement: Agreement::Unknown,
        score: None,
    };

    fn exact(agrees: bool) -> Self {
        FieldCheck::scored(if agrees { 1.0 } else { 0.0 })
    }

    fn scored(score: f64) -> Self {
        let agreement = match score {
            s if s >= 0.9 => Agreement::Match,
            s if s >= 0.5 => Agreement::Partial,
            _ => Agreement::Mismatch,
        };
        FieldCheck {
            agreement,
            score: Some(score),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Assessment {
    pub confidence: f64,
    pub title: FieldCheck,
    pub authors: FieldCheck,
    pub year: FieldCheck,
    /// The journal: `Match` when the claim names it (or an abbreviation),
    /// `Mismatch` when a structured claim names one clearly another, or a
    /// journal article's reference cites one
    /// ([`Claim::cited_container`], [`crate::publication::other_journal`]).
    pub container: FieldCheck,
    pub volume: FieldCheck,
    /// The first page.
    pub page: FieldCheck,
    /// Whether the claim's identifiers are the record's: `Match` if any
    /// claimed ID is the record's, `Partial` if one is but another (a PMID
    /// or PMCID) differs from the record's of its kind ([`conflicting_id`]),
    /// `Mismatch` if none is and the record has a different ID of the same
    /// kind, else `Unknown`.
    pub ids: Agreement,
}

#[cfg(test)]
// Exact constants: scores and spends rounded or summed from binary-exact values.
#[allow(clippy::float_cmp)]
mod tests;
