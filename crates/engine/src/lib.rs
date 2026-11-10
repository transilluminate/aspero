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
//! Not yet: fan-out search, the circuit breaker and the cross-process
//! limiter.

mod abstracts;
mod check;
mod resolve;
pub mod retry;
mod standard;
pub mod transport;
mod verdict;
mod versions;

pub use abstracts::{Abstract, AbstractsReport};
pub use check::{Checked, Finding, Severity, duplicates, findings};
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
pub use verdict::{Verdict, Verification, describe_update};
pub use versions::VersionsReport;

use std::sync::Arc;
use std::time::Duration;

use aspero_core::{
    Error, ErrorCategory, Field, FieldSet, Filter, Page, Provider, ProviderId, ResiliencePolicy,
    SearchMode, SearchQuery, SourceRecord, Usage, WorkId,
};
pub use aspero_store::{Cache, SqliteCache, Stats as CacheStats, StoreError};
use jiff::Timestamp;

use crate::retry::{Jitter, random_jitter, with_retries};

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

pub struct Engine {
    /// In priority order.
    entries: Vec<Entry>,
    jitter: Jitter,
    cache: Option<Arc<dyn Cache>>,
    cache_policy: CachePolicy,
    clock: Clock,
}

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
        }
    }
}

/// The outcome of one provider's attempt at one ID.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Outcome {
    Found,
    NotFound,
    Failed(Error),
}

#[derive(Debug, Clone)]
pub struct Attempt {
    pub provider: ProviderId,
    pub outcome: Outcome,
    pub served: Served,
}

#[derive(Debug, Clone)]
pub struct LookupResult {
    /// As requested.
    pub id: WorkId,
    pub record: Option<SourceRecord>,
    /// Every provider tried, in order. Empty: no enabled provider accepts
    /// this kind of identifier.
    pub attempts: Vec<Attempt>,
}

impl LookupResult {
    /// No provider found it, and at least one could answer authoritatively
    /// (rather than every attempt failing).
    pub fn is_not_found(&self) -> bool {
        self.record.is_none()
            && self
                .attempts
                .iter()
                .any(|a| matches!(a.outcome, Outcome::NotFound))
    }
}

#[derive(Debug, Clone)]
pub struct LookupReport {
    /// One per requested ID, in request order.
    pub results: Vec<LookupResult>,
    /// Combined usage per provider that reported any.
    pub usage: Vec<(ProviderId, Usage)>,
}

#[derive(Debug, Clone)]
pub struct SearchReport {
    pub provider: ProviderId,
    pub page: Page,
    /// Query filters the provider can't apply; they were not sent.
    pub ignored_filters: Vec<Filter>,
    pub usage: Option<Usage>,
    pub served: Served,
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

    /// Registered providers, in priority order.
    pub fn providers(&self) -> impl Iterator<Item = &Arc<dyn Provider>> {
        self.entries.iter().map(|e| &e.provider)
    }

    /// Looks up every ID, falling through providers by priority. Never fails
    /// as a whole: per-ID outcomes are in the report.
    pub async fn lookup(&self, ids: &[WorkId], fields: FieldSet) -> LookupReport {
        let mut results: Vec<LookupResult> = ids
            .iter()
            .map(|id| LookupResult {
                id: id.clone(),
                record: None,
                attempts: Vec::new(),
            })
            .collect();
        let mut usage: Vec<(ProviderId, Usage)> = Vec::new();

        for entry in &self.entries {
            let caps = entry.provider.capabilities();
            let eligible: Vec<usize> = (0..results.len())
                .filter(|&i| results[i].record.is_none())
                .filter(|&i| caps.lookup_kinds.contains(&results[i].id.kind()))
                .collect();
            let ids: Vec<WorkId> = eligible.iter().map(|&i| results[i].id.clone()).collect();
            let answers = self.lookup_at(entry, &ids, fields, &mut usage).await;
            for (&i, (attempt, record)) in eligible.iter().zip(answers) {
                results[i].record = record;
                results[i].attempts.push(attempt);
            }
        }
        LookupReport { results, usage }
    }

    /// Adds editorial notices, in both directions, to records that another
    /// provider supplied, from the providers that report them in detail
    /// (Crossref): an original then lists its corrections and retractions,
    /// a correction the work it corrects. Batched and cached, like lookups.
    pub async fn add_notices(&self, report: &mut LookupReport) {
        for entry in &self.entries {
            if !entry.provider.capabilities().notices {
                continue;
            }
            let works = found_records(&report.results, |_| true);
            for (i, found) in self.ask_about(entry, works, &mut report.usage).await {
                let Some(record) = report.results[i].record.as_mut() else {
                    continue;
                };
                let work = &mut record.work;
                resolve::merge_updates(&mut work.updates, &found.work.updates);
                for target in found.work.update_to {
                    if !work.update_to.contains(&target) {
                        work.update_to.push(target);
                    }
                }
            }
        }
    }

    /// Fills in where works can be read free, for records whose provider
    /// didn't say, from the first provider (by priority) that does.
    pub async fn add_open_access(&self, report: &mut LookupReport) {
        for entry in &self.entries {
            if !entry
                .provider
                .capabilities()
                .fields
                .contains(Field::OpenAccess)
            {
                continue;
            }
            let works = found_records(&report.results, |r| r.work.open_access.is_none());
            for (i, found) in self.ask_about(entry, works, &mut report.usage).await {
                if let (Some(oa), Some(record)) =
                    (found.work.open_access, report.results[i].record.as_mut())
                {
                    record.work.open_access = Some(oa);
                }
            }
        }
    }

    /// One provider's records of works it didn't supply itself, each asked
    /// for by an identifier it accepts: `(work index, record)` per work it
    /// knows.
    async fn ask_about<'a>(
        &self,
        entry: &Entry,
        works: impl IntoIterator<Item = (usize, &'a SourceRecord)>,
        usage: &mut Vec<(ProviderId, Usage)>,
    ) -> Vec<(usize, SourceRecord)> {
        let provider = entry.provider.id();
        let kinds = &entry.provider.capabilities().lookup_kinds;
        let wanted: Vec<(usize, WorkId)> = works
            .into_iter()
            .filter(|(_, r)| r.source != provider)
            .filter_map(|(i, r)| {
                let id = r.ids.iter().find(|id| kinds.contains(&id.kind()))?;
                Some((i, id.clone()))
            })
            .collect();
        if wanted.is_empty() {
            return Vec::new();
        }
        let ids: Vec<WorkId> = wanted.iter().map(|(_, id)| id.clone()).collect();
        let answers = self.lookup_at(entry, &ids, FieldSet::ALL, usage).await;
        wanted
            .into_iter()
            .zip(answers)
            .filter_map(|((i, _), (_, record))| Some((i, record?)))
            .collect()
    }

    /// Asks one provider about `ids` (all of kinds it accepts): fresh cached
    /// answers first, the rest in batches with retries, and stale cached
    /// records for any that fail. One answer per ID, in order.
    async fn lookup_at(
        &self,
        entry: &Entry,
        ids: &[WorkId],
        fields: FieldSet,
        usage: &mut Vec<(ProviderId, Usage)>,
    ) -> Vec<(Attempt, Option<SourceRecord>)> {
        let provider = entry.provider.id();
        let caps = entry.provider.capabilities();
        let attempt = |outcome, served| Attempt {
            provider: provider.clone(),
            outcome,
            served,
        };
        let mut answers: Vec<Option<(Attempt, Option<SourceRecord>)>> = vec![None; ids.len()];

        // Fresh cached answers first (one cache read for the whole set).
        let mut pending = Vec::new();
        let mut from_cache = 0;
        for (i, cached) in self
            .cached_lookups(&provider, ids, fields)
            .into_iter()
            .enumerate()
        {
            answers[i] = match cached {
                Some(Some(record)) => {
                    from_cache += 1;
                    Some((attempt(Outcome::Found, Served::Cache), Some(record)))
                }
                Some(None) => {
                    from_cache += 1;
                    Some((attempt(Outcome::NotFound, Served::Cache), None))
                }
                None => {
                    pending.push(i);
                    None
                }
            };
        }

        if from_cache > 0 {
            tracing::info!(target: "aspero::cache", provider = %provider, ids = from_cache, "lookups served from cache");
        }
        let mut chunks = pending.chunks(caps.max_batch.max(1));
        while let Some(chunk) = chunks.next() {
            let batch: Vec<WorkId> = chunk.iter().map(|&i| ids[i].clone()).collect();
            let answer = with_retries(&entry.policy, self.jitter, || {
                entry.provider.lookup(&batch, fields)
            })
            .await;

            let error = match answer {
                Ok(resp) => {
                    if let Some(u) = resp.usage {
                        add_usage(usage, &provider, u);
                    }
                    // The contract is one answer per ID, in order; anything
                    // else is a provider bug, not a miss.
                    let in_order = resp.value.len() == batch.len()
                        && resp.value.iter().zip(&batch).all(|(l, id)| &l.id == id);
                    if in_order {
                        let stored: Vec<_> = resp
                            .value
                            .iter()
                            .map(|l| (&l.id, l.record.as_ref()))
                            .collect();
                        self.remember_lookups(&provider, &stored);
                        for (&i, lookup) in chunk.iter().zip(resp.value) {
                            let outcome = match lookup.record {
                                Some(_) => Outcome::Found,
                                None => Outcome::NotFound,
                            };
                            answers[i] = Some((attempt(outcome, Served::Live), lookup.record));
                        }
                        continue;
                    }
                    Error::decode(ContractViolation)
                }
                Err(error) => error,
            };

            // Out of budget or credentials: don't spend more calls on this
            // provider in this lookup.
            let give_up = matches!(
                error.category(),
                ErrorCategory::Throttled | ErrorCategory::Auth
            );
            let affected: Vec<usize> = if give_up {
                chunk
                    .iter()
                    .chain(chunks.by_ref().flatten())
                    .copied()
                    .collect()
            } else {
                chunk.to_vec()
            };
            // Stale-on-error: an old answer beats none.
            let affected_ids: Vec<WorkId> = affected.iter().map(|&i| ids[i].clone()).collect();
            for (&i, stale) in affected
                .iter()
                .zip(self.stale_lookups(&provider, &affected_ids))
            {
                answers[i] = Some(match stale {
                    Some(record) => (attempt(Outcome::Found, Served::Stale), Some(record)),
                    None => (attempt(Outcome::Failed(error.clone()), Served::Live), None),
                });
            }
        }
        answers
            .into_iter()
            .map(|a| a.expect("every ID answered from cache, provider or failure"))
            .collect()
    }

    /// Searches one provider: `source` if given, otherwise the first that
    /// supports the query's mode.
    pub async fn search(
        &self,
        source: Option<&str>,
        query: &SearchQuery,
    ) -> Result<SearchReport, EngineError> {
        let citation = query.mode == SearchMode::Citation;
        let entry = match source {
            Some(name) => self
                .entries
                .iter()
                .find(|e| e.provider.id().as_str() == name)
                .ok_or_else(|| EngineError::UnknownProvider(name.to_owned()))?,
            None => self
                .entries
                .iter()
                .find(|e| {
                    let caps = e.provider.capabilities();
                    caps.search && (!citation || caps.citation_search)
                })
                .ok_or(EngineError::NoCapableProvider(if citation {
                    "citation search"
                } else {
                    "search"
                }))?,
        };
        let provider = entry.provider.id();
        let caps = entry.provider.capabilities();
        let refuse = |what| EngineError::Provider {
            provider: provider.clone(),
            error: Error::Unsupported(what),
        };
        if !caps.search {
            return Err(refuse("search"));
        }
        if citation && !caps.citation_search {
            return Err(refuse("citation search"));
        }

        let ignored_filters: Vec<Filter> = query
            .filters()
            .into_iter()
            .filter(|f| !caps.search_filters.contains(f))
            .collect();
        let mut sent = query.clone();
        for filter in &ignored_filters {
            match filter {
                Filter::Years => sent.years = None,
                Filter::WorkType => sent.work_type = None,
                Filter::OpenAccess => sent.open_access = None,
                _ => {}
            }
        }

        let report = |page, usage, served| SearchReport {
            provider: provider.clone(),
            page,
            ignored_filters: ignored_filters.clone(),
            usage,
            served,
        };
        let cached = self.cached_search(&provider, &sent);
        if let Some((page, true)) = cached {
            tracing::info!(target: "aspero::cache", provider = %provider, text = %sent.text, "search served from cache");
            return Ok(report(page, None, Served::Cache));
        }
        match with_retries(&entry.policy, self.jitter, || entry.provider.search(&sent)).await {
            Ok(resp) => {
                self.remember_search(&provider, &sent, &resp.value);
                Ok(report(resp.value, resp.usage, Served::Live))
            }
            Err(error) => match cached {
                Some((page, _)) => Ok(report(page, None, Served::Stale)),
                None => Err(EngineError::Provider {
                    provider: provider.clone(),
                    error,
                }),
            },
        }
    }

    // Cache access. Cache failures are never fatal: a broken cache behaves
    // like an empty one, and failed writes are skipped.

    /// For each ID, a fresh cached answer covering `fields`:
    /// `Some(Some(record))`, `Some(None)` for a fresh "not found", or `None`
    /// to ask the provider.
    fn cached_lookups(
        &self,
        provider: &ProviderId,
        ids: &[WorkId],
        fields: FieldSet,
    ) -> Vec<Option<Option<SourceRecord>>> {
        let now = (self.clock)();
        let policy = self.cache_policy;
        self.cache_entries(provider, ids)
            .into_iter()
            .map(|entry| {
                let entry = entry?;
                match entry.value {
                    Some(ref record)
                        if entry.is_fresh(now, policy.records)
                            && fields.is_subset(record.fields) =>
                    {
                        Some(entry.value)
                    }
                    None if entry.is_fresh(now, policy.misses) => Some(None),
                    _ => None,
                }
            })
            .collect()
    }

    /// For each ID, any cached record, however old.
    fn stale_lookups(&self, provider: &ProviderId, ids: &[WorkId]) -> Vec<Option<SourceRecord>> {
        self.cache_entries(provider, ids)
            .into_iter()
            .map(|entry| entry.and_then(|e| e.value))
            .collect()
    }

    fn cache_entries(
        &self,
        provider: &ProviderId,
        ids: &[WorkId],
    ) -> Vec<Option<aspero_store::CachedLookup>> {
        self.cache
            .as_ref()
            .and_then(|c| c.get_lookups(provider, ids).ok())
            .unwrap_or_else(|| vec![None; ids.len()])
    }

    fn remember_lookups(
        &self,
        provider: &ProviderId,
        answers: &[(&WorkId, Option<&SourceRecord>)],
    ) {
        if let Some(cache) = &self.cache {
            let _ = cache.put_lookups(provider, answers, (self.clock)());
        }
    }

    /// A cached page and whether it is still fresh.
    fn cached_search(&self, provider: &ProviderId, query: &SearchQuery) -> Option<(Page, bool)> {
        let entry = self.cache.as_ref()?.get_search(provider, query).ok()??;
        let fresh = entry.is_fresh((self.clock)(), self.cache_policy.searches);
        Some((entry.value, fresh))
    }

    fn remember_search(&self, provider: &ProviderId, query: &SearchQuery, page: &Page) {
        if let Some(cache) = &self.cache {
            let _ = cache.put_search(provider, query, page, (self.clock)());
        }
    }
}

/// The records found among `results`, by index, that `keep` selects.
fn found_records(
    results: &[LookupResult],
    keep: impl Fn(&SourceRecord) -> bool,
) -> impl Iterator<Item = (usize, &SourceRecord)> {
    results
        .iter()
        .enumerate()
        .filter_map(move |(i, r)| Some((i, r.record.as_ref().filter(|r| keep(r))?)))
}

fn add_usage(totals: &mut Vec<(ProviderId, Usage)>, provider: &ProviderId, usage: Usage) {
    match totals.iter_mut().find(|(p, _)| p == provider) {
        Some((_, total)) => *total = total.then(usage),
        None => totals.push((provider.clone(), usage)),
    }
}

#[derive(Debug, thiserror::Error)]
#[error("provider answered a batch lookup out of order or incompletely")]
struct ContractViolation;
