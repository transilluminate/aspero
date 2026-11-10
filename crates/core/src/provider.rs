//! The `Provider` trait: the contract every literature source implements.
//!
//! Rules every implementation follows (the engine relies on them):
//!
//! 1. **No merging, no caching, no retries.** A provider makes the request,
//!    maps the response, and returns. Retries, backoff, rate limiting, caching
//!    and merging are the engine's job, so they are consistent and shared.
//! 2. **Declared capabilities are honoured.** Every [`Filter`] listed in
//!    [`Capabilities::search_filters`] is applied when present in a query. The
//!    engine reports filters a provider doesn't declare as ignored, so nothing
//!    is dropped silently. Unknown keys in [`SearchQuery::extra`] are
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
    pub search_filters: Vec<Filter>,
    pub citations: bool,
    pub references: bool,
    pub recommend: bool,
}

/// Common search filters (see [`SearchQuery`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Filter {
    Years,
    WorkType,
    OpenAccess,
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SearchQuery {
    pub text: String,
    pub years: Option<YearRange>,
    pub work_type: Option<WorkType>,
    pub open_access: Option<bool>,
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

    /// The common filters this query actually uses.
    pub fn filters(&self) -> Vec<Filter> {
        [
            self.years.map(|_| Filter::Years),
            self.work_type.as_ref().map(|_| Filter::WorkType),
            self.open_access.map(|_| Filter::OpenAccess),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
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
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct Page {
    pub works: Vec<SourceRecord>,
    /// Total matches, if the provider reports it.
    pub total: Option<u64>,
    /// `None` on the last page.
    pub next: Option<Cursor>,
}

impl Page {
    pub fn new(works: Vec<SourceRecord>, total: Option<u64>, next: Option<Cursor>) -> Self {
        Page { works, total, next }
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
