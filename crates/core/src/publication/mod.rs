//! Where a citation says a work appeared: journal, volume, first page.
//!
//! Journals are compared by rule rather than with an abbreviation list:
//! each word of an abbreviation must abbreviate the matching significant
//! word of the full title, in order ("N Engl J Med" ↔ "New England Journal
//! of Medicine", "Lancet" ↔ "The Lancet"), or the abbreviation is the
//! initials ("NEJM", "JAMA", "BMJ"). Pairs the rule can't see are
//! reported as different, so a mismatch by [`same_journal`] alone is a hint,
//! not proof; [`other_journal`] is the stricter test of a journal clearly
//! cited in place of another. Volume and page are numbers and compare
//! exactly.
//!
//! Where the rules live:
//! - `journal.rs`: comparing journal names: abbreviations, initials, sister
//!   journals and sections.
//! - `cited_journal.rs`: reading the journal a free-text reference cites.
//! - `locator.rs`: reading a reference's volume and first page, by style.
//! - `conference.rs`: conferences and meetings' proceedings as venues.
//! - `bibtex.rs`: a BibTeX entry's fields and accents.

mod bibtex;
mod cited_journal;
mod conference;
mod journal;
mod locator;

pub use journal::{other_journal, same_journal, sister_journal};
pub use locator::first_page;

pub(crate) use bibtex::{bibtex_field, delatex};
pub(crate) use cited_journal::{cited_journal, cited_journals};
pub(crate) use conference::{conferences, meeting_name, proceedings, proceedings_agree};
pub(crate) use journal::{
    journal_in, journal_word, other_journal_read, section_of, significant, sister_journal_as,
};
pub(crate) use locator::{Locator, cites_issue, is_month, locator};
