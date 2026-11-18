//! Core types, identifier parsing/normalisation, the `Provider` trait and errors. No I/O.
//!
//! - [`id`]: identifiers and their normalisation.
//! - [`csl`]: lenient CSL-JSON reading.
//! - [`work`]: the provider-neutral [`Work`] model.
//! - [`record`]: [`SourceRecord`], one provider's view of one work.
//! - [`provider`]: the [`Provider`] trait and its request/response types.
//! - [`error`]: the shared [`Error`] contract.
//! - [`matching`]: does a record match what a citation claims?
//! - [`references`]: splitting a pasted reference list.
//! - [`tagged`]: a reference pasted as a Wikipedia citation template, an
//!   RIS or an EndNote record, read as a structured claim.
//! - [`resilience`]: per-provider [`ResiliencePolicy`].
//! - [`site`]: an article page's site, and its publisher's DOI prefixes.
//! - [`webpage`]: a claim from a publisher's article page's markup.
//! - [`http`]: the [`HttpClient`] transport seam.
//! - `testing` (feature): fixture replay for provider tests; log events
//!   captured for tests of the diagnostics events.

pub mod csl;
pub mod error;
mod fold;
pub mod http;
pub mod id;
pub mod matching;
pub mod provider;
pub mod publication;
pub mod query;
pub mod record;
pub mod references;
pub mod resilience;
pub mod site;
pub mod tagged;
pub mod webpage;
pub mod work;

#[cfg(feature = "testing")]
pub mod testing;

pub use error::{CapKind, Error, ErrorCategory, ErrorKind, Result};
pub use http::{HttpClient, HttpRequest, HttpResponse};
pub use id::{
    ArxivId, CorpusId, Doi, IdError, IdKind, Isbn, Nbk, OpenAlexId, Pii, Pmcid, Pmid, S2PaperId,
    WorkId, canonical,
};
pub use matching::{Agreement, Alternative, Assessment, Claim, FieldCheck};
pub use provider::{
    Capabilities, Cursor, Filter, Hit, Lookup, Page, PageReq, Place, Prices, Provider, ProviderId,
    Response, SearchMode, SearchQuery, Sort, Usage, YearRange,
};
pub use query::{
    AuthorId, BooleanSupport, FilterValue, QueryNode, Relation, RelationTerm, Term, TermField,
};
pub use record::{Field, FieldSet, Raw, SourceRecord};
pub use references::{IndentedLine, LineReference, Reference, split_lines, split_references};
pub use resilience::ResiliencePolicy;
pub use webpage::{PageClaim, PageId, PageKind, PageSource, WebPage, claim_from_page};
pub use work::{
    Notice, NoticeFor, NoticeKind, OpenAccess, PartialDate, Person, Work, WorkType, orcid,
    tidy_abstract,
};
