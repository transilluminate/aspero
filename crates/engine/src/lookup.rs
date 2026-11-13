//! Lookups by identifier: routing across providers, the cache, and
//! sharing calls in flight.

use aspero_core::{Error, ErrorCategory, FieldSet, ProviderId, SourceRecord, Usage, WorkId};

use crate::inflight::Answer;
use crate::{Engine, Entry, Served, add_usage};

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

    /// Where the record came from: the last attempt that found it.
    pub fn served(&self) -> Option<Served> {
        self.attempts
            .iter()
            .rfind(|a| matches!(a.outcome, Outcome::Found))
            .map(|a| a.served)
    }

    /// Why nothing was asked, when no enabled provider accepts this kind
    /// of identifier.
    pub fn unsupported(&self) -> Option<String> {
        (self.record.is_none() && self.attempts.is_empty()).then(|| {
            format!(
                "no enabled provider looks up {} identifiers",
                self.id.kind()
            )
        })
    }
}

#[derive(Debug, Clone)]
pub struct LookupReport {
    /// One per requested ID, in request order.
    pub results: Vec<LookupResult>,
    /// Combined usage per provider that reported any.
    pub usage: Vec<(ProviderId, Usage)>,
}

impl Engine {
    /// Looks up every ID, falling through providers by priority. Never fails
    /// as a whole: per-ID outcomes are in the report.
    pub async fn lookup(&self, ids: &[WorkId], fields: FieldSet) -> LookupReport {
        self.lookup_skipping(ids, fields, None).await
    }

    /// As [`lookup`](Self::lookup), without asking `skip` (a provider
    /// whose answer is already known).
    pub(crate) async fn lookup_skipping(
        &self,
        ids: &[WorkId],
        fields: FieldSet,
        skip: Option<&ProviderId>,
    ) -> LookupReport {
        let mut results: Vec<LookupResult> = ids
            .iter()
            .map(|id| LookupResult {
                id: id.clone(),
                record: None,
                attempts: Vec::new(),
            })
            .collect();
        let mut usage: Vec<(ProviderId, Usage)> = Vec::new();

        // Not found where that is final (doi.org for DOIs): ask no further.
        let mut settled = vec![false; results.len()];
        for entry in &self.entries {
            if skip.is_some_and(|p| *p == entry.provider.id()) {
                continue;
            }
            let caps = entry.provider.capabilities();
            let eligible: Vec<usize> = (0..results.len())
                .filter(|&i| results[i].record.is_none() && !settled[i])
                .filter(|&i| caps.lookup_kinds.contains(&results[i].id.kind()))
                .collect();
            let ids: Vec<WorkId> = eligible.iter().map(|&i| results[i].id.clone()).collect();
            let answers = self.lookup_at(entry, &ids, fields, &mut usage).await;
            for (&i, (attempt, record)) in eligible.iter().zip(answers) {
                settled[i] = matches!(attempt.outcome, Outcome::NotFound)
                    && caps.final_misses.contains(&results[i].id.kind());
                results[i].record = record;
                results[i].attempts.push(attempt);
            }
        }
        LookupReport { results, usage }
    }

    /// Asks one provider about `ids` (all of kinds it accepts): fresh cached
    /// answers first (none, past the cache), the rest in batches with
    /// retries, and stale cached answers for any that fail. One answer per
    /// ID, in order.
    pub(crate) async fn lookup_at(
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
        if self.options.offline {
            return self.offline_lookups(&provider, ids, fields);
        }
        let mut answers: Vec<Option<Answer>> = vec![None; ids.len()];

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
        // Ask about the rest, unless another caller is already asking.
        let wanted: Vec<(usize, WorkId)> = pending.iter().map(|&i| (i, ids[i].clone())).collect();
        let (mut leads, joined) = self.inflight.claim(&provider, &wanted, fields);
        let led = leads.indices();
        let mut chunks = led.chunks(caps.max_batch.max(1));
        while let Some(chunk) = chunks.next() {
            leads.publish(&answers);
            let batch: Vec<WorkId> = chunk.iter().map(|&i| ids[i].clone()).collect();
            let answer = self
                .call(entry, || entry.provider.lookup(&batch, fields))
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
                .zip(self.fallback_lookups(&provider, &affected_ids))
            {
                answers[i] = Some(match stale {
                    Some(Some(record)) => (attempt(Outcome::Found, Served::Stale), Some(record)),
                    Some(None) => (attempt(Outcome::NotFound, Served::Stale), None),
                    None => (attempt(Outcome::Failed(error.clone()), Served::Live), None),
                });
            }
        }
        leads.publish(&answers);
        drop(leads);

        // Answers others fetched; ask again for any they gave up on.
        let mut abandoned = Vec::new();
        for (i, call) in joined {
            match call.answer().await {
                Some(answer) => answers[i] = Some(answer),
                None => abandoned.push(i),
            }
        }
        if !abandoned.is_empty() {
            let retry: Vec<WorkId> = abandoned.iter().map(|&i| ids[i].clone()).collect();
            let again = Box::pin(self.lookup_at(entry, &retry, fields, usage)).await;
            for (i, answer) in abandoned.into_iter().zip(again) {
                answers[i] = Some(answer);
            }
        }
        answers
            .into_iter()
            .map(|a| a.expect("every ID answered from cache, provider or failure"))
            .collect()
    }

    // Cache access. Cache failures are never fatal: a broken cache behaves
    // like an empty one, and failed writes are skipped.

    /// For each ID, a fresh cached answer covering `fields`:
    /// `Some(Some(record))`, `Some(None)` for a fresh "not found", or `None`
    /// to ask the provider (always, past the cache).
    pub(crate) fn cached_lookups(
        &self,
        provider: &ProviderId,
        ids: &[WorkId],
        fields: FieldSet,
    ) -> Vec<Option<Option<SourceRecord>>> {
        let policy = self.cache_policy;
        self.cache_entries(provider, ids)
            .into_iter()
            .map(|entry| {
                let entry = entry?;
                match entry.value {
                    Some(ref record)
                        if self.current(&entry, policy.records)
                            && fields.is_subset(record.fields) =>
                    {
                        Some(entry.value)
                    }
                    None if self.current(&entry, policy.misses) => Some(None),
                    _ => None,
                }
            })
            .collect()
    }

    /// For each ID, the cached answer to serve as stale when the provider
    /// fails: any record, however old, or a "not found" within its
    /// lifetime (which only a call past the cache gets this far with;
    /// older ones aren't served). `None` where there's neither.
    fn fallback_lookups(
        &self,
        provider: &ProviderId,
        ids: &[WorkId],
    ) -> Vec<Option<Option<SourceRecord>>> {
        let now = (self.clock)();
        self.cache_entries(provider, ids)
            .into_iter()
            .map(|entry| {
                let entry = entry?;
                match entry.value {
                    Some(record) => Some(Some(record)),
                    None => entry
                        .is_fresh(now, self.cache_policy.misses)
                        .then_some(None),
                }
            })
            .collect()
    }

    /// Every answer from the cache, however old: offline.
    fn offline_lookups(
        &self,
        provider: &ProviderId,
        ids: &[WorkId],
        fields: FieldSet,
    ) -> Vec<Answer> {
        let fresh = self.cached_lookups(provider, ids, fields);
        let any = self.stale_lookups(provider, ids);
        let attempt = |outcome, served| Attempt {
            provider: provider.clone(),
            outcome,
            served,
        };
        fresh
            .into_iter()
            .zip(any)
            .map(|answer| match answer {
                (Some(Some(record)), _) => (attempt(Outcome::Found, Served::Cache), Some(record)),
                (Some(None), _) => (attempt(Outcome::NotFound, Served::Cache), None),
                (None, Some(record)) => (attempt(Outcome::Found, Served::Stale), Some(record)),
                (None, None) => (attempt(Outcome::Failed(Error::Offline), Served::Live), None),
            })
            .collect()
    }

    /// For each ID, any cached record, however old.
    pub(crate) fn stale_lookups(
        &self,
        provider: &ProviderId,
        ids: &[WorkId],
    ) -> Vec<Option<SourceRecord>> {
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
}

#[derive(Debug, thiserror::Error)]
#[error("provider answered a batch lookup out of order or incompletely")]
struct ContractViolation;
