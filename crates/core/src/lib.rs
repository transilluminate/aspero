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
//! - [`resilience`]: per-provider [`ResiliencePolicy`].
//! - [`http`]: the [`HttpClient`] transport seam.
//! - `testing` (feature): fixture replay for provider tests.

pub mod csl;
pub mod error;
mod fold;
pub mod http;
pub mod id;
pub mod matching;
pub mod provider;
pub mod publication;
pub mod record;
pub mod references;
pub mod resilience;
pub mod work;

#[cfg(feature = "testing")]
pub mod testing;

pub use error::{Error, ErrorCategory, ErrorKind, Result};
pub use http::{HttpClient, HttpRequest, HttpResponse};
pub use id::{
    ArxivId, CorpusId, Doi, IdError, IdKind, Isbn, Nbk, OpenAlexId, Pmcid, Pmid, S2PaperId, WorkId,
    canonical,
};
pub use matching::{Agreement, Alternative, Assessment, Claim, FieldCheck};
pub use provider::{
    Capabilities, Cursor, Filter, Hit, Lookup, Page, PageReq, Provider, ProviderId, Response,
    SearchMode, SearchQuery, Usage, YearRange,
};
pub use record::{Field, FieldSet, Raw, SourceRecord};
pub use references::{IndentedLine, LineReference, Reference, split_lines, split_references};
pub use resilience::ResiliencePolicy;
pub use work::{
    OpenAccess, PartialDate, Person, Update, UpdateKind, UpdateTo, Work, WorkType, tidy_abstract,
};
