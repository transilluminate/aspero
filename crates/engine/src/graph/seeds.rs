//! The set's works: seeds resolved to OpenAlex works, cheaply and asked
//! once, and merged where they are one work.

use std::collections::BTreeSet;

use aspero_core::matching::{same_work, same_work_id};
use aspero_core::{FieldSet, IdKind, SourceRecord, WorkId};
use futures::StreamExt;

use super::{Incomplete, IncompleteKind, MAX_FILTER, Seed, SeedProblem, Spend, openalex_id};
use crate::{CallOptions, Engine, Entry, LookupResult, Outcome};

/// Lookups of seeds at once when estimating (each free, one id each).
const ESTIMATE_LOOKUPS: usize = 4;

/// Seeds counted as one work: merged seeds, their OpenAlex ids and their
/// references (the union of the known ones).
pub(super) struct Group {
    pub(super) members: Vec<usize>,
    pub(super) works: BTreeSet<WorkId>,
    pub(super) references: Option<BTreeSet<WorkId>>,
}

impl Group {
    pub(super) fn cites(&self, other: &Group) -> bool {
        self.references
            .as_ref()
            .is_some_and(|r| other.works.iter().any(|w| r.contains(w)))
    }

    pub(super) fn shares_references(&self, other: &Group) -> bool {
        match (&self.references, &other.references) {
            (Some(a), Some(b)) => !a.is_disjoint(b),
            _ => false,
        }
    }
}

impl Engine {
    /// Each seed's OpenAlex record, or why there's none; and whether
    /// resolving stopped early.
    pub(super) async fn resolve_seeds(
        &self,
        entry: &Entry,
        inputs: &[String],
        spend: &mut Spend,
    ) -> (Vec<Seed>, Option<Incomplete>) {
        // Many seeds are expected not to resolve: ask once, never retry.
        let quick = self.with_options(CallOptions {
            max_retries: Some(0),
            ..self.options
        });
        let accepts = &entry.provider.capabilities().lookup_kinds;
        let mut seeds: Vec<Option<Seed>> = vec![None; inputs.len()];
        // (seed, identifier OpenAlex looks up)
        let mut asks: Vec<(usize, WorkId)> = Vec::new();
        let mut elsewhere: Vec<(usize, WorkId)> = Vec::new();
        for (i, input) in inputs.iter().enumerate() {
            match input.parse::<WorkId>() {
                Err(e) => {
                    seeds[i] = Some(Seed::unresolved(
                        input,
                        SeedProblem::InvalidId,
                        e.to_string(),
                    ))
                }
                Ok(id) if accepts.contains(&id.kind()) => asks.push((i, id)),
                Ok(id) => elsewhere.push((i, id)),
            }
        }

        // Identifiers OpenAlex doesn't look up (PMCIDs, ISBNs): found
        // elsewhere (free), then asked about by the DOI or PMID found.
        if !elsewhere.is_empty() {
            let ids: Vec<WorkId> = elsewhere.iter().map(|(_, id)| id.clone()).collect();
            let report = quick.lookup(&ids, FieldSet::DEFAULT).await;
            spend.add(report.usage, 0);
            for ((i, id), result) in elsewhere.into_iter().zip(report.results) {
                let input = &inputs[i];
                let found = result.record.as_ref().and_then(|r| ask_by(r, accepts, &id));
                match (found, seed_failure(&result)) {
                    (Some(other), _) => asks.push((i, other.clone())),
                    (None, Some((problem, message))) => {
                        seeds[i] = Some(Seed::unresolved(input, problem, message))
                    }
                    (None, None) => {
                        seeds[i] = Some(Seed::unresolved(
                            input,
                            SeedProblem::UnsupportedId,
                            format!(
                                "OpenAlex doesn't look up {} identifiers, and the record found \
                                 has no DOI or PMID it does",
                                id.kind()
                            ),
                        ))
                    }
                }
            }
        }

        let mut stopped = None;
        let mut answers = self
            .ask_openalex(&quick, entry, &asks, spend, &mut stopped)
            .await;

        // Seeds OpenAlex has no record of by the identifier given are
        // looked up as `lookup` looks them up (Crossref, DataCite, doi.org,
        // …, through the cache; free, asked once), then asked about again
        // by an identifier the record found carries: an OpenAlex id, or
        // another DOI or PMID.
        let missed: Vec<usize> = (0..asks.len())
            .filter(|&k| matches!(answers[k], Some((Outcome::NotFound, None))))
            .collect();
        if !missed.is_empty() {
            let ids: Vec<WorkId> = missed.iter().map(|&k| asks[k].1.clone()).collect();
            let provider = entry.provider.id();
            let report = quick
                .lookup_skipping(&ids, FieldSet::DEFAULT, Some(&provider))
                .await;
            spend.add(report.usage, 0);
            let mut again: Vec<(usize, WorkId)> = Vec::new();
            for (&k, result) in missed.iter().zip(&report.results) {
                let Some(record) = &result.record else {
                    continue;
                };
                if let Some(other) = ask_by(record, accepts, &asks[k].1) {
                    again.push((k, other.clone()));
                }
            }
            if !again.is_empty() && stopped.is_none() {
                let found = self
                    .ask_openalex(&quick, entry, &again, spend, &mut stopped)
                    .await;
                for ((k, _), answer) in again.iter().zip(found) {
                    if let Some((outcome, Some(record))) = answer {
                        answers[*k] = Some((outcome, Some(record)));
                    }
                }
            }
        }

        for ((i, id), answer) in asks.iter().zip(answers) {
            let input = &inputs[*i];
            seeds[*i] = Some(match answer {
                Some((_, Some(record))) => Seed {
                    input: input.clone(),
                    record: Some(record),
                    problem: None,
                    message: None,
                    same_as: None,
                },
                Some((Outcome::Failed(e), None)) => Seed::unresolved(
                    input,
                    SeedProblem::ProviderFailed,
                    format!("the lookup failed: {e}"),
                ),
                Some(_) => Seed::unresolved(
                    input,
                    SeedProblem::NotFound,
                    format!("OpenAlex has no record of {id}"),
                ),
                None => Seed::unresolved(
                    input,
                    SeedProblem::OverBudget,
                    "not looked up: the budget ran out",
                ),
            });
        }
        let seeds = seeds
            .into_iter()
            .map(|s| s.expect("every seed resolved or settled"))
            .collect();
        (seeds, stopped)
    }

    /// OpenAlex's answer for each of `asks` (seed, identifier); `None`
    /// where the budget stopped it, saying so in `stopped`. Fresh cached
    /// answers first, then the rest in batches of one kind each (a batch of
    /// one is a free lookup), asked once. Estimating, the rest are asked
    /// one at a time (free), and the batches a run would pay for are
    /// counted when there's no cache for the run to find them in.
    async fn ask_openalex(
        &self,
        quick: &Engine,
        entry: &Entry,
        asks: &[(usize, WorkId)],
        spend: &mut Spend,
        stopped: &mut Option<Incomplete>,
    ) -> Vec<Option<(Outcome, Option<SourceRecord>)>> {
        let provider = entry.provider.id();
        let ids: Vec<WorkId> = asks.iter().map(|(_, id)| id.clone()).collect();
        let cached = self.cached_lookups(&provider, &ids, FieldSet::DEFAULT);
        let mut answers: Vec<Option<(Outcome, Option<SourceRecord>)>> = cached
            .into_iter()
            .map(|c| {
                c.map(|record| match record {
                    Some(r) => (Outcome::Found, Some(r)),
                    None => (Outcome::NotFound, None),
                })
            })
            .collect();
        let pending: Vec<usize> = (0..asks.len()).filter(|&k| answers[k].is_none()).collect();
        let pending_ids: Vec<WorkId> = pending.iter().map(|&k| asks[k].1.clone()).collect();
        let chunks = batches(&pending_ids, &pending, entry);
        if spend.estimating {
            // Without a cache, or past it, the run won't find them there.
            if self.cache.is_none() || !self.options.reads_cache() {
                spend.requests += chunks.iter().filter(|c| c.len() > 1).count() as u64;
            }
            let lookups = futures::stream::iter(pending_ids.into_iter().zip(pending).map(
                |(id, k)| async move {
                    let mut usage = Vec::new();
                    let answer = quick
                        .lookup_at(
                            entry,
                            std::slice::from_ref(&id),
                            FieldSet::DEFAULT,
                            &mut usage,
                        )
                        .await
                        .pop()
                        .expect("one answer per id");
                    (k, answer, usage)
                },
            ))
            .buffered(ESTIMATE_LOOKUPS)
            .collect::<Vec<_>>()
            .await;
            for (k, (attempt, record), usage) in lookups {
                spend.add(usage, 0);
                answers[k] = Some((attempt.outcome, record));
            }
            return answers;
        }
        let mut done = asks.len() - pending.len();
        for chunk in chunks {
            let paid = usize::from(chunk.len() > 1);
            if !spend.allows(paid) {
                *stopped = Some(Incomplete::new(IncompleteKind::Budget, done, asks.len()));
                break;
            }
            let batch: Vec<WorkId> = chunk.iter().map(|&k| asks[k].1.clone()).collect();
            let mut usage = Vec::new();
            let found = quick
                .lookup_at(entry, &batch, FieldSet::DEFAULT, &mut usage)
                .await;
            spend.add(usage, paid);
            for (&k, (attempt, record)) in chunk.iter().zip(found) {
                answers[k] = Some((attempt.outcome, record));
            }
            done += chunk.len();
        }
        answers
    }
}

/// The identifier to ask OpenAlex about `record` by: its OpenAlex id, else
/// one OpenAlex looks up (`accepts`) other than `asked`.
fn ask_by<'a>(record: &'a SourceRecord, accepts: &[IdKind], asked: &WorkId) -> Option<&'a WorkId> {
    let ids = &record.ids;
    ids.iter()
        .find(|x| x.kind() == IdKind::OpenAlex)
        .or_else(|| {
            ids.iter()
                .find(|x| accepts.contains(&x.kind()) && !same_work_id(x, asked))
        })
}

/// A lookup's failure as a seed's problem; `None` when it found a record.
fn seed_failure(result: &LookupResult) -> Option<(SeedProblem, String)> {
    if result.record.is_some() {
        return None;
    }
    if let Some(reason) = result.unsupported() {
        return Some((SeedProblem::UnsupportedId, reason));
    }
    if result.is_not_found() {
        return Some((SeedProblem::NotFound, "no such work found".into()));
    }
    let failed = result.attempts.iter().find_map(|a| match &a.outcome {
        Outcome::Failed(e) => Some(e),
        _ => None,
    });
    Some(match failed {
        Some(e) => (
            SeedProblem::ProviderFailed,
            format!("the lookup failed: {e}"),
        ),
        None => (SeedProblem::NotFound, "no such work found".into()),
    })
}

/// `pending` (indices into the asks, with `ids` their identifiers) in
/// batches as OpenAlex looks them up: one kind per batch (a filter per
/// kind; arXiv IDs are DOIs there), at most a batch's worth each.
fn batches(ids: &[WorkId], pending: &[usize], entry: &Entry) -> Vec<Vec<usize>> {
    let max = entry.provider.capabilities().max_batch.clamp(1, MAX_FILTER);
    let kind = |id: &WorkId| match id.kind() {
        IdKind::Arxiv => IdKind::Doi,
        k => k,
    };
    let mut by_kind: Vec<(IdKind, Vec<usize>)> = Vec::new();
    for (id, &k) in ids.iter().zip(pending) {
        match by_kind.iter_mut().find(|(kd, _)| *kd == kind(id)) {
            Some((_, group)) => group.push(k),
            None => by_kind.push((kind(id), vec![k])),
        }
    }
    by_kind
        .into_iter()
        .flat_map(|(_, group)| group.chunks(max).map(<[usize]>::to_vec).collect::<Vec<_>>())
        .collect()
}

/// Marks seeds that are one work (`same_as` the first of them) and groups
/// the resolved ones, in order of their first seed.
pub(super) fn merge_seeds(seeds: &mut [Seed]) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for i in 0..seeds.len() {
        let Some(record) = &seeds[i].record else {
            continue;
        };
        let same = groups.iter().position(|g| {
            g.members.iter().any(|&m| {
                let other = seeds[m]
                    .record
                    .as_ref()
                    .expect("grouped seeds are resolved");
                openalex_id(other) == openalex_id(record) || same_work(other, record)
            })
        });
        let work = openalex_id(record).cloned();
        let cited: Option<BTreeSet<WorkId>> = record
            .references
            .as_ref()
            .map(|r| r.iter().cloned().collect());
        match same {
            Some(g) => {
                let group = &mut groups[g];
                seeds[i].same_as = Some(group.members[0]);
                group.members.push(i);
                group.works.extend(work);
                if let Some(cited) = cited {
                    group.references.get_or_insert_default().extend(cited);
                }
            }
            None => groups.push(Group {
                members: vec![i],
                works: work.into_iter().collect(),
                references: cited,
            }),
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use aspero_core::ProviderId;

    use super::super::OPENALEX;
    use super::*;

    #[test]
    fn batches_keep_to_one_kind_and_a_hundred() {
        let mut ids: Vec<WorkId> = (1..=150)
            .map(|n| format!("10.1234/{n}").parse().unwrap())
            .collect();
        ids.push("pmid:1".parse().unwrap());
        ids.push("arXiv:1706.03762".parse().unwrap());
        let pending: Vec<usize> = (0..ids.len()).collect();
        let entry = crate::Entry {
            provider: std::sync::Arc::new(Stub(caps_with(100))),
            policy: aspero_core::ResiliencePolicy::default(),
        };
        let sizes: Vec<usize> = batches(&ids, &pending, &entry)
            .iter()
            .map(Vec::len)
            .collect();
        // DOIs and the arXiv ID (a DOI there) together, 100 at a time;
        // then the PMID.
        assert_eq!(sizes, [100, 51, 1]);
    }

    fn caps_with(max_batch: usize) -> aspero_core::Capabilities {
        let mut caps = aspero_core::Capabilities::default();
        caps.max_batch = max_batch;
        caps
    }

    struct Stub(aspero_core::Capabilities);

    #[async_trait::async_trait]
    impl aspero_core::Provider for Stub {
        fn id(&self) -> ProviderId {
            ProviderId::from_static(OPENALEX)
        }
        fn capabilities(&self) -> &aspero_core::Capabilities {
            &self.0
        }
    }
}
