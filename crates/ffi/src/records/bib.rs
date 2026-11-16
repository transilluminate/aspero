//! Bibliography files and Word documents read as claims, and the formats
//! works are exported in.

use super::{Claim, count};

/// A bibliography file's format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BibFormat {
    Bibtex,
    Ris,
    CslJson,
}

impl From<aspero_bib::Format> for BibFormat {
    fn from(f: aspero_bib::Format) -> Self {
        match f {
            aspero_bib::Format::Ris => BibFormat::Ris,
            aspero_bib::Format::CslJson => BibFormat::CslJson,
            _ => BibFormat::Bibtex,
        }
    }
}

/// A bibliography file's entries, each a claim ready to verify.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Bibliography {
    pub format: BibFormat,
    pub entries: Vec<BibEntry>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct BibEntry {
    /// Its fields and identifiers, with `key` the entry's key (an RIS
    /// record or CSL-JSON item without one: its 1-based position).
    pub claim: Claim,
    /// Lowercase, in the file's own vocabulary: `article` (BibTeX),
    /// `jour` (RIS), `article-journal` (CSL-JSON).
    pub entry_type: String,
    /// Problems with the entry's own fields: a `doi` that isn't a DOI.
    pub notes: Vec<BibNote>,
    /// Whether there is anything to verify: a title or an identifier.
    pub checkable: bool,
}

impl From<aspero_bib::BibEntry> for BibEntry {
    fn from(e: aspero_bib::BibEntry) -> Self {
        let checkable = e.is_checkable();
        let c = e.claim;
        BibEntry {
            claim: Claim {
                text: None,
                title: c.title,
                authors: c.authors,
                year: c.year,
                container: c.container,
                volume: c.volume,
                page: c.page,
                ids: c.ids.iter().map(ToString::to_string).collect(),
                key: Some(e.key),
                alternative_ids: Vec::new(),
                site: None,
            },
            entry_type: e.entry_type,
            notes: e.notes.iter().map(Into::into).collect(),
            checkable,
        }
    }
}

/// A problem with one of an entry's own fields.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BibNote {
    /// The field as the file names it: `doi`, `eprint`, `DO`, `ISBN`.
    pub field: String,
    pub value: String,
    pub problem: BibProblem,
    /// In English, for logs.
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BibProblem {
    NotADoi,
    NotAnArxivId,
    NotAnIsbn,
    NotAPmid,
}

impl From<aspero_bib::Problem> for BibProblem {
    fn from(p: aspero_bib::Problem) -> Self {
        use aspero_bib::Problem;
        match p {
            Problem::NotADoi => BibProblem::NotADoi,
            Problem::NotAnArxivId => BibProblem::NotAnArxivId,
            Problem::NotAPmid => BibProblem::NotAPmid,
            _ => BibProblem::NotAnIsbn,
        }
    }
}

impl From<&aspero_bib::Note> for BibNote {
    fn from(n: &aspero_bib::Note) -> Self {
        BibNote {
            field: n.field.clone(),
            value: n.value.clone(),
            problem: n.problem.into(),
            message: n.to_string(),
        }
    }
}

/// Why text couldn't be read as a bibliography file.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum ReadError {
    /// Not BibTeX, RIS or CSL-JSON.
    #[error("not BibTeX, RIS or CSL-JSON")]
    Unrecognised,
    /// The format was recognised, but the file is broken at `line`.
    #[error("{message} (line {line})")]
    Invalid {
        format: BibFormat,
        line: u64,
        message: String,
    },
}

impl From<aspero_bib::ReadError> for ReadError {
    fn from(e: aspero_bib::ReadError) -> Self {
        match e {
            aspero_bib::ReadError::Invalid {
                format,
                line,
                message,
            } => ReadError::Invalid {
                format: format.into(),
                line: line as u64,
                message,
            },
            _ => ReadError::Unrecognised,
        }
    }
}

/// A format to write works in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ExportFormat {
    Bibtex,
    Ris,
    /// One CSL-JSON array, as Zotero imports it (and `readReferences`
    /// reads it).
    CslJson,
}

/// The citation manager whose fields a Word document holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CitationManager {
    Zotero,
    Mendeley,
}

/// What a Word document cites.
#[derive(Debug, Clone, uniffi::Record)]
pub struct DocxReferences {
    /// Each work cited, once, in order of first citation, its `claim`
    /// ready to verify; `key` is the manager's item key.
    pub entries: Vec<BibEntry>,
    /// Whose citation fields they came from; `nil` when there were none.
    pub manager: Option<CitationManager>,
    /// Citations in the text (one may cite several works).
    pub citations: u32,
    /// The document's text, a paragraph per line: without fields, its
    /// reference list can be split with `splitReferences` instead.
    pub text: String,
}

impl From<aspero_bib::docx::Manager> for CitationManager {
    fn from(m: aspero_bib::docx::Manager) -> Self {
        match m {
            aspero_bib::docx::Manager::Mendeley => CitationManager::Mendeley,
            _ => CitationManager::Zotero,
        }
    }
}

impl From<aspero_bib::docx::DocxReferences> for DocxReferences {
    fn from(d: aspero_bib::docx::DocxReferences) -> Self {
        DocxReferences {
            entries: d.entries.into_iter().map(Into::into).collect(),
            manager: d.manager.map(Into::into),
            citations: count(d.citations),
            text: d.text,
        }
    }
}
