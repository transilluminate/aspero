//! Core types, identifier parsing/normalisation, the `Provider` trait and errors. No I/O.

pub mod id;

pub use id::{ArxivId, CorpusId, Doi, IdError, IdKind, OpenAlexId, Pmcid, Pmid, S2PaperId, WorkId};
