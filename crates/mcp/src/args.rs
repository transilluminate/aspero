//! Each tool's arguments: serde types whose JSON Schemas are the tools'
//! input schemas. Unknown keys are refused, so typos aren't ignored.

use aspero_core::Sort;
use aspero_json::{ClaimInput, QueryTree};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::tools::{DEFAULT_LIMIT, MAX_CITATIONS, MAX_IDS, MAX_LIMIT};

fn default_limit() -> u32 {
    DEFAULT_LIMIT
}

/// Order of the hits.
#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SortBy {
    /// Best match first.
    #[default]
    Relevance,
    /// Most recently published first.
    Newest,
    /// Most cited first.
    MostCited,
}

impl From<SortBy> for Sort {
    fn from(by: SortBy) -> Sort {
        match by {
            SortBy::Relevance => Sort::Relevance,
            SortBy::Newest => Sort::Newest,
            SortBy::MostCited => Sort::MostCited,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchArgs {
    /// The search as a query tree: all, any and none of terms, filters and
    /// relations. Give this or `text`.
    pub query: Option<QueryTree>,
    /// Keywords. Give this or `query`.
    pub text: Option<String>,
    /// The source to search; by default the first that can. Rankings
    /// differ between sources.
    pub source: Option<String>,
    #[serde(default)]
    pub sort: SortBy,
    /// Results per page.
    #[serde(default = "default_limit")]
    #[schemars(range(min = 1, max = MAX_LIMIT))]
    pub limit: u32,
    /// A previous page's `next_cursor`, to get the next page: with the same
    /// search and `source` (the `provider` that answered).
    pub cursor: Option<String>,
    /// Every known field of each work, not just the lean set.
    #[serde(default)]
    pub full: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SupportsArgs {
    /// The source to ask about; by default the one `search` would pick.
    pub source: Option<String>,
    /// The query tree, as `search` takes it.
    pub query: QueryTree,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LookupArgs {
    /// Identifiers, bare, prefixed or as URLs: DOIs, arXiv IDs, PMIDs,
    /// PMCIDs, NCBI Bookshelf IDs, ISBNs, OpenAlex IDs. Bare digits are
    /// ambiguous: prefix them (pmid:).
    #[schemars(length(min = 1, max = MAX_IDS))]
    pub ids: Vec<String>,
    /// Every known field, plus notices (retractions, corrections) from the
    /// DOI's registry, open access from any source, disagreements with the
    /// registry, and the latest version of superseded works.
    #[serde(default)]
    pub full: bool,
}

/// A citation: the reference as written, or what you know of it as fields
/// (more reliable than text).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Citation {
    Text(String),
    Fields(#[schemars(with = "ClaimInput")] Map<String, Value>),
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VerifyArgs {
    /// One citation. Give this or `citations`.
    pub citation: Option<Citation>,
    /// Several citations, verified together; results come back in the same
    /// order. Give this or `citation`.
    #[schemars(length(min = 1, max = MAX_CITATIONS))]
    pub citations: Option<Vec<Citation>>,
    /// Every known field of each match, and every candidate considered.
    #[serde(default)]
    pub full: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IdentifyArgs {
    /// The document's first page, as text (from pdftotext, PDFKit, …).
    pub text: String,
    /// The document's title metadata; ignored when junk
    /// ("Microsoft Word - draft3.docx").
    pub title: Option<String>,
    /// The largest type at the top of the page.
    pub title_hint: Option<String>,
    /// Every known field of the work, and every candidate considered.
    #[serde(default)]
    pub full: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RelationArgs {
    /// The work, by any identifier `lookup` takes (DOI, PMID, arXiv ID,
    /// OpenAlex ID, …).
    pub id: String,
    #[serde(default)]
    pub sort: SortBy,
    /// Results per page.
    #[serde(default = "default_limit")]
    #[schemars(range(min = 1, max = MAX_LIMIT))]
    pub limit: u32,
    /// A previous page's `next_cursor`, for the next page.
    pub cursor: Option<String>,
    /// Every known field of each work, not just the lean set.
    #[serde(default)]
    pub full: bool,
}
