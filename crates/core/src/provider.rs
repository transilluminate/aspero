//! The `Provider` trait: the contract every literature source implements.
//!
//! Rules every implementation follows (the engine relies on them):
//!
//! 1. **No merging, no caching, no retries.** A provider makes the request,
//!    maps the response, and returns. Retries, backoff, rate limiting, caching
//!    and merging are the engine's job, so they are consistent and shared.
//! 2. **Declared capabilities are honoured.** Every [`Filter`] listed in
//!    [`Capabilities::search_filters`] is applied when present in a query,
//!    and every [`Sort`] in [`Capabilities::sorts`] honoured. The engine
//!    reports filters a provider doesn't declare as ignored, so nothing is
//!    dropped silently, and refuses fields and sorts it doesn't declare with
//!    [`Error::Unsupported`]. Unknown keys in [`SearchQuery::extra`] are
//!    rejected with [`Error::BadRequest`].
//! 3. **Batch lookups answer every ID.** [`Provider::lookup`] returns one
//!    [`Lookup`] per input ID, in input order, with `record: None` for IDs the
//!    provider doesn't know. Whole-call failures are `Err`.
//! 4. **Errors are mapped** into [`crate::Error`], including non-provider
//!    bodies (e.g. an API gateway's 429 page).
//! 5. **Usage is reported** when the provider exposes it (credits spent,
//!    budget remaining), so agents can pace themselves.
//!
//! Each provider also owns its HTTP mapping, credentials and default
//! [`ResiliencePolicy`](crate::ResiliencePolicy).

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;

use async_trait::async_trait;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::id::{IdKind, WorkId};
use crate::query::{BooleanSupport, QueryNode, Relation};
use crate::record::{FieldSet, SourceRecord};
use crate::work::WorkType;

/// Stable short name of a provider, e.g. `openalex`. Lowercase ASCII letters,
/// digits and `-`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct ProviderId(Cow<'static, str>);

impl ProviderId {
    /// For built-in providers.
    ///
    /// # Panics
    /// If `id` is not a valid provider name (checked at compile time when used
    /// in a `const`).
    pub const fn from_static(id: &'static str) -> Self {
        assert!(Self::is_valid(id), "invalid provider id");
        ProviderId(Cow::Borrowed(id))
    }

    pub fn parse(id: &str) -> Option<Self> {
        Self::is_valid(id).then(|| ProviderId(Cow::Owned(id.to_owned())))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    const fn is_valid(id: &str) -> bool {
        let b = id.as_bytes();
        if b.is_empty() {
            return false;
        }
        let mut i = 0;
        while i < b.len() {
            if !(b[i].is_ascii_lowercase() || b[i].is_ascii_digit() || b[i] == b'-') {
                return false;
            }
            i += 1;
        }
        true
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ProviderId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        ProviderId::parse(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("invalid provider id {s:?}")))
    }
}

/// What a provider can do. The engine routes on this before calling.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Capabilities {
    /// Identifier kinds [`Provider::lookup`] accepts.
    pub lookup_kinds: Vec<IdKind>,
    /// Most IDs per `lookup` call; the engine splits larger batches. 0 is
    /// treated as 1.
    pub max_batch: usize,
    /// Fields this provider can supply at all.
    pub fields: FieldSet,
    pub search: bool,
    /// Supports [`SearchMode::Citation`]: a matcher built for reference
    /// strings, not just keyword search.
    pub citation_search: bool,
    /// Reports editorial notices in detail and in both directions (a work's
    /// corrections, a correction's work), so the engine asks it about
    /// records other providers supplied.
    pub notices: bool,
    /// Filters and fields [`Provider::search`] applies.
    pub search_filters: Vec<Filter>,
    /// Orders [`Provider::search`] can return hits in. Every searching
    /// provider lists [`Sort::Relevance`].
    pub sorts: Vec<Sort>,
    /// How far a query tree's any-of groups can reach.
    pub search_any: BooleanSupport,
    /// How far a query tree's none-of groups can reach.
    pub search_none: BooleanSupport,
    /// Whether a query tree's groups can hold groups.
    pub search_nesting: bool,
    /// Relations a query tree can search by.
    pub relations: Vec<Relation>,
    /// Identifier kinds whose "not found" here is final: no other provider
    /// can know them (doi.org for DOIs), so lookups stop asking.
    pub final_misses: Vec<IdKind>,
    pub citations: bool,
    pub references: bool,
    pub recommend: bool,
    /// [`Provider::budget`] can say what's left without spending any
    /// (OpenAlex, with a key).
    pub budget_status: bool,
    /// Lookups of at most this many IDs cost nothing (OpenAlex: 1, its
    /// singleton endpoint); 0 when none are known to be free. Under a
    /// daily cap, the engine looks IDs up this many at a time.
    pub max_free_batch: usize,
    /// Pages of one search's results a caller can read; 0 for no limit.
    /// OpenAlex without a key: 1 (paging on needs a key).
    pub max_pages: usize,
}

/// Common search filters (see [`SearchQuery`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Filter {
    Years,
    WorkType,
    OpenAccess,
    /// Volume and first page ([`SearchQuery::place`]).
    Place,
    /// [`SearchQuery::author`].
    Author,
    /// [`SearchQuery::title`].
    Title,
    /// [`SearchQuery::container`].
    Container,
    /// Plain text: every searching provider has it. Named only in query
    /// trees; [`SearchQuery::text`] needs no capability.
    Keywords,
    Abstract,
    /// An ORCID or an OpenAlex author id.
    AuthorId,
    /// Leaves out retracted works.
    NotRetracted,
    /// The language a work is written in.
    Language,
    /// Only works with an abstract.
    HasAbstract,
    /// Only works the source added on or after a date
    /// ([`SearchQuery::since`]). A source lacking it fails rather than
    /// ignoring it: a watch would otherwise see old works as new.
    Since,
    /// Works named by an identifier, in a query tree. Which kinds a source
    /// takes is said by [`Provider::express`] (for a kind it can't, why).
    Identifier,
}

impl Filter {
    /// Every filter, for front ends to test their mappings.
    pub const ALL: &[Filter] = &[
        Filter::Years,
        Filter::WorkType,
        Filter::OpenAccess,
        Filter::Place,
        Filter::Author,
        Filter::Title,
        Filter::Container,
        Filter::Keywords,
        Filter::Abstract,
        Filter::AuthorId,
        Filter::NotRetracted,
        Filter::Language,
        Filter::HasAbstract,
        Filter::Since,
        Filter::Identifier,
    ];

    /// As serialized: `years`, `work_type`, ….
    pub fn as_str(self) -> &'static str {
        match self {
            Filter::Years => "years",
            Filter::WorkType => "work_type",
            Filter::OpenAccess => "open_access",
            Filter::Place => "place",
            Filter::Author => "author",
            Filter::Title => "title",
            Filter::Container => "container",
            Filter::Keywords => "keywords",
            Filter::Abstract => "abstract",
            Filter::AuthorId => "author_id",
            Filter::NotRetracted => "not_retracted",
            Filter::Language => "language",
            Filter::HasAbstract => "has_abstract",
            Filter::Since => "since",
            Filter::Identifier => "identifier",
        }
    }

    /// Whether this filter narrows by a field of the work (keywords,
    /// author, title, container, abstract, author id, identifier). A
    /// source lacking one fails rather than ignoring it.
    pub fn is_field(self) -> bool {
        matches!(
            self,
            Filter::Author
                | Filter::Title
                | Filter::Container
                | Filter::Keywords
                | Filter::Abstract
                | Filter::AuthorId
                | Filter::Identifier
        )
    }
}

/// The order of search hits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Sort {
    /// Best match first; without text to rank by, the source's own order.
    #[default]
    Relevance,
    /// Most recently published first.
    Newest,
    /// Most cited first.
    MostCited,
}

impl Sort {
    /// Every sort, for front ends to test their mappings.
    pub const ALL: &[Sort] = &[Sort::Relevance, Sort::Newest, Sort::MostCited];

    /// As serialized: `relevance`, `newest` or `most_cited`.
    pub fn as_str(self) -> &'static str {
        match self {
            Sort::Relevance => "relevance",
            Sort::Newest => "newest",
            Sort::MostCited => "most_cited",
        }
    }
}

/// Where a work appeared, to search by: volume and first page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Place {
    pub volume: String,
    pub first_page: String,
}

impl Place {
    pub fn new(volume: impl Into<String>, first_page: impl Into<String>) -> Self {
        Place {
            volume: volume.into(),
            first_page: first_page.into(),
        }
    }
}

/// Inclusive publication-year range; either end may be open.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct YearRange {
    pub from: Option<i16>,
    pub to: Option<i16>,
}

/// An opaque, provider-specific continuation token.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Cursor(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PageReq {
    /// Requested page size. Providers cap it at their own maximum and may
    /// return fewer, never more.
    pub limit: u32,
    /// `None` for the first page.
    pub cursor: Option<Cursor>,
}

impl PageReq {
    pub const DEFAULT_LIMIT: u32 = 25;

    pub fn first(limit: u32) -> Self {
        PageReq {
            limit,
            cursor: None,
        }
    }
}

impl Default for PageReq {
    fn default() -> Self {
        PageReq::first(Self::DEFAULT_LIMIT)
    }
}

/// How [`SearchQuery::text`] should be interpreted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SearchMode {
    /// Free-text keywords.
    #[default]
    Keywords,
    /// A whole, possibly messy reference string ("LeCun Y, Bengio Y. Deep
    /// learning. Nature. 2015"), to be matched to the work it cites. Only
    /// sent to providers declaring [`Capabilities::citation_search`].
    Citation,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SearchQuery {
    /// May be empty when a field (author, title, container) or a tree is
    /// given.
    pub text: String,
    pub mode: SearchMode,
    /// An author's name, ANDed with the rest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Words of the title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Where the work appeared: the journal, or the book for a chapter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    /// The search as a tree of all, any and none: instead of `text`,
    /// `author`, `title`, `container` and `place`, which must then be
    /// empty. The engine folds `years`, `work_type` and `open_access` into
    /// it, so a provider reads only the tree (see [`crate::query`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree: Option<QueryNode>,
    /// Sources fail with [`Error::Unsupported`] for a sort they lack.
    #[serde(default, skip_serializing_if = "is_relevance")]
    pub sort: Sort,
    pub years: Option<YearRange>,
    pub work_type: Option<WorkType>,
    pub open_access: Option<bool>,
    /// Volume and first page, for citations that give no title.
    pub place: Option<Place>,
    /// Only works the source added on or after this date (first deposited
    /// or indexed), whenever they were published: for watching a search.
    /// Sources fail with [`Error::UnsupportedQuery`] without
    /// [`Filter::Since`]; it is never ignored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<jiff::civil::Date>,
    pub fields: FieldSet,
    pub page: PageReq,
    /// Provider-specific parameters, passed through by name.
    pub extra: BTreeMap<String, String>,
}

impl SearchQuery {
    pub fn new(text: impl Into<String>) -> Self {
        SearchQuery {
            text: text.into(),
            fields: FieldSet::DEFAULT,
            ..SearchQuery::default()
        }
    }

    /// A [`SearchMode::Citation`] query for a reference string.
    pub fn citation(reference: impl Into<String>) -> Self {
        SearchQuery {
            mode: SearchMode::Citation,
            ..SearchQuery::new(reference)
        }
    }

    /// A [`since`](SearchQuery::since) as written, `YYYY-MM-DD`, or why
    /// it isn't one, in English.
    pub fn parse_since(s: &str) -> std::result::Result<jiff::civil::Date, String> {
        let t = s.trim();
        let shaped = t.len() == 10
            && t.bytes().enumerate().all(|(i, b)| {
                if i == 4 || i == 7 {
                    b == b'-'
                } else {
                    b.is_ascii_digit()
                }
            });
        shaped
            .then(|| t.parse().ok())
            .flatten()
            .ok_or_else(|| format!("invalid date {s:?}: expected YYYY-MM-DD"))
    }

    /// The common filters this query actually uses.
    pub fn filters(&self) -> Vec<Filter> {
        [
            self.years.map(|_| Filter::Years),
            self.work_type.as_ref().map(|_| Filter::WorkType),
            self.open_access.map(|_| Filter::OpenAccess),
            self.place.as_ref().map(|_| Filter::Place),
            self.field(Filter::Author).map(|_| Filter::Author),
            self.field(Filter::Title).map(|_| Filter::Title),
            self.field(Filter::Container).map(|_| Filter::Container),
            self.since.map(|_| Filter::Since),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// A field's value, trimmed; `None` when absent or blank, or for a
    /// filter that isn't a field.
    pub fn field(&self, field: Filter) -> Option<&str> {
        let value = match field {
            Filter::Author => &self.author,
            Filter::Title => &self.title,
            Filter::Container => &self.container,
            _ => return None,
        };
        value.as_deref().map(str::trim).filter(|v| !v.is_empty())
    }
}

// Keeps queries without a sort keyed as before in caches.
fn is_relevance(sort: &Sort) -> bool {
    *sort == Sort::Relevance
}

/// The answer for one requested ID in a batch lookup.
#[derive(Debug, Clone, PartialEq)]
pub struct Lookup {
    /// The ID as requested.
    pub id: WorkId,
    /// `None`: the provider doesn't know this ID.
    pub record: Option<SourceRecord>,
}

/// A page of works: search hits in rank order, or one side of the citation graph.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Page {
    pub hits: Vec<Hit>,
    /// Total matches, if the provider reports it.
    pub total: Option<u64>,
    /// `None` on the last page.
    pub next: Option<Cursor>,
}

impl Page {
    pub fn new(hits: Vec<Hit>, total: Option<u64>, next: Option<Cursor>) -> Self {
        Page { hits, total, next }
    }
}

/// One work in a [`Page`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hit {
    pub record: SourceRecord,
    /// The provider's relevance score, if it gives one. Only comparable
    /// within one provider's results for one query.
    pub score: Option<f64>,
}

impl Hit {
    pub fn new(record: SourceRecord, score: Option<f64>) -> Self {
        Hit { record, score }
    }
}

/// Usage accounting reported by the provider, in its own units (OpenAlex:
/// USD). Any part may be unknown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Usage {
    /// Spent by this call.
    pub cost: Option<f64>,
    /// Left in the current budget period.
    pub remaining: Option<f64>,
    /// When the budget resets.
    pub resets_at: Option<Timestamp>,
    /// The budget for the period (OpenAlex: the daily allowance), as the
    /// provider says it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<f64>,
    /// A prepaid balance left beside the period's budget, where the
    /// provider says one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepaid_remaining: Option<f64>,
}

impl Usage {
    /// Usage of this call followed by `later`: costs add up; the remaining
    /// budget and reset time are the most recent known.
    pub fn then(self, later: Usage) -> Usage {
        Usage {
            cost: match (self.cost, later.cost) {
                (Some(a), Some(b)) => Some(a + b),
                (a, b) => a.or(b),
            },
            remaining: later.remaining.or(self.remaining),
            resets_at: later.resets_at.or(self.resets_at),
            limit: later.limit.or(self.limit),
            prepaid_remaining: later.prepaid_remaining.or(self.prepaid_remaining),
        }
    }
}

/// A provider result plus the usage it cost.
#[derive(Debug, Clone, PartialEq)]
pub struct Response<T> {
    pub value: T,
    pub usage: Option<Usage>,
}

impl<T> Response<T> {
    pub fn new(value: T) -> Self {
        Response { value, usage: None }
    }

    pub fn with_usage(value: T, usage: Usage) -> Self {
        Response {
            value,
            usage: Some(usage),
        }
    }
}

/// A source of scholarly metadata. See the [module docs](self) for the rules
/// implementations follow.
///
/// Operations default to [`Error::Unsupported`]; a provider overrides those
/// it declares in [`Provider::capabilities`].
#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> ProviderId;

    fn capabilities(&self) -> &Capabilities;

    /// Looks up works by ID. One [`Lookup`] per input, in input order.
    /// Callers pass at most `max_batch` IDs, all of a kind in `lookup_kinds`.
    async fn lookup(&self, ids: &[WorkId], fields: FieldSet) -> Result<Response<Vec<Lookup>>> {
        let _ = (ids, fields);
        Err(Error::Unsupported("lookup"))
    }

    async fn search(&self, query: &SearchQuery) -> Result<Response<Page>> {
        let _ = query;
        Err(Error::Unsupported("search"))
    }

    /// Whether [`search`](Provider::search) can say `tree` (normalised and
    /// validated, see [`crate::query`]), and if not, why, in English.
    /// Searching with a tree this refuses fails with
    /// [`Error::UnsupportedQuery`] and the same reason, so one translation
    /// decides both.
    fn express(&self, tree: &QueryNode) -> std::result::Result<(), String> {
        let _ = tree;
        Err(format!("{} can't search by a query tree", self.id()))
    }

    /// Why [`search`](Provider::search) can't take
    /// [`SearchQuery::since`], in English, when
    /// [`Capabilities::search_filters`] lacks [`Filter::Since`]. Searching
    /// with one fails with [`Error::UnsupportedQuery`] and this reason.
    fn since_unsupported(&self) -> String {
        format!("{} can't search by date added", self.id())
    }

    /// What is left of the provider's budget, asked without spending any
    /// (OpenAlex's `/rate-limit`). Called only when
    /// [`Capabilities::budget_status`] says it can be.
    async fn budget(&self) -> Result<Usage> {
        Err(Error::Unsupported("budget status"))
    }

    /// Works that cite `id`.
    async fn citations(
        &self,
        id: &WorkId,
        page: &PageReq,
        fields: FieldSet,
    ) -> Result<Response<Page>> {
        let _ = (id, page, fields);
        Err(Error::Unsupported("citations"))
    }

    /// Works that `id` cites.
    async fn references(
        &self,
        id: &WorkId,
        page: &PageReq,
        fields: FieldSet,
    ) -> Result<Response<Page>> {
        let _ = (id, page, fields);
        Err(Error::Unsupported("references"))
    }

    /// Works similar to `positive` and unlike `negative`.
    async fn recommend(
        &self,
        positive: &[WorkId],
        negative: &[WorkId],
        page: &PageReq,
        fields: FieldSet,
    ) -> Result<Response<Page>> {
        let _ = (positive, negative, page, fields);
        Err(Error::Unsupported("recommend"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Nothing(Capabilities);

    #[async_trait]
    impl Provider for Nothing {
        fn id(&self) -> ProviderId {
            ProviderId::from_static("nothing")
        }

        fn capabilities(&self) -> &Capabilities {
            &self.0
        }
    }

    #[test]
    fn usage_adds_costs_and_keeps_latest_budget() {
        let first = Usage {
            cost: Some(0.001),
            remaining: Some(0.9),
            limit: Some(1.0),
            prepaid_remaining: Some(2.0),
            ..Usage::default()
        };
        let second = Usage {
            cost: Some(0.0001),
            prepaid_remaining: Some(1.5),
            ..Usage::default()
        };
        let total = first.then(second);
        assert!((total.cost.unwrap() - 0.0011).abs() < 1e-12);
        assert_eq!(total.remaining, Some(0.9));
        assert_eq!(total.limit, Some(1.0));
        assert_eq!(total.prepaid_remaining, Some(1.5));
    }

    #[test]
    fn provider_ids_are_validated() {
        assert!(ProviderId::parse("openalex").is_some());
        assert!(ProviderId::parse("s2-graph").is_some());
        assert!(ProviderId::parse("OpenAlex").is_none());
        assert!(ProviderId::parse("").is_none());
        assert!(serde_json::from_str::<ProviderId>(r#""bad id""#).is_err());
    }

    #[test]
    fn query_reports_the_filters_it_uses() {
        let mut q = SearchQuery::new("transformers");
        assert!(q.filters().is_empty());
        q.years = Some(YearRange {
            from: Some(2017),
            to: None,
        });
        q.open_access = Some(true);
        assert_eq!(q.filters(), [Filter::Years, Filter::OpenAccess]);
        q.author = Some("LeCun".into());
        q.container = Some("  ".into());
        assert_eq!(
            q.filters(),
            [Filter::Years, Filter::OpenAccess, Filter::Author]
        );
        q.since = Some(jiff::civil::date(2026, 11, 1));
        assert_eq!(q.filters().last(), Some(&Filter::Since));
    }

    #[test]
    fn every_filter_and_sort_is_listed_under_its_name() {
        let position = |f: Filter| match f {
            Filter::Years => 0,
            Filter::WorkType => 1,
            Filter::OpenAccess => 2,
            Filter::Place => 3,
            Filter::Author => 4,
            Filter::Title => 5,
            Filter::Container => 6,
            Filter::Keywords => 7,
            Filter::Abstract => 8,
            Filter::AuthorId => 9,
            Filter::NotRetracted => 10,
            Filter::Language => 11,
            Filter::HasAbstract => 12,
            Filter::Since => 13,
            Filter::Identifier => 14,
        };
        for (i, f) in Filter::ALL.iter().enumerate() {
            assert_eq!(position(*f), i);
            assert_eq!(serde_json::to_value(f).unwrap(), f.as_str());
        }
        assert_eq!(Filter::ALL.len(), position(Filter::Identifier) + 1);
        let position = |s: Sort| match s {
            Sort::Relevance => 0,
            Sort::Newest => 1,
            Sort::MostCited => 2,
        };
        for (i, s) in Sort::ALL.iter().enumerate() {
            assert_eq!(position(*s), i);
            assert_eq!(serde_json::to_value(s).unwrap(), s.as_str());
        }
        assert_eq!(Sort::ALL.len(), position(Sort::MostCited) + 1);
    }

    #[test]
    fn since_dates_are_iso_days() {
        assert_eq!(
            SearchQuery::parse_since(" 2026-11-01 "),
            Ok(jiff::civil::date(2026, 11, 1))
        );
        for bad in ["2026-11", "20261101", "2026-02-30", "1 Nov 2026", ""] {
            assert_eq!(
                SearchQuery::parse_since(bad),
                Err(format!("invalid date {bad:?}: expected YYYY-MM-DD"))
            );
        }
    }

    #[test]
    fn queries_without_fields_or_sort_serialize_as_before() {
        let json = serde_json::to_string(&SearchQuery::new("x")).unwrap();
        assert!(!json.contains("\"author\"") && !json.contains("\"sort\""));
        assert!(!json.contains("\"tree\"") && !json.contains("\"since\""));
        let mut q = SearchQuery::new("x");
        q.sort = Sort::MostCited;
        q.since = Some(jiff::civil::date(2026, 11, 1));
        let json = serde_json::to_string(&q).unwrap();
        assert!(json.contains(r#""since":"2026-11-01""#), "{json}");
        let back: SearchQuery = serde_json::from_str(&json).unwrap();
        assert_eq!(back, q);
    }

    #[test]
    fn unimplemented_operations_are_unsupported_and_dyn_compatible() {
        let p: Box<dyn Provider> = Box::new(Nothing(Capabilities::default()));
        let query = SearchQuery::new("x");
        let result = block_on_ready(p.search(&query));
        assert!(matches!(result, Err(Error::Unsupported("search"))));
    }

    /// Polls a future that is immediately ready (no runtime in `core`).
    fn block_on_ready<F: std::future::Future>(fut: F) -> F::Output {
        use std::task::{Context, Poll, Waker};
        let mut fut = std::pin::pin!(fut);
        match fut.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(out) => out,
            Poll::Pending => panic!("future was not immediately ready"),
        }
    }
}
