//! Searching: the request and its query tree, what each source can
//! search by, and the results.

use std::collections::HashMap;

use aspero_core::{
    BooleanSupport, Capabilities, Filter, FilterValue, ProviderId, QueryNode, Relation, Sort,
    TermField, WorkType, YearRange,
};
use aspero_engine as engine;

use super::{Failure, Freshness, ProviderUsage, Work, usage};

#[derive(Debug, Clone, uniffi::Record)]
pub struct SearchRequest {
    /// Keywords or a title.
    pub text: String,
    #[uniffi(default = None)]
    pub year_from: Option<i16>,
    #[uniffi(default = None)]
    pub year_to: Option<i16>,
    /// A CSL type: `article-journal`, `book`, ….
    #[uniffi(default = None)]
    pub work_type: Option<String>,
    #[uniffi(default = false)]
    pub open_access_only: bool,
    #[uniffi(default = 10)]
    pub limit: u32,
    /// A provider by name; otherwise the first that searches (OpenAlex,
    /// $0.001 a page).
    #[uniffi(default = None)]
    pub source: Option<String>,
    /// A previous page's `nextCursor`, with the same request otherwise. Crossref
    /// sorted by `newest` pages by offset, and orders works sharing a date
    /// differently from one request to the next, so its pages may repeat
    /// or skip such works.
    #[uniffi(default = None)]
    pub cursor: Option<String>,
    /// An author's name. Fields are joined with AND to `text`, which may then be
    /// empty; a source that can't search one fails (`unsupported`), as
    /// `searchCapabilities` tells.
    #[uniffi(default = None)]
    pub author: Option<String>,
    /// Words of the title.
    #[uniffi(default = None)]
    pub title: Option<String>,
    /// Where the work appeared: the journal, or the book for a chapter.
    #[uniffi(default = None)]
    pub container: Option<String>,
    /// The order of hits; `None` is relevance. A source without the order
    /// fails (`unsupported`).
    #[uniffi(default = None)]
    pub sort: Option<SearchSort>,
    /// Provider-specific parameters, passed through by name: OpenAlex
    /// `filter` and `sort`, Crossref `query.*`, Europe PMC `query` (its
    /// own syntax), …. A name the source doesn't know fails.
    #[uniffi(default)]
    pub extra: HashMap<String, String>,
    /// The search as a tree of all, any and none, instead of `text`,
    /// `author`, `title` and `container`: giving it with any of those fails
    /// (`badRequest`). The other filters, the sort, paging, `source` and
    /// `extra` still apply. A tree the source can't say fails
    /// (`unsupported`), as `searchSupports` tells beforehand.
    #[uniffi(default = None)]
    pub query: Option<SearchNode>,
    /// Only works the source added on or after this day, `YYYY-MM-DD`
    /// (first deposited or indexed), whenever they were published: for
    /// watching a search. Without a `source`, the first that can
    /// (`searchCapabilities`' filters hold `since`). A source that can't
    /// fails (`unsupported`) and says why, never ignoring it; a date that
    /// isn't one fails (`badRequest`).
    #[uniffi(default = None)]
    pub since: Option<String>,
}

/// A search as a tree. Groups hold any number of nodes, groups included
/// (where `SourceSearch.nesting`); empty groups are dropped, and an all or
/// any of one node is that node.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum SearchNode {
    /// Every node matches.
    All {
        nodes: Vec<SearchNode>,
    },
    /// At least one node matches (where `SourceSearch.any` allows).
    Any {
        nodes: Vec<SearchNode>,
    },
    /// No node matches (where `SourceSearch.none` allows).
    None {
        nodes: Vec<SearchNode>,
    },
    /// Words in a field, or with `phrase`, the words together in order.
    /// The value is searched as written: quotes and operators in it are
    /// never syntax.
    Term {
        field: SearchField,
        value: String,
        phrase: bool,
    },
    Filter {
        filter: SearchFilterValue,
    },
    /// Works citing, or cited by, the work `id` names: any identifier
    /// `lookup` takes. Aspero looks it up (through the cache) to find the
    /// work at the source; a work it can't find fails the search.
    Relation {
        relation: SearchRelation,
        id: String,
    },
}

impl From<SearchNode> for QueryNode {
    fn from(node: SearchNode) -> Self {
        let nodes = |nodes: Vec<SearchNode>| nodes.into_iter().map(QueryNode::from).collect();
        match node {
            SearchNode::All { nodes: n } => QueryNode::All(nodes(n)),
            SearchNode::Any { nodes: n } => QueryNode::Any(nodes(n)),
            SearchNode::None { nodes: n } => QueryNode::None(nodes(n)),
            SearchNode::Term {
                field,
                value,
                phrase: false,
            } => QueryNode::term(field.into(), value),
            SearchNode::Term { field, value, .. } => QueryNode::phrase(field.into(), value),
            SearchNode::Filter { filter } => QueryNode::Filter(filter.into()),
            SearchNode::Relation { relation, id } => QueryNode::relation(relation.into(), id),
        }
    }
}

/// A filter in a query tree.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum SearchFilterValue {
    /// Published from `from` to `to`, inclusive; either may be open.
    Years { from: Option<i16>, to: Option<i16> },
    /// A CSL type: `article-journal`, `book`, ….
    WorkType { r#type: String },
    /// Whether a free, legal copy exists.
    OpenAccess { is_oa: bool },
    /// Leaves out retracted works.
    NotRetracted,
    /// An ISO 639-1 code, like `en`; aspero gives each source its own form.
    Language { code: String },
    /// Only works with an abstract.
    HasAbstract,
}

impl From<SearchFilterValue> for FilterValue {
    fn from(filter: SearchFilterValue) -> Self {
        match filter {
            SearchFilterValue::Years { from, to } => FilterValue::Years(YearRange { from, to }),
            SearchFilterValue::WorkType { r#type } => {
                FilterValue::WorkType(WorkType::from_csl(&r#type))
            }
            SearchFilterValue::OpenAccess { is_oa } => FilterValue::OpenAccess(is_oa),
            SearchFilterValue::NotRetracted => FilterValue::NotRetracted,
            SearchFilterValue::Language { code } => FilterValue::Language(code),
            SearchFilterValue::HasAbstract => FilterValue::HasAbstract,
        }
    }
}

impl SearchFilterValue {
    #[cfg(test)]
    pub(super) fn from_core(filter: &FilterValue) -> Option<Self> {
        Some(match filter {
            FilterValue::Years(y) => SearchFilterValue::Years {
                from: y.from,
                to: y.to,
            },
            FilterValue::WorkType(t) => SearchFilterValue::WorkType {
                r#type: t.as_csl().to_owned(),
            },
            FilterValue::OpenAccess(is_oa) => SearchFilterValue::OpenAccess { is_oa: *is_oa },
            FilterValue::NotRetracted => SearchFilterValue::NotRetracted,
            FilterValue::Language(code) => SearchFilterValue::Language { code: code.clone() },
            FilterValue::HasAbstract => SearchFilterValue::HasAbstract,
            _ => return None,
        })
    }
}

/// How one work relates to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SearchRelation {
    /// Works citing the given work.
    Cites,
    /// Works the given work cites.
    CitedBy,
}

impl SearchRelation {
    pub(super) fn from_core(relation: Relation) -> Option<Self> {
        match relation {
            Relation::Cites => Some(SearchRelation::Cites),
            Relation::CitedBy => Some(SearchRelation::CitedBy),
            _ => None,
        }
    }
}

impl From<SearchRelation> for Relation {
    fn from(relation: SearchRelation) -> Self {
        match relation {
            SearchRelation::Cites => Relation::Cites,
            SearchRelation::CitedBy => Relation::CitedBy,
        }
    }
}

/// How far a source can combine terms with any or none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SearchBoolean {
    No,
    /// Only among terms in one field, or values of one filter.
    WithinField,
    /// Across fields.
    Anywhere,
}

impl SearchBoolean {
    pub(super) fn from_core(b: BooleanSupport) -> Self {
        match b {
            BooleanSupport::WithinField => SearchBoolean::WithinField,
            BooleanSupport::Anywhere => SearchBoolean::Anywhere,
            _ => SearchBoolean::No,
        }
    }
}

/// The order of search hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SearchSort {
    /// Best match first; with only fields to go on, the source's own order.
    Relevance,
    /// Most recently published first.
    Newest,
    MostCited,
}

impl SearchSort {
    pub(super) fn from_core(sort: Sort) -> Option<Self> {
        match sort {
            Sort::Relevance => Some(SearchSort::Relevance),
            Sort::Newest => Some(SearchSort::Newest),
            Sort::MostCited => Some(SearchSort::MostCited),
            _ => None,
        }
    }
}

impl From<SearchSort> for Sort {
    fn from(sort: SearchSort) -> Self {
        match sort {
            SearchSort::Relevance => Sort::Relevance,
            SearchSort::Newest => Sort::Newest,
            SearchSort::MostCited => Sort::MostCited,
        }
    }
}

/// A field a search can name ([`SearchRequest`]'s `author`, `title`,
/// `container`).
///
/// In a query tree, the rest too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SearchField {
    Author,
    Title,
    Container,
    /// Plain text, wherever the source searches it: every source.
    Keywords,
    Abstract,
    /// An ORCID (bare or as a URL) or an OpenAlex author id (`A…`);
    /// anything else fails (`badRequest`).
    AuthorId,
    /// In a query tree only: the work an identifier names, any `lookup`
    /// takes (a DOI, bare, `doi:` or as a URL; `pmid:`, `pmcid:`,
    /// `arXiv:`, an OpenAlex `W…` id, an ISBN). One that isn't fails
    /// (`badRequest`); `phrase` means nothing. Sources take some kinds
    /// only (Crossref DOIs, Open Library ISBNs, …): `searchSupports` says
    /// which, and why not.
    Identifier,
}

impl SearchField {
    pub(super) fn from_core(filter: Filter) -> Option<Self> {
        match filter {
            Filter::Author => Some(SearchField::Author),
            Filter::Title => Some(SearchField::Title),
            Filter::Container => Some(SearchField::Container),
            Filter::Keywords => Some(SearchField::Keywords),
            Filter::Abstract => Some(SearchField::Abstract),
            Filter::AuthorId => Some(SearchField::AuthorId),
            Filter::Identifier => Some(SearchField::Identifier),
            _ => None,
        }
    }
}

impl From<SearchField> for TermField {
    fn from(field: SearchField) -> Self {
        match field {
            SearchField::Author => TermField::Author,
            SearchField::Title => TermField::Title,
            SearchField::Container => TermField::Container,
            SearchField::Keywords => TermField::Keywords,
            SearchField::Abstract => TermField::Abstract,
            SearchField::AuthorId => TermField::AuthorId,
            SearchField::Identifier => TermField::Identifier,
        }
    }
}

/// A filter a search can apply. One a source lacks is ignored, but for
/// `since`.
///
/// In a query tree, one a source lacks fails (`unsupported`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SearchFilter {
    /// `yearFrom` and `yearTo`.
    Years,
    WorkType,
    /// `openAccessOnly`.
    OpenAccess,
    /// In a query tree only, as are the rest.
    NotRetracted,
    Language,
    HasAbstract,
    /// `since`, never in a query tree. A source without it fails
    /// (`unsupported`) rather than ignoring it.
    Since,
}

impl SearchFilter {
    pub(super) fn from_core(filter: Filter) -> Option<Self> {
        match filter {
            Filter::Years => Some(SearchFilter::Years),
            Filter::WorkType => Some(SearchFilter::WorkType),
            Filter::OpenAccess => Some(SearchFilter::OpenAccess),
            Filter::NotRetracted => Some(SearchFilter::NotRetracted),
            Filter::Language => Some(SearchFilter::Language),
            Filter::HasAbstract => Some(SearchFilter::HasAbstract),
            Filter::Since => Some(SearchFilter::Since),
            _ => None,
        }
    }
}

/// What one source can search by.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SourceSearch {
    /// As `SearchRequest.source` and `SearchResults.provider` name it.
    pub source: String,
    pub fields: Vec<SearchField>,
    pub filters: Vec<SearchFilter>,
    pub sorts: Vec<SearchSort>,
    /// How far a query tree's any can reach: `withinField` is terms of one
    /// field (or values of one filter) only.
    pub any: SearchBoolean,
    /// How far a query tree's none can reach.
    pub none: SearchBoolean,
    /// Whether a query tree's groups can hold groups.
    pub nesting: bool,
    pub relations: Vec<SearchRelation>,
}

impl SourceSearch {
    pub(crate) fn new(source: &ProviderId, caps: &Capabilities) -> Self {
        let filters = &caps.search_filters;
        SourceSearch {
            source: source.to_string(),
            fields: filters
                .iter()
                .filter_map(|f| SearchField::from_core(*f))
                .collect(),
            filters: filters
                .iter()
                .filter_map(|f| SearchFilter::from_core(*f))
                .collect(),
            sorts: caps
                .sorts
                .iter()
                .filter_map(|s| SearchSort::from_core(*s))
                .collect(),
            any: SearchBoolean::from_core(caps.search_any),
            none: SearchBoolean::from_core(caps.search_none),
            nesting: caps.search_nesting,
            relations: caps
                .relations
                .iter()
                .filter_map(|r| SearchRelation::from_core(*r))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SearchHit {
    pub work: Work,
    /// The provider's relevance score; `nil` when it gives none (OpenAlex
    /// sorted by citations).
    pub score: Option<f64>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SearchResults {
    /// Who answered.
    pub provider: Option<String>,
    pub hits: Vec<SearchHit>,
    /// Total matches, if the provider reports it.
    pub total: Option<u64>,
    /// The cursor for the next page; none on the last.
    pub next_cursor: Option<String>,
    pub served: Option<Freshness>,
    /// Why there are no hits, if the search failed.
    pub failure: Option<Failure>,
    pub usage: Vec<ProviderUsage>,
    /// Filters asked for that the source can't apply, so ignored (a query
    /// tree's never are: they fail).
    #[uniffi(default = [])]
    pub ignored_filters: Vec<SearchFilter>,
}

impl SearchResults {
    pub(crate) fn new(result: Result<engine::SearchReport, engine::EngineError>) -> Self {
        match result {
            Err(e) => SearchResults {
                provider: None,
                hits: Vec::new(),
                total: None,
                next_cursor: None,
                served: None,
                failure: Some(Failure::from(&e)),
                usage: Vec::new(),
                ignored_filters: Vec::new(),
            },
            Ok(report) => SearchResults {
                provider: Some(report.provider.to_string()),
                hits: report
                    .page
                    .hits
                    .iter()
                    .map(|h| SearchHit {
                        work: Work::from_record(&h.record),
                        score: h.score,
                    })
                    .collect(),
                total: report.page.total,
                next_cursor: report.page.next.map(|c| c.0),
                served: Some(report.served.into()),
                failure: None,
                usage: report
                    .usage
                    .map(|u| usage(&[(report.provider.clone(), u)]))
                    .unwrap_or_default(),
                ignored_filters: report
                    .ignored_filters
                    .iter()
                    .filter_map(|&f| SearchFilter::from_core(f))
                    .collect(),
            },
        }
    }
}
