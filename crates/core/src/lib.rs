//! Core types, identifier parsing/normalisation, the `Provider` trait and errors. No I/O.
//!
//! - [`id`]: identifiers and their normalisation.
//! - [`work`]: the provider-neutral [`Work`] model.
//! - [`record`]: [`SourceRecord`], one provider's view of one work.
//! - [`provider`]: the [`Provider`] trait and its request/response types.
//! - [`error`]: the shared [`Error`] contract.
//! - [`resilience`]: per-provider [`ResiliencePolicy`].

pub mod error;
pub mod id;
pub mod provider;
pub mod record;
pub mod resilience;
pub mod work;

pub use error::{Error, ErrorCategory, Result};
pub use id::{ArxivId, CorpusId, Doi, IdError, IdKind, OpenAlexId, Pmcid, Pmid, S2PaperId, WorkId};
pub use provider::{
    Capabilities, Cursor, Filter, Lookup, Page, PageReq, Provider, ProviderId, Response,
    SearchQuery, Usage, YearRange,
};
pub use record::{Field, FieldSet, Raw, SourceRecord};
pub use resilience::ResiliencePolicy;
pub use work::{PartialDate, Person, Work, WorkType};
