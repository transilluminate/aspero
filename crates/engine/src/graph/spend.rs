//! What an analysis spends, the works it looks up (candidates: probably
//! missing, related), and what counts as one of the set.

use aspero_core::matching::{same_work, same_work_id};
use aspero_core::{FieldSet, ProviderId, SourceRecord, Usage, WorkId};

use super::{Incomplete, IncompleteKind, LIST_PRICE, MAX_FILTER, Seed};
use crate::{Engine, Entry, Outcome, add_usage};

/// What an analysis has spent and may spend.
pub(super) struct Spend {
    pub(super) budget: Option<f64>,
    pub(super) spent: f64,
    /// Paid requests counted, when estimating.
    pub(super) requests: u64,
    pub(super) estimating: bool,
    /// Some of `requests` are a ceiling, not an exact count.
    pub(super) ceiling: bool,
    pub(super) usage: Vec<(ProviderId, Usage)>,
}

impl Spend {
    /// Whether `calls` more paid calls fit the budget.
    pub(super) fn allows(&self, calls: usize) -> bool {
        // A hair over, so rounding doesn't refuse the last call that fits.
        self.budget
            .is_none_or(|b| self.spent + calls as f64 * LIST_PRICE <= b + LIST_PRICE * 1e-6)
    }

    /// How many more paid calls fit the budget.
    pub(super) fn affordable(&self) -> usize {
        match self.budget {
            None => usize::MAX,
            Some(b) => ((b - self.spent) / LIST_PRICE + 1e-6).max(0.0).floor() as usize,
        }
    }

    /// Adds a step's usage; `calls` paid calls were made, and count at
    /// list price if the provider reported no cost.
    pub(super) fn add(&mut self, usage: Vec<(ProviderId, Usage)>, calls: usize) {
        let reported: Option<f64> = usage
            .iter()
            .filter_map(|(_, u)| u.cost)
            .reduce(|a, b| a + b);
        self.spent += reported.unwrap_or(calls as f64 * LIST_PRICE);
        for (provider, u) in usage {
            add_usage(&mut self.usage, &provider, u);
        }
    }
}

/// How many lookups failed, each failure logged (a warning, once per
/// distinct error) with `what` failed.
pub(super) fn failed_lookups(
    answers: &[(crate::Attempt, Option<SourceRecord>)],
    what: &str,
) -> usize {
    let mut seen: Vec<String> = Vec::new();
    let mut failed = 0;
    for (attempt, _) in answers {
        if let Outcome::Failed(e) = &attempt.outcome {
            failed += 1;
            let error = e.to_string();
            if !seen.contains(&error) {
                tracing::warn!(target: "aspero::graph", provider = %attempt.provider, %error, "{what}");
                seen.push(error);
            }
        }
    }
    failed
}

/// What makes a work one of the set: sharing any identifier with any seed,
/// resolved or not (its input, or any id its record carries), or being one
/// work with a seed's record (a preprint and its published version).
pub(super) struct TheSet<'a> {
    ids: Vec<WorkId>,
    records: Vec<&'a SourceRecord>,
}

impl<'a> TheSet<'a> {
    pub(super) fn new(seeds: &'a [Seed]) -> Self {
        let ids = seeds
            .iter()
            .flat_map(|s| {
                let given = s.input.parse::<WorkId>().ok();
                let carried = s.record.iter().flat_map(|r| r.ids.iter().cloned());
                given.into_iter().chain(carried)
            })
            .collect();
        let records = seeds.iter().filter_map(|s| s.record.as_ref()).collect();
        TheSet { ids, records }
    }

    /// Whether the work `id` (with its record, when looked up) is one of
    /// the set.
    pub(super) fn has(&self, id: &WorkId, record: Option<&SourceRecord>) -> bool {
        let is_seed = |id: &WorkId| self.ids.iter().any(|s| same_work_id(s, id));
        is_seed(id)
            || record.is_some_and(|r| {
                r.ids.iter().any(is_seed) || self.records.iter().any(|s| same_work(s, r))
            })
    }
}

/// Candidates looked up: a record (or none) for each, and what stopped
/// the lookup, if anything (`done` of `total` works answered).
pub(super) struct Looked {
    pub(super) records: Vec<Option<SourceRecord>>,
    pub(super) stop: Option<Incomplete>,
}

impl Engine {
    /// Looks the candidates `ids` up at `entry` in whole batches, if the
    /// budget allows every batch (else none is asked); a failure is logged
    /// as `what`. Estimating, counts the paid batches instead.
    pub(super) async fn look_up(
        &self,
        entry: &Entry,
        ids: &[WorkId],
        spend: &mut Spend,
        what: &str,
    ) -> Looked {
        let provider = entry.provider.id();
        let uncached = self
            .cached_lookups(&provider, ids, FieldSet::DEFAULT)
            .iter()
            .filter(|c| c.is_none())
            .count();
        let paid = match uncached {
            0 | 1 => 0,
            n => n.div_ceil(MAX_FILTER),
        };
        let mut looked = Looked {
            records: vec![None; ids.len()],
            stop: None,
        };
        if spend.estimating {
            spend.requests += paid as u64;
            return looked;
        }
        if ids.is_empty() {
            return looked;
        }
        if !spend.allows(paid) {
            looked.stop = Some(Incomplete::new(IncompleteKind::Budget, 0, ids.len()));
            return looked;
        }
        let mut usage = Vec::new();
        let answers = self
            .lookup_at(entry, ids, FieldSet::DEFAULT, &mut usage)
            .await;
        spend.add(usage, paid);
        let failed = failed_lookups(&answers, what);
        if failed > 0 {
            looked.stop = Some(
                Incomplete::new(
                    IncompleteKind::ProviderFailed,
                    ids.len() - failed,
                    ids.len(),
                )
                .from(provider),
            );
        }
        looked.records = answers.into_iter().map(|(_, r)| r).collect();
        looked
    }
}

/// A work found, its versions (a preprint and its published version)
/// merged in.
pub(super) struct Merged<T> {
    /// The first version's id (the best ranked).
    pub(super) id: WorkId,
    /// Every version's, in order.
    pub(super) ids: Vec<WorkId>,
    /// The first version's record.
    pub(super) record: Option<SourceRecord>,
    /// The versions' values, joined.
    pub(super) value: T,
}

/// Merges the works of `found` (id, record when looked up, value; best
/// first) that are one work (`same_work`), in order of their first
/// version, `join`ing their values. Works without a record merge with none.
pub(super) fn merge_versions<T>(
    found: Vec<(WorkId, Option<SourceRecord>, T)>,
    mut join: impl FnMut(&mut T, T),
) -> Vec<Merged<T>> {
    let mut merged: Vec<Merged<T>> = Vec::new();
    for (id, record, value) in found {
        let same = record.as_ref().and_then(|r| {
            merged
                .iter_mut()
                .find(|m| m.record.as_ref().is_some_and(|other| same_work(other, r)))
        });
        match same {
            Some(m) => {
                m.ids.push(id);
                join(&mut m.value, value);
            }
            None => merged.push(Merged {
                ids: vec![id.clone()],
                id,
                record,
                value,
            }),
        }
    }
    merged
}
