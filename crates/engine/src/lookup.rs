//! Lookups by identifier: routing across providers, the cache, and
//! sharing calls in flight.

use aspero_core::matching::same_work_id;
use aspero_core::{
    Error, ErrorCategory, FieldSet, IdKind, Lookup, ProviderId, SourceRecord, Usage, WorkId,
};

use crate::inflight::Answer;
use crate::observe::{self, FromCache, op};
use crate::{Engine, Entry, Served, add_usage, transport};

/// The outcome of one provider's attempt at one ID.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Outcome {
    Found,
    NotFound,
    Failed(Error),
}

/// A cached answer to a lookup, as one call may use it.
#[derive(Debug, Clone)]
// As big as the `Option<Option<SourceRecord>>` it stands for, and as
// short-lived: a call's answers, moved out as they're read.
#[allow(clippy::large_enum_variant)]
pub(crate) enum Cached {
    Found(SourceRecord),
    NotFound,
    /// Nothing usable cached: ask the provider (or, falling back, fail).
    Uncached,
}

impl Cached {
    /// The outcome and record to answer with; `None` when uncached.
    pub(crate) fn answer(self) -> Option<(Outcome, Option<SourceRecord>)> {
        match self {
            Cached::Found(record) => Some((Outcome::Found, Some(record))),
            Cached::NotFound => Some((Outcome::NotFound, None)),
            Cached::Uncached => None,
        }
    }
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
        let input = || {
            ids.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        };
        let work = Box::pin(self.lookup_skipping(ids, fields, None));
        self.observed(op::LOOKUP, ids.len(), input, work, observe::never_fails)
            .await
    }

    /// As [`lookup`](Self::lookup), without asking `skip` (a provider
    /// whose answer is already known).
    pub(crate) async fn lookup_skipping(
        &self,
        ids: &[WorkId],
        fields: FieldSet,
        skip: Option<&ProviderId>,
    ) -> LookupReport {
        let mut report = self.chain(ids, fields, skip).await;
        self.follow_piis(&mut report, fields, skip).await;
        report
    }

    /// A PII found (by Crossref, the one source that can) is looked up
    /// again as the DOI its record gives, through the whole chain: the
    /// answer is the record a lookup of that DOI gives (OpenAlex's,
    /// usually), carrying the PII too, so verdicts and merging are a
    /// DOI's. The DOI's attempts follow the PII's. Where the DOI isn't
    /// found (all failing), the PII's record stands.
    async fn follow_piis(
        &self,
        report: &mut LookupReport,
        fields: FieldSet,
        skip: Option<&ProviderId>,
    ) {
        let wanted: Vec<(usize, WorkId)> = report
            .results
            .iter()
            .enumerate()
            .filter(|(_, r)| r.id.kind() == IdKind::Pii)
            .filter_map(|(i, r)| {
                let ids = &r.record.as_ref()?.ids;
                Some((i, ids.iter().find(|id| id.kind() == IdKind::Doi)?.clone()))
            })
            .collect();
        if wanted.is_empty() {
            return;
        }
        let dois: Vec<WorkId> = wanted.iter().map(|(_, doi)| doi.clone()).collect();
        let by_doi = self.chain(&dois, fields, skip).await;
        for (provider, usage) in by_doi.usage {
            add_usage(&mut report.usage, &provider, usage);
        }
        for ((i, _), found) in wanted.into_iter().zip(by_doi.results) {
            let result = &mut report.results[i];
            result.attempts.extend(found.attempts);
            if let Some(mut record) = found.record {
                if !record.ids.contains(&result.id) {
                    record.ids.push(result.id.clone());
                }
                result.record = Some(record);
            }
        }
    }

    /// Asks each provider in turn about the IDs still missing.
    async fn chain(
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
            for (&i, (attempt, mut record)) in eligible.iter().zip(answers) {
                settled[i] = matches!(attempt.outcome, Outcome::NotFound)
                    && caps.final_misses.contains(&results[i].id.kind());
                // A record found for an identifier it doesn't list (OpenAlex
                // answers a Zenodo version's DOI with the concept's record)
                // is that identifier's: it carries it.
                if let Some(r) = &mut record
                    && !r.ids.iter().any(|id| same_work_id(id, &results[i].id))
                {
                    r.ids.push(results[i].id.clone());
                }
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
            answers[i] = if let Some((outcome, record)) = cached.answer() {
                from_cache += 1;
                Some((attempt(outcome, Served::Cache), record))
            } else {
                pending.push(i);
                None
            };
        }

        observe::served(&provider, op::LOOKUP, FromCache::Hit, from_cache);
        // Ask about the rest, unless another caller is already asking.
        let wanted: Vec<(usize, WorkId)> = pending.iter().map(|&i| (i, ids[i].clone())).collect();
        let (mut leads, joined) = self.inflight.claim(&provider, &wanted, fields);
        let mut led = leads.indices();
        let free = caps.max_free_batch;
        let mut size = caps.max_batch.max(1);
        if self.taped {
            // Under a tape, one ID per request, so each ID's answer replays
            // however later batches are made up; batches recorded whole
            // replay as before.
            let replayed;
            (led, replayed) = self
                .replay_batches(entry, ids, &led, size, fields, usage)
                .await;
            for (i, answer) in replayed {
                answers[i] = Some(answer);
            }
            size = 1;
        } else if free > 0 && led.len() > free && self.check_cap(entry).await.is_err() {
            // Batches cost where single lookups don't (OpenAlex): past a
            // daily cap, IDs go as many at a time as is free.
            size = size.min(free);
        }
        let mut chunks = led.chunks(size);
        while let Some(chunk) = chunks.next() {
            leads.publish(&answers);
            let batch: Vec<WorkId> = chunk.iter().map(|&i| ids[i].clone()).collect();
            let error = match self.ask_batch(entry, batch, fields).await {
                Ok((spent, answered)) => {
                    if let Some(u) = spent {
                        add_usage(usage, &provider, u);
                    }
                    if let Some(answered) = answered {
                        for (&i, answer) in chunk.iter().zip(answered) {
                            answers[i] = Some(answer);
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
            let mut stale_served = 0;
            for (&i, stale) in affected
                .iter()
                .zip(self.fallback_lookups(&provider, &affected_ids))
            {
                answers[i] = Some(match stale.answer() {
                    Some((outcome, record)) => {
                        stale_served += 1;
                        (attempt(outcome, Served::Stale), record)
                    }
                    None => (attempt(Outcome::Failed(error.clone()), Served::Live), None),
                });
            }
            let cached = FromCache::Stale(error.kind().as_str());
            observe::served(&provider, op::LOOKUP, cached, stale_served);
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

    /// Asks `entry` about `batch` (paid past its free batch size), and
    /// remembers the answers: the call's usage, and its answers (`None`
    /// when they break the contract). Finished and cached, once sent, even
    /// if the caller stops waiting ([`finish_paid`](Self::finish_paid)).
    async fn ask_batch(
        &self,
        entry: &Entry,
        batch: Vec<WorkId>,
        fields: FieldSet,
    ) -> Result<(Option<Usage>, Option<Vec<Answer>>), Error> {
        let free = entry.provider.capabilities().max_free_batch;
        let (engine, owned) = (self.clone(), entry.clone());
        let ask = async move {
            let entry = &owned;
            let ask = || entry.provider.lookup(&batch, fields);
            let answer = if batch.len() > free {
                engine.call_paid(entry, op::LOOKUP, ask).await
            } else {
                engine.call(entry, op::LOOKUP, ask).await
            };
            answer.map(|resp| {
                let answered = engine.answered(&entry.provider.id(), &batch, resp.value);
                (resp.usage, answered)
            })
        };
        self.finish_paid(entry, ask, |answer| answer.as_ref().ok()?.0)
            .await
    }

    /// A provider's answers to `batch`, live, remembered in the cache; `None`
    /// when they break the contract (one answer per ID, in order), which is
    /// a provider bug, not a miss.
    fn answered(
        &self,
        provider: &ProviderId,
        batch: &[WorkId],
        lookups: Vec<Lookup>,
    ) -> Option<Vec<Answer>> {
        let in_order =
            lookups.len() == batch.len() && lookups.iter().zip(batch).all(|(l, id)| &l.id == id);
        if !in_order {
            return None;
        }
        let stored: Vec<_> = lookups.iter().map(|l| (&l.id, l.record.as_ref())).collect();
        self.remember_lookups(provider, &stored);
        let answers = lookups
            .into_iter()
            .map(|lookup| {
                let outcome = match lookup.record {
                    Some(_) => Outcome::Found,
                    None => Outcome::NotFound,
                };
                let attempt = Attempt {
                    provider: provider.clone(),
                    outcome,
                    served: Served::Live,
                };
                (attempt, lookup.record)
            })
            .collect();
        Some(answers)
    }

    /// Under a tape, the batches of `led` (indices into `ids`, `size` at a
    /// time, as they'd be asked untaped) that the tape holds whole, asked
    /// replay-only: a tape recorded before lookups went one ID at a time
    /// replays its batches while they're made up the same. The IDs still
    /// to ask, and the answers replayed.
    async fn replay_batches(
        &self,
        entry: &Entry,
        ids: &[WorkId],
        led: &[usize],
        size: usize,
        fields: FieldSet,
        usage: &mut Vec<(ProviderId, Usage)>,
    ) -> (Vec<usize>, Vec<(usize, Answer)>) {
        let provider = entry.provider.id();
        let (mut rest, mut replayed) = (Vec::new(), Vec::new());
        for chunk in led.chunks(size) {
            // One ID is asked alone anyway.
            if chunk.len() < 2 {
                rest.extend_from_slice(chunk);
                continue;
            }
            let batch: Vec<WorkId> = chunk.iter().map(|&i| ids[i].clone()).collect();
            let ask = entry.provider.lookup(&batch, fields);
            let answered = match transport::replaying_only(ask).await {
                Ok(resp) => {
                    if let Some(u) = resp.usage {
                        add_usage(usage, &provider, u);
                    }
                    self.answered(&provider, &batch, resp.value)
                }
                Err(_) => None,
            };
            match answered {
                Some(answers) => replayed.extend(chunk.iter().copied().zip(answers)),
                None => rest.extend_from_slice(chunk),
            }
        }
        (rest, replayed)
    }

    // Cache access. Cache failures are never fatal: a broken cache behaves
    // like an empty one, and failed writes are skipped.

    /// For each ID, a fresh cached answer covering `fields`: a record, a
    /// fresh "not found", or uncached to ask the provider (always, past
    /// the cache).
    pub(crate) fn cached_lookups(
        &self,
        provider: &ProviderId,
        ids: &[WorkId],
        fields: FieldSet,
    ) -> Vec<Cached> {
        let policy = self.cache_policy;
        self.cache_entries(provider, ids)
            .into_iter()
            .map(|entry| {
                let Some(entry) = entry else {
                    return Cached::Uncached;
                };
                let fresh = match &entry.value {
                    Some(record) => {
                        self.current(&entry, policy.records) && fields.is_subset(record.fields)
                    }
                    None => self.current(&entry, policy.misses),
                };
                match (fresh, entry.value) {
                    (true, Some(record)) => Cached::Found(record),
                    (true, None) => Cached::NotFound,
                    (false, _) => Cached::Uncached,
                }
            })
            .collect()
    }

    /// For each ID, the cached answer to serve as stale when the provider
    /// fails: any record, however old, or a "not found" within its
    /// lifetime (which only a call past the cache gets this far with;
    /// older ones aren't served). Uncached where there's neither.
    fn fallback_lookups(&self, provider: &ProviderId, ids: &[WorkId]) -> Vec<Cached> {
        let now = (self.clock)();
        self.cache_entries(provider, ids)
            .into_iter()
            .map(|entry| match entry {
                Some(e) => match e.value {
                    Some(record) => Cached::Found(record),
                    None if e.is_fresh(now, self.cache_policy.misses) => Cached::NotFound,
                    None => Cached::Uncached,
                },
                None => Cached::Uncached,
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
        let answers: Vec<Answer> = fresh
            .into_iter()
            .zip(any)
            .map(|(fresh, any)| match (fresh.answer(), any) {
                (Some((outcome, record)), _) => (attempt(outcome, Served::Cache), record),
                (None, Some(record)) => (attempt(Outcome::Found, Served::Stale), Some(record)),
                (None, None) => (attempt(Outcome::Failed(Error::Offline), Served::Live), None),
            })
            .collect();
        let count = |served| answers.iter().filter(|(a, _)| a.served == served).count();
        observe::served(provider, op::LOOKUP, FromCache::Hit, count(Served::Cache));
        let stale = count(Served::Stale);
        observe::served(provider, op::LOOKUP, FromCache::Stale("offline"), stale);
        answers
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
