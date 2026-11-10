//! Core types, identifier parsing/normalisation, the `Provider` trait and errors. No I/O.
//!
//! - [`id`]: identifiers and their normalisation.
//! - [`work`]: the provider-neutral [`Work`] model.
//! - [`record`]: [`SourceRecord`], one provider's view of one work.
//! - [`provider`]: the [`Provider`] trait and its request/response types.
//! - [`error`]: the shared [`Error`] contract.
//! - [`matching`]: does a record match what a citation claims?
//! - [`resilience`]: per-provider [`ResiliencePolicy`].
//! - [`http`]: the [`HttpClient`] transport seam.
//! - `testing` (feature): fixture replay for provider tests.

pub mod error;
pub mod http;
pub mod id;
pub mod matching;
pub mod provider;
pub mod record;
pub mod resilience;
pub mod work;

#[cfg(feature = "testing")]
pub mod testing;

pub use error::{Error, ErrorCategory, Result};
pub use http::{HttpClient, HttpRequest, HttpResponse};
pub use id::{ArxivId, CorpusId, Doi, IdError, IdKind, OpenAlexId, Pmcid, Pmid, S2PaperId, WorkId};
pub use matching::{Agreement, Assessment, Claim, FieldCheck};
pub use provider::{
    Capabilities, Cursor, Filter, Hit, Lookup, Page, PageReq, Provider, ProviderId, Response,
    SearchMode, SearchQuery, Usage, YearRange,
};
pub use record::{Field, FieldSet, Raw, SourceRecord};
pub use resilience::ResiliencePolicy;
pub use work::{PartialDate, Person, Work, WorkType, tidy_abstract};
