//! What an analysis spends, and what counts as one of the set.

use aspero_core::matching::{same_work, same_work_id};
use aspero_core::{ProviderId, SourceRecord, Usage, WorkId};

use super::{LIST_PRICE, Seed};
use crate::{Outcome, add_usage};

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
