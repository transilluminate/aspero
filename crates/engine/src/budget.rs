//! Providers' budgets as their last responses reported them, so a front
//! end can show what's left without spending any.
//!
//! Prepaid credit spent today, which OpenAlex doesn't say, is told from
//! the balance: the first one seen in the budget day less the balance now
//! ([`PrepaidDay`]: the lowest a response's headers said, since they can
//! be minutes stale, unless `/rate-limit` said it since), kept where every
//! process sharing the cache file sees the same day ([`PrepaidDays`];
//! without one, by the engine alone). Like the allowance's use it counts
//! everything using the key, but only from the day's first sighting: a
//! day first seen part-way through, after credit was spent, undercounts.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use aspero_core::{Error, ProviderId, Response, Usage};
use aspero_store::{PrepaidDay, PrepaidDays};
use jiff::Timestamp;

use crate::{Engine, Entry, lock};

/// What a provider last said of its budget.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Budget {
    /// Left in the current period (OpenAlex: today, in USD).
    pub remaining: Option<f64>,
    pub resets_at: Option<Timestamp>,
    /// The period's budget (OpenAlex: the daily allowance), as the
    /// provider says it.
    pub limit: Option<f64>,
    /// A prepaid balance beside the period's budget, where the provider
    /// says one: OpenAlex's as told for the day's credit spend (the lowest
    /// its responses said since `/rate-limit` last did), not the last
    /// response's, which may be stale.
    pub prepaid_remaining: Option<f64>,
    /// When the provider said so.
    pub seen_at: Timestamp,
    /// Prepaid credit spent in the current period (OpenAlex: today, in
    /// USD), by everything using the key: the period's first balance seen
    /// (by any process sharing the cache file), raised by any top-up since,
    /// less the balance now ([`PrepaidDay`]). Seen first part-way through
    /// the period, it misses what was spent before. `None` where no
    /// balance is known this period.
    pub prepaid_spent_today: Option<f64>,
    /// When the prepaid balance expires, where the provider says
    /// (OpenAlex: `/rate-limit` only).
    pub prepaid_expires_at: Option<Timestamp>,
}

impl Budget {
    /// Nothing known yet, as of `seen_at`.
    pub fn new(seen_at: Timestamp) -> Self {
        Budget {
            remaining: None,
            resets_at: None,
            limit: None,
            prepaid_remaining: None,
            seen_at,
            prepaid_spent_today: None,
            prepaid_expires_at: None,
        }
    }

    /// Spent in the current period (`limit − remaining`, to a millionth),
    /// when both are known.
    pub fn used(&self) -> Option<f64> {
        let used = (self.limit? - self.remaining?).max(0.0);
        Some((used * 1e6).round() / 1e6)
    }

    /// Whether the period's allowance is used up (nothing, to a millionth,
    /// remaining), so paid calls spend prepaid credit; `None` if unknown.
    pub fn allowance_used_up(&self) -> Option<bool> {
        Some(self.remaining? < 1e-6)
    }
}

/// The budget day a balance seen at `now` belongs to, `YYYY-MM-DD`: the
/// day of the period ending at `resets_at` (OpenAlex: midnight UTC, so
/// the UTC day; an hour back from the reset, so a reset a moment past
/// midnight still names the day before it), else `now`'s UTC day.
fn budget_day(resets_at: Option<Timestamp>, now: Timestamp) -> String {
    let at = resets_at
        .and_then(|r| r.checked_sub(Duration::from_secs(3600)).ok())
        .unwrap_or(now);
    at.to_zoned(jiff::tz::TimeZone::UTC).date().to_string()
}

pub(crate) struct Budgets {
    all: Mutex<Vec<(ProviderId, Budget)>>,
    /// Each day's prepaid balances, shared with other processes.
    prepaid_days: Option<Arc<dyn PrepaidDays>>,
    /// The same, for this engine alone: without a store, or while it fails.
    own_days: Mutex<Vec<(ProviderId, String, PrepaidDay)>>,
}

impl Budgets {
    pub(crate) fn new(prepaid_days: Option<Arc<dyn PrepaidDays>>) -> Self {
        Budgets {
            all: Mutex::default(),
            prepaid_days,
            own_days: Mutex::default(),
        }
    }

    /// Remembers what a response (or a spent-budget error) said. What it
    /// leaves out (the limit, a prepaid balance, its expiry) is kept from
    /// before, and the credit spent today while the period lasts.
    pub(crate) fn note(
        &self,
        provider: &ProviderId,
        usage: Option<&Usage>,
        error: Option<&Error>,
        now: Timestamp,
    ) {
        let (remaining, resets_at, limit, prepaid, expires, bought) = match (usage, error) {
            (Some(u), _) => (
                u.remaining,
                u.resets_at,
                u.limit,
                u.prepaid_remaining,
                u.prepaid_expires_at,
                u.prepaid_bought,
            ),
            (None, Some(Error::QuotaExhausted { resets_at })) => {
                (Some(0.0), *resets_at, None, None, None, None)
            }
            _ => return,
        };
        if remaining.is_none() && resets_at.is_none() && limit.is_none() && prepaid.is_none() {
            return;
        }
        // Before the budgets' lock: it may wait on the file.
        let day = prepaid.map(|balance| {
            self.see_prepaid(provider, &budget_day(resets_at, now), balance, bought, now)
        });
        let spent = day.map(|d| d.spent_usd());
        let prepaid = day.map(|d| d.last_usd);
        let mut all = lock(&self.all);
        let known = all.iter().find(|(p, _)| p == provider).map(|(_, b)| *b);
        let still_today = |k: &Budget| k.resets_at.is_some_and(|r| now < r);
        let budget = Budget {
            remaining,
            resets_at,
            limit: limit.or(known.and_then(|k| k.limit)),
            prepaid_remaining: prepaid.or(known.and_then(|k| k.prepaid_remaining)),
            seen_at: now,
            prepaid_spent_today: spent.or(known
                .filter(still_today)
                .and_then(|k| k.prepaid_spent_today)),
            prepaid_expires_at: expires.or(known.and_then(|k| k.prepaid_expires_at)),
        };
        match all.iter_mut().find(|(p, _)| p == provider) {
            Some((_, known)) => *known = budget,
            None => all.push((provider.clone(), budget)),
        }
    }

    /// The budget day of `provider`'s prepaid balance with `balance` seen:
    /// from the shared store, or, without one or if it fails, this
    /// engine's own.
    fn see_prepaid(
        &self,
        provider: &ProviderId,
        day: &str,
        balance: f64,
        bought: Option<f64>,
        now: Timestamp,
    ) -> PrepaidDay {
        if let Some(days) = &self.prepaid_days {
            match days.see_prepaid(provider.as_str(), day, balance, bought, now) {
                Ok(seen) => return seen,
                Err(e) => tracing::warn!(
                    target: "aspero::budget",
                    provider = %provider, error = %e,
                    "couldn't share the day's prepaid balance; keeping it in this process"
                ),
            }
        }
        let mut own = lock(&self.own_days);
        own.retain(|(p, d, _)| p != provider || d == day);
        let known = own
            .iter()
            .find(|(p, _, _)| p == provider)
            .map(|(_, _, seen)| *seen);
        let seen = PrepaidDay::see(known, balance, bought, now);
        match own.iter_mut().find(|(p, _, _)| p == provider) {
            Some((_, _, kept)) => *kept = seen,
            None => own.push((provider.clone(), day.to_owned(), seen)),
        }
        seen
    }

    pub(crate) fn all(&self) -> Vec<(ProviderId, Budget)> {
        lock(&self.all).clone()
    }
}

impl Engine {
    /// Asks each provider that can say so for free (OpenAlex, with a key:
    /// `/rate-limit`) what's left of its budget, then returns every
    /// provider's budget as [`budgets`](Self::budgets) has it. Providers
    /// that can't be asked for free (OpenAlex without a key) are sent
    /// nothing and keep what their last responses said, as is every
    /// provider [`offline`](crate::CallOptions::offline).
    pub async fn refresh_budgets(&self) -> Vec<(ProviderId, Budget)> {
        if !self.options.offline {
            let asks = self
                .entries
                .iter()
                .filter(|e| e.provider.capabilities().budget_status)
                .map(|e| self.refresh_budget(e));
            futures::future::join_all(asks).await;
        }
        self.budgets()
    }

    /// Asks one provider its budget, which [`call`](Self::call) remembers.
    pub(crate) async fn refresh_budget(&self, entry: &Entry) {
        let asked = self
            .call(entry, || async {
                let usage = entry.provider.budget().await?;
                Ok(Response::with_usage((), usage))
            })
            .await;
        if let Err(e) = asked {
            tracing::warn!(
                target: "aspero::budget",
                provider = %entry.provider.id(), error = %e, "couldn't ask the budget"
            );
        }
    }
}
