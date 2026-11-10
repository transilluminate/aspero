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
//! Not yet: caching (store), fan-out search, the circuit breaker and the
//! cross-process limiter.

mod resolve;
pub mod retry;
mod standard;
pub mod transport;

pub use resolve::{Candidate, ResolveReport, Sighting, Verdict, Verification, Via};
/// Re-exported for credentials in provider configs.
pub use secrecy::SecretString;
#[cfg(feature = "crossref")]
pub use standard::CrossrefConfig;
#[cfg(feature = "openalex")]
pub use standard::OpenAlexConfig;
pub use standard::{StandardConfig, standard};

use std::sync::Arc;

use aspero_core::{
    Error, ErrorCategory, FieldSet, Filter, Page, Provider, ProviderId, ResiliencePolicy,
    SearchMode, SearchQuery, SourceRecord, Usage, WorkId,
};

use crate::retry::{Jitter, random_jitter, with_retries};

pub struct Engine {
    /// In priority order.
    entries: Vec<Entry>,
    jitter: Jitter,
}

struct Entry {
    provider: Arc<dyn Provider>,
    policy: ResiliencePolicy,
}

pub struct EngineBuilder {
    entries: Vec<(i32, Entry)>,
    jitter: Jitter,
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

    pub fn build(mut self) -> Engine {
        self.entries.sort_by_key(|(priority, _)| *priority);
        Engine {
            entries: self.entries.into_iter().map(|(_, e)| e).collect(),
            jitter: self.jitter,
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
            let provider = entry.provider.id();
            let caps = entry.provider.capabilities();
            let pending: Vec<usize> = (0..results.len())
                .filter(|&i| results[i].record.is_none())
                .filter(|&i| caps.lookup_kinds.contains(&results[i].id.kind()))
                .collect();

            let mut chunks = pending.chunks(caps.max_batch.max(1));
            while let Some(chunk) = chunks.next() {
                let batch: Vec<WorkId> = chunk.iter().map(|&i| results[i].id.clone()).collect();
                let answer = with_retries(&entry.policy, self.jitter, || {
                    entry.provider.lookup(&batch, fields)
                })
                .await;

                let attempt = |outcome| Attempt {
                    provider: provider.clone(),
                    outcome,
                };
                match answer {
                    Ok(resp) => {
                        if let Some(u) = resp.usage {
                            add_usage(&mut usage, &provider, u);
                        }
                        let mut lookups = resp.value.into_iter();
                        for &i in chunk {
                            // The contract is one answer per ID, in order;
                            // anything else is a provider bug, not a miss.
                            let outcome = match lookups.next() {
                                Some(l) if l.id == results[i].id => match l.record {
                                    Some(record) => {
                                        results[i].record = Some(record);
                                        Outcome::Found
                                    }
                                    None => Outcome::NotFound,
                                },
                                _ => Outcome::Failed(Error::decode(ContractViolation)),
                            };
                            results[i].attempts.push(attempt(outcome));
                        }
                    }
                    Err(error) => {
                        // Out of budget or credentials: don't spend more
                        // calls on this provider in this lookup.
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
                        for i in affected {
                            results[i]
                                .attempts
                                .push(attempt(Outcome::Failed(error.clone())));
                        }
                    }
                }
            }
        }
        LookupReport { results, usage }
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

        let resp = with_retries(&entry.policy, self.jitter, || entry.provider.search(&sent))
            .await
            .map_err(|error| EngineError::Provider {
                provider: provider.clone(),
                error,
            })?;
        Ok(SearchReport {
            provider,
            page: resp.value,
            ignored_filters,
            usage: resp.usage,
        })
    }
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
