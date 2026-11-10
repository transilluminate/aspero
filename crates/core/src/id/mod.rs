//! Scholarly work identifiers: parsing, normalisation and canonical display.
//!
//! Contract shared by every identifier type here:
//!
//! - `FromStr` accepts the forms people and providers actually write: bare
//!   (`10.1145/3368089`), `kind:`-prefixed with any case (`DOI: 10.1145/…`,
//!   `PMID:123`, `CorpusId:123`), and the kind's well-known URLs
//!   (`https://doi.org/…`, `arxiv.org/abs/…`).
//! - A parsed value is canonical. `Display` prints the canonical form, and
//!   parsing that output yields an equal value, so normalisation is idempotent.
//! - Equality is equality of canonical forms.
//! - Parsing is strict about structure, with one concession to pasted prose:
//!   DOIs drop trailing sentence punctuation (see [`Doi`]). Pulling
//!   identifiers out of running text (line-wrapped DOIs, surrounding words)
//!   belongs to `resolve`, not here.
//!
//! [`WorkId`] is the kind-tagged union. Its canonical form is `kind:value`
//! (`doi:10.1145/3368089`), which is also how it serialises.

mod arxiv;
mod doi;
mod numeric;
mod s2;
mod syntax;
mod url;

use std::fmt;
use std::str::FromStr;

pub use arxiv::ArxivId;
pub use doi::Doi;
pub use numeric::{CorpusId, OpenAlexId, Pmcid, Pmid};
pub use s2::S2PaperId;

use syntax::IdSyntax;
use url::Url;

/// The kinds of work identifier aspero understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum IdKind {
    Doi,
    Arxiv,
    Pmid,
    Pmcid,
    OpenAlex,
    S2Paper,
    CorpusId,
}

impl IdKind {
    pub const ALL: [IdKind; 7] = [
        IdKind::Doi,
        IdKind::Arxiv,
        IdKind::Pmid,
        IdKind::Pmcid,
        IdKind::OpenAlex,
        IdKind::S2Paper,
        IdKind::CorpusId,
    ];

    /// The lowercase prefix used in `kind:value` form.
    pub fn prefix(self) -> &'static str {
        match self {
            IdKind::Doi => "doi",
            IdKind::Arxiv => "arxiv",
            IdKind::Pmid => "pmid",
            IdKind::Pmcid => "pmcid",
            IdKind::OpenAlex => "openalex",
            IdKind::S2Paper => "s2",
            IdKind::CorpusId => "corpusid",
        }
    }

    /// Case-insensitive inverse of [`IdKind::prefix`].
    pub fn from_prefix(s: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|k| k.prefix().eq_ignore_ascii_case(s))
    }
}

impl fmt::Display for IdKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.prefix())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum IdError {
    #[error("empty identifier")]
    Empty,
    #[error("unrecognised identifier {0:?}")]
    Unrecognised(String),
    #[error(
        "ambiguous identifier {input:?}: prefix it with one of {}",
        prefixes(.candidates)
    )]
    Ambiguous {
        input: String,
        candidates: Vec<IdKind>,
    },
    #[error("invalid {kind} {input:?}: {reason}")]
    Invalid {
        kind: IdKind,
        input: String,
        reason: &'static str,
    },
}

fn prefixes(kinds: &[IdKind]) -> String {
    kinds
        .iter()
        .map(|k| format!("{k}:"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Any supported work identifier, tagged with its kind.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum WorkId {
    Doi(Doi),
    Arxiv(ArxivId),
    Pmid(Pmid),
    Pmcid(Pmcid),
    OpenAlex(OpenAlexId),
    S2Paper(S2PaperId),
    CorpusId(CorpusId),
}

impl WorkId {
    pub fn kind(&self) -> IdKind {
        match self {
            WorkId::Doi(_) => IdKind::Doi,
            WorkId::Arxiv(_) => IdKind::Arxiv,
            WorkId::Pmid(_) => IdKind::Pmid,
            WorkId::Pmcid(_) => IdKind::Pmcid,
            WorkId::OpenAlex(_) => IdKind::OpenAlex,
            WorkId::S2Paper(_) => IdKind::S2Paper,
            WorkId::CorpusId(_) => IdKind::CorpusId,
        }
    }

    /// Parses `input` as a specific kind, skipping detection. Accepts the same
    /// forms as that kind's own `FromStr`.
    pub fn parse_as(kind: IdKind, input: &str) -> Result<Self, IdError> {
        Ok(match kind {
            IdKind::Doi => WorkId::Doi(input.parse()?),
            IdKind::Arxiv => WorkId::Arxiv(input.parse()?),
            IdKind::Pmid => WorkId::Pmid(input.parse()?),
            IdKind::Pmcid => WorkId::Pmcid(input.parse()?),
            IdKind::OpenAlex => WorkId::OpenAlex(input.parse()?),
            IdKind::S2Paper => WorkId::S2Paper(input.parse()?),
            IdKind::CorpusId => WorkId::CorpusId(input.parse()?),
        })
    }

    fn from_url(url: &Url<'_>, input: &str) -> Result<Self, IdError> {
        fn try_kind<T: IdSyntax>(
            url: &Url<'_>,
            input: &str,
            wrap: fn(T) -> WorkId,
        ) -> Option<Result<WorkId, IdError>> {
            T::from_url(url).map(|bare| syntax::finish::<T>(bare, input).map(wrap))
        }

        try_kind(url, input, WorkId::Doi)
            .or_else(|| try_kind(url, input, WorkId::Arxiv))
            .or_else(|| try_kind(url, input, WorkId::Pmid))
            .or_else(|| try_kind(url, input, WorkId::Pmcid))
            .or_else(|| try_kind(url, input, WorkId::OpenAlex))
            .or_else(|| try_kind(url, input, WorkId::S2Paper))
            .or_else(|| try_kind(url, input, WorkId::CorpusId))
            .unwrap_or_else(|| Err(IdError::Unrecognised(input.to_owned())))
    }

    /// Guesses the kind of an unprefixed, non-URL identifier.
    fn detect_bare(s: &str, input: &str) -> Result<Self, IdError> {
        let all_digits = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());

        if s.starts_with("10.") || doi::has_info_prefix(s) {
            return WorkId::parse_as(IdKind::Doi, input);
        }
        if s.get(..3).is_some_and(|p| p.eq_ignore_ascii_case("pmc")) && all_digits(&s[3..]) {
            return WorkId::parse_as(IdKind::Pmcid, input);
        }
        if s.starts_with(['W', 'w']) && all_digits(&s[1..]) {
            return WorkId::parse_as(IdKind::OpenAlex, input);
        }
        if all_digits(s) {
            return Err(IdError::Ambiguous {
                input: input.to_owned(),
                candidates: vec![IdKind::Pmid, IdKind::CorpusId],
            });
        }
        if s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return WorkId::parse_as(IdKind::S2Paper, input);
        }
        if let Ok(id) = s.parse::<ArxivId>() {
            return Ok(WorkId::Arxiv(id));
        }
        Err(IdError::Unrecognised(input.to_owned()))
    }
}

impl FromStr for WorkId {
    type Err = IdError;

    /// Detects the kind: URL host first, then an explicit `kind:` prefix, then
    /// the shape of a bare value. Bare all-digit input is ambiguous (PMID or
    /// S2 CorpusId) and is rejected rather than guessed.
    fn from_str(input: &str) -> Result<Self, IdError> {
        let s = input.trim();
        if s.is_empty() {
            return Err(IdError::Empty);
        }
        if let Some(url) = Url::parse(s) {
            return Self::from_url(&url, input);
        }
        if let Some((head, _)) = s.split_once(':')
            && let Some(kind) = IdKind::from_prefix(head.trim())
        {
            return Self::parse_as(kind, input);
        }
        Self::detect_bare(s, input)
    }
}

impl fmt::Display for WorkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:", self.kind())?;
        match self {
            WorkId::Doi(id) => id.fmt(f),
            WorkId::Arxiv(id) => id.fmt(f),
            WorkId::Pmid(id) => id.fmt(f),
            WorkId::Pmcid(id) => id.fmt(f),
            WorkId::OpenAlex(id) => id.fmt(f),
            WorkId::S2Paper(id) => id.fmt(f),
            WorkId::CorpusId(id) => id.fmt(f),
        }
    }
}

syntax::impl_string_serde!(WorkId);

macro_rules! impl_from_for_work_id {
    ($($variant:ident($ty:ty)),* $(,)?) => {
        $(impl From<$ty> for WorkId {
            fn from(id: $ty) -> Self {
                WorkId::$variant(id)
            }
        })*
    };
}

impl_from_for_work_id!(
    Doi(Doi),
    Arxiv(ArxivId),
    Pmid(Pmid),
    Pmcid(Pmcid),
    OpenAlex(OpenAlexId),
    S2Paper(S2PaperId),
    CorpusId(CorpusId),
);

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<WorkId, IdError> {
        s.parse()
    }

    #[test]
    fn detects_kind_from_bare_prefixed_and_url_forms() {
        let cases = [
            ("10.1145/3368089", "doi:10.1145/3368089"),
            ("https://doi.org/10.1145/3368089", "doi:10.1145/3368089"),
            ("info:doi/10.1145/3368089", "doi:10.1145/3368089"),
            ("1706.03762v5", "arxiv:1706.03762v5"),
            ("arXiv:hep-th/9901001", "arxiv:hep-th/9901001"),
            ("https://arxiv.org/pdf/1706.03762", "arxiv:1706.03762"),
            ("PMID: 31452104", "pmid:31452104"),
            ("https://pubmed.ncbi.nlm.nih.gov/31452104/", "pmid:31452104"),
            ("PMC6716493", "pmcid:PMC6716493"),
            ("W2741809807", "openalex:W2741809807"),
            ("https://openalex.org/W2741809807", "openalex:W2741809807"),
            ("CorpusId:13756489", "corpusid:13756489"),
            (
                "https://api.semanticscholar.org/CorpusID:13756489",
                "corpusid:13756489",
            ),
            (
                "204E3073870FAE3D05BCBC2F6A8E263D9B72E776",
                "s2:204e3073870fae3d05bcbc2f6a8e263d9b72e776",
            ),
        ];
        for (input, want) in cases {
            assert_eq!(parse(input).unwrap().to_string(), want, "input {input:?}");
        }
    }

    #[test]
    fn bare_digits_are_ambiguous() {
        assert_eq!(
            parse("31452104"),
            Err(IdError::Ambiguous {
                input: "31452104".into(),
                candidates: vec![IdKind::Pmid, IdKind::CorpusId],
            })
        );
    }

    #[test]
    fn rejects_empty_and_unknown() {
        assert_eq!(parse("  "), Err(IdError::Empty));
        assert!(matches!(
            parse("attention is all you need"),
            Err(IdError::Unrecognised(_))
        ));
        assert!(matches!(
            parse("https://example.com/paper/1"),
            Err(IdError::Unrecognised(_))
        ));
    }

    #[test]
    fn explicit_prefix_reports_kind_specific_errors() {
        assert!(matches!(
            parse("doi:11.1234/x"),
            Err(IdError::Invalid {
                kind: IdKind::Doi,
                ..
            })
        ));
    }

    #[test]
    fn serialises_as_prefixed_string() {
        let id = parse("10.1145/3368089").unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, r#""doi:10.1145/3368089""#);
        assert_eq!(serde_json::from_str::<WorkId>(&json).unwrap(), id);
    }
}
