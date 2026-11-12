//! Routing, caching, merging and resilience over registered providers.
//!
//! The engine owns what providers deliberately don't: retries and backoff
//! ([`retry`]), request pacing and the real HTTP client ([`transport`]), and
//! choosing which provider answers what.
//!
//! - **Lookup** is interchangeable across providers: each ID goes to the
//!   providers that accept its kind, in priority order, until one finds it.
//!   A provider that doesn't know an ID or fails falls through to the next;
//!   every attempt is reported.
//! - **Search** is not interchangeable (rankings differ), so it runs against
//!   one provider: the one named, or the first capable one. Filters that
//!   provider doesn't declare are stripped and reported as ignored, never
//!   silently dropped.
//!
//! - **Resolve / verify** turn a citation claim into scored candidates and a
//!   verdict (see [`resolve`](crate::Engine::resolve)).
//!
//! - **Caching**: provider answers (records, "not found", search pages) are
//!   cached with per-kind TTLs ([`CachePolicy`]) and served stale if a
//!   provider fails; every answer says how it was [`Served`].
//!
//! - **Sharing**: one engine serves many callers at once. Lookups already
//!   in flight are joined rather than repeated, and each caller sets its
//!   own patience with [`CallOptions`]. Providers that can't answer (a
//!   spent quota, a rejected key, repeated failures) are skipped until
//!   they can ([`Engine::unavailable`]).
//!
//! Not yet: fan-out search.

mod abstracts;
mod batch;
mod budget;
mod check;
mod enrich;
mod health;
mod identify;
mod inflight;
mod inputs;
mod lookup;
mod options;
mod registry;
mod resolve;
pub mod retry;
mod search;
mod standard;
pub mod transport;
mod verdict;
mod versions;

pub use abstracts::{Abstract, AbstractsReport};
pub use aspero_core::matching::WorkKind;
pub use batch::Batch;
pub use budget::Budget;
pub use check::{CheckReport, Checked, Finding, Severity, duplicates, findings};
pub use identify::{Decision, Identified, MAX_IDENTIFIERS};
pub use registry::{Disagreement, disagreements};
pub use resolve::{Candidate, ResolveReport, Sighting, Via};
/// Re-exported for credentials in provider configs.
pub use secrecy::SecretString;
#[cfg(feature = "crossref")]
pub use standard::CrossrefConfig;
#[cfg(feature = "datacite")]
pub use standard::DataciteConfig;
#[cfg(feature = "doi")]
pub use standard::DoiConfig;
#[cfg(feature = "europepmc")]
pub use standard::EuropePmcConfig;
#[cfg(feature = "openalex")]
pub use standard::OpenAlexConfig;
#[cfg(feature = "openlibrary")]
pub use standard::OpenLibraryConfig;
pub use standard::{StandardConfig, standard};
pub use verdict::{CitedField, Difference, Reason, Verdict, Verification, describe_update};
pub use versions::VersionsReport;

use std::sync::Arc;
use std::time::Duration;

use aspero_core::{
    Error, ErrorCategory, ErrorKind, Provider, ProviderId, ResiliencePolicy, Response,
    SourceRecord, Usage,
};
pub use aspero_store::{Cache, Pacing, SqliteCache, Stats as CacheStats, StoreError};
pub use inputs::{Extras, InputLookup, InputsReport, Looked};
use jiff::Timestamp;
pub use lookup::{Attempt, LookupReport, LookupResult, Outcome};
pub use options::CallOptions;
pub use search::SearchReport;

use crate::budget::Budgets;
use crate::health::Health;
use crate::inflight::InFlight;
use crate::retry::{Jitter, random_jitter, with_retries};

/// Results per provider search when resolving, verifying or checking, unless
/// the caller asks for another number.
pub const SEARCH_LIMIT: u32 = 5;

/// A clock, injectable so cache expiry is testable.
pub type Clock = fn() -> Timestamp;

/// How long cached answers are trusted (DESIGN §5). Past these, a provider
/// is asked again; if it then fails, the old answer is served as stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CachePolicy {
    pub records: Duration,
    /// "Not found" answers: short, since works get registered.
    pub misses: Duration,
    pub searches: Duration,
}

impl Default for CachePolicy {
    fn default() -> Self {
        const DAY: u64 = 86_400;
        CachePolicy {
            records: Duration::from_secs(7 * DAY),
            misses: Duration::from_secs(DAY),
            searches: Duration::from_secs(DAY),
        }
    }
}

/// Where an answer came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Served {
    Live,
    /// A cached answer within its TTL.
    Cache,
    /// An expired cached answer, served because the provider failed.
    Stale,
}

impl Served {
    pub fn as_str(self) -> &'static str {
        match self {
            Served::Live => "live",
            Served::Cache => "cache",
            Served::Stale => "stale",
        }
    }
}

/// Cheap to clone: clones, and views made with
/// [`with_options`](Engine::with_options), share the cache, providers'
/// pacing and requests in flight. Hold one per process.
#[derive(Clone)]
pub struct Engine {
    /// In priority order.
    entries: Vec<Entry>,
    jitter: Jitter,
    cache: Option<Arc<dyn Cache>>,
    cache_policy: CachePolicy,
    clock: Clock,
    options: CallOptions,
    inflight: Arc<InFlight>,
    health: Arc<Health>,
    budgets: Arc<Budgets>,
}

#[derive(Clone)]
struct Entry {
    provider: Arc<dyn Provider>,
    policy: ResiliencePolicy,
}

pub struct EngineBuilder {
    entries: Vec<(i32, Entry)>,
    jitter: Jitter,
    cache: Option<Arc<dyn Cache>>,
    cache_policy: CachePolicy,
    clock: Clock,
}

impl EngineBuilder {
    /// Registers a provider. Lower `priority` is tried first; ties keep
    /// registration order.
    pub fn provider(
        mut self,
        provider: Arc<dyn Provider>,
        policy: ResiliencePolicy,
        priority: i32,
    ) -> Self {
        self.entries.push((priority, Entry { provider, policy }));
        self
    }

    /// Replaces the backoff jitter source (for deterministic tests).
    pub fn jitter(mut self, jitter: Jitter) -> Self {
        self.jitter = jitter;
        self
    }

    /// Caches provider answers (and serves them stale if a provider fails).
    pub fn cache(mut self, cache: Arc<dyn Cache>) -> Self {
        self.cache = Some(cache);
        self
    }

    pub fn cache_policy(mut self, policy: CachePolicy) -> Self {
        self.cache_policy = policy;
        self
    }

    /// Replaces the clock (for deterministic cache-expiry tests).
    pub fn clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }

    pub fn build(mut self) -> Engine {
        self.entries.sort_by_key(|(priority, _)| *priority);
        Engine {
            entries: self.entries.into_iter().map(|(_, e)| e).collect(),
            jitter: self.jitter,
            cache: self.cache,
            cache_policy: self.cache_policy,
            clock: self.clock,
            options: CallOptions::default(),
            inflight: Arc::default(),
            health: Arc::default(),
            budgets: Arc::default(),
        }
    }
}

#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum EngineError {
    #[error("no enabled provider is named {0:?}")]
    UnknownProvider(String),
    #[error("no enabled provider supports {0}")]
    NoCapableProvider(&'static str),
    #[error("{provider}: {error}")]
    Provider {
        provider: ProviderId,
        #[source]
        error: Error,
    },
}

impl EngineError {
    pub fn category(&self) -> ErrorCategory {
        match self {
            EngineError::UnknownProvider(_) => ErrorCategory::Usage,
            EngineError::NoCapableProvider(_) => ErrorCategory::Unsupported,
            EngineError::Provider { error, .. } => error.category(),
        }
    }

    pub fn kind(&self) -> ErrorKind {
        match self {
            EngineError::UnknownProvider(_) => ErrorKind::BadRequest,
            EngineError::NoCapableProvider(_) => ErrorKind::Unsupported,
            EngineError::Provider { error, .. } => error.kind(),
        }
    }
}

impl Engine {
    pub fn builder() -> EngineBuilder {
        EngineBuilder {
            entries: Vec::new(),
            jitter: random_jitter,
            cache: None,
            cache_policy: CachePolicy::default(),
            clock: Timestamp::now,
        }
    }

    /// This engine for a caller with different patience; it shares
    /// everything else.
    pub fn with_options(&self, options: CallOptions) -> Engine {
        Engine {
            options,
            ..self.clone()
        }
    }

    pub fn options(&self) -> CallOptions {
        self.options
    }

    /// Providers refusing calls now, and why: a spent quota (until it
    /// resets), rejected credentials, or an open circuit breaker.
    pub fn unavailable(&self) -> Vec<(ProviderId, Error)> {
        self.health.unavailable((self.clock)())
    }

    /// Each provider's budget as its last response reported it: what's
    /// left without spending any. Only providers with budgets (OpenAlex)
    /// report one, and only once called.
    pub fn budgets(&self) -> Vec<(ProviderId, Budget)> {
        self.budgets.all()
    }

    /// Runs one provider operation with retries, unless the provider is
    /// refusing calls, and remembers how it went.
    async fn call<T, F, Fut>(&self, entry: &Entry, op: F) -> Result<Response<T>, Error>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<Response<T>, Error>>,
    {
        let provider = entry.provider.id();
        let pass = self.health.admit(&provider, (self.clock)())?;
        let policy = self.options.apply(&entry.policy);
        let result = with_retries(&policy, self.jitter, op).await;
        let now = (self.clock)();
        pass.record(&entry.policy, result.as_ref().err(), now);
        let usage = result.as_ref().ok().and_then(|r| r.usage.as_ref());
        self.budgets
            .note(&provider, usage, result.as_ref().err(), now);
        result
    }

    /// Registered providers, in priority order.
    pub fn providers(&self) -> impl Iterator<Item = &Arc<dyn Provider>> {
        self.entries.iter().map(|e| &e.provider)
    }
}

impl Candidate {
    /// Its best record with every identifier and notice the merged records
    /// gave.
    pub fn merged_record(&self) -> SourceRecord {
        let mut record = self.record.clone();
        record.ids = self.ids.clone();
        record.work.updates = self.updates.clone();
        record
    }
}

/// Locks `mutex`, poisoned or not: the maps behind the engine's locks stay
/// consistent if a holder panics, and a second panic (in a `Drop`) would abort.
pub(crate) fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Adds `usage` to a per-provider total.
pub fn add_usage(totals: &mut Vec<(ProviderId, Usage)>, provider: &ProviderId, usage: Usage) {
    match totals.iter_mut().find(|(p, _)| p == provider) {
        Some((_, total)) => *total = total.then(usage),
        None => totals.push((provider.clone(), usage)),
    }
}
