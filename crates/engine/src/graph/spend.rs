//! What an analysis spends, the works it looks up (candidates: probably
//! missing, related), and what counts as one of the set.

use aspero_core::matching::{same_work, same_work_id};
use aspero_core::{ErrorKind, FieldSet, ProviderId, SourceRecord, Usage, WorkId};

use super::{Incomplete, IncompleteKind, LIST_PRICE, Seed};
use std::sync::Mutex;

use crate::lookup::Cached;
use crate::{Engine, Entry, Outcome, add_usage, lock};

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
            // Floored and non-negative; a float cast saturates past usize::MAX.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
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

/// What pages read at once may still spend, by what they cost: each page
/// asked takes list price, then what OpenAlex says it cost. So a page that
/// costs more than list price leaves less for the others, and none is
/// asked that could take the spend past the budget at list price.
pub(super) struct Purse {
    left: Option<Mutex<f64>>,
}

impl Purse {
    /// What `spend` leaves, less `keep` paid calls' worth (kept back).
    pub(super) fn new(spend: &Spend, keep: usize) -> Self {
        let left = spend
            .budget
            .map(|b| Mutex::new(b - spend.spent - keep as f64 * LIST_PRICE));
        Purse { left }
    }

    /// Takes a page's list price, if it fits.
    pub(super) fn take(&self) -> bool {
        let Some(left) = &self.left else {
            return true;
        };
        let mut left = lock(left);
        // A hair over, as `Spend::allows`.
        let fits = *left + LIST_PRICE * 1e-6 >= LIST_PRICE;
        if fits {
            *left -= LIST_PRICE;
        }
        fits
    }

    /// A page taken was asked: it cost `cost` (list price, unreported), or
    /// nothing when it failed.
    pub(super) fn settle(&self, cost: Option<f64>) {
        if let Some(left) = &self.left {
            *lock(left) -= cost.unwrap_or(LIST_PRICE) - LIST_PRICE;
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
                tracing::warn!(target: "aspero::engine", provider = %attempt.provider, status = %e.kind().as_str(), "{what}");
                tracing::debug!(target: "aspero::engine", provider = %attempt.provider, %error, "lookup_detail");
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
/// the lookup, if anything (`done` of `total` works answered): a failure,
/// then the budget.
pub(super) struct Looked {
    pub(super) records: Vec<Option<SourceRecord>>,
    pub(super) stops: Vec<Incomplete>,
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
            .filter(|c| matches!(c, Cached::Uncached))
            .count();
        let paid = paid_batches(uncached, entry);
        let mut looked = Looked {
            records: vec![None; ids.len()],
            stops: Vec::new(),
        };
        if spend.estimating {
            spend.requests += paid as u64;
            return looked;
        }
        if ids.is_empty() {
            return looked;
        }
        if !spend.allows(paid) {
            looked.stops = vec![Incomplete::new(IncompleteKind::Budget, 0, ids.len())];
            return looked;
        }
        let mut usage = Vec::new();
        let answers = self
            .lookup_at(entry, ids, FieldSet::DEFAULT, &mut usage)
            .await;
        spend.add(usage, paid);
        // The caller's daily cap stopping a batch is the budget.
        let failed = failed_lookups(&answers, what);
        let capped = answers
            .iter()
            .filter(|(a, _)| matches!(&a.outcome, Outcome::Failed(e) if e.kind() == ErrorKind::BudgetCapped))
            .count();
        let done = ids.len() - failed;
        let failure = (failed > capped).then(|| {
            Incomplete::new(IncompleteKind::ProviderFailed, done, ids.len()).from(provider)
        });
        let over = (capped > 0).then(|| Incomplete::new(IncompleteKind::Budget, done, ids.len()));
        looked.stops = failure.into_iter().chain(over).collect();
        looked.records = answers.into_iter().map(|(_, r)| r).collect();
        looked
    }
}

/// The paid requests looking `n` ids up at `entry` takes, as `lookup_at`
/// asks them: batches of the most it takes (100 at OpenAlex), each paid,
/// but one no bigger than a free batch (OpenAlex asks a lone id free).
fn paid_batches(n: usize, entry: &Entry) -> usize {
    let caps = entry.provider.capabilities();
    let (size, free) = (caps.max_batch.max(1), caps.max_free_batch);
    let paid = |len: usize| usize::from(len > free);
    (n / size) * paid(size) + paid(n % size)
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
/// first) that are one work (`same_work`), directly or through others (a
/// record carrying one's DOI and another's arXiv ID), in order of their
/// first version, `join`ing their values. Works without a record merge
/// with none.
pub(super) fn merge_versions<T>(
    found: Vec<(WorkId, Option<SourceRecord>, T)>,
    mut join: impl FnMut(&mut T, T),
) -> Vec<Merged<T>> {
    let records: Vec<Option<&SourceRecord>> = found.iter().map(|(_, r, _)| r.as_ref()).collect();
    let classes = classes(found.len(), |a, b| match (records[a], records[b]) {
        (Some(x), Some(y)) => same_work(x, y),
        _ => false,
    });
    let mut found: Vec<Option<(WorkId, Option<SourceRecord>, T)>> =
        found.into_iter().map(Some).collect();
    let mut take = |k: usize| found[k].take().expect("each work in one class");
    classes
        .into_iter()
        .map(|class| {
            let (id, record, mut value) = take(class[0]);
            let mut ids = vec![id.clone()];
            for &k in &class[1..] {
                let (other, _, more) = take(k);
                ids.push(other);
                join(&mut value, more);
            }
            Merged {
                id,
                ids,
                record,
                value,
            }
        })
        .collect()
}

/// The classes of `0..n` that `same` joins, directly or through others
/// (every pair asked: `n` is a few hundred at most), each in order, in
/// order of their first.
pub(super) fn classes(n: usize, same: impl Fn(usize, usize) -> bool) -> Vec<Vec<usize>> {
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let mut parent: Vec<usize> = (0..n).collect();
    for a in 0..n {
        for b in a + 1..n {
            let (x, y) = (root(&mut parent, a), root(&mut parent, b));
            if x != y && same(a, b) {
                // The earlier root stays: a class's root is its first.
                parent[x.max(y)] = x.min(y);
            }
        }
    }
    let mut classes: Vec<Vec<usize>> = Vec::new();
    let mut at: Vec<Option<usize>> = vec![None; n];
    for i in 0..n {
        let r = root(&mut parent, i);
        if let Some(c) = at[r] {
            classes[c].push(i);
        } else {
            at[r] = Some(classes.len());
            classes.push(vec![i]);
        }
    }
    classes
}
