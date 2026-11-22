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
//!
//! The credit spent "here", which a daily credit cap counts, is what calls
//! made through this engine, and any other sharing its store (the cache
//! file), were charged once the allowance was used up: each response's
//! cost, as far as it went past the allowance left before it
//! ([`credit_part`]), added up per budget day in the store
//! ([`PrepaidDays::add_credit_here`]; without one, by the engine alone).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use aspero_core::{Error, Prices, ProviderId, Response, Usage};
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
    /// Prepaid credit charged in the current period (OpenAlex: today, in
    /// USD) to calls made "here": through this engine and any other
    /// sharing its store ([`EngineBuilder::prepaid_days`](crate::EngineBuilder::prepaid_days),
    /// usually the cache file), not other tools using the key. What the
    /// daily credit cap counts. Each paid call's cost once the allowance
    /// is used up (the part past what was left of it, where that's known);
    /// replayed responses cost nothing. `None` until the provider reports
    /// a budget.
    pub credit_spent_today_here: Option<f64>,
    /// When the prepaid balance expires, where the provider says
    /// (OpenAlex: `/rate-limit` only).
    pub prepaid_expires_at: Option<Timestamp>,
    /// What each kind of call costs, as the provider last said (OpenAlex:
    /// `/rate-limit` only, so `None` until it's asked, with a key); kept
    /// until it says otherwise.
    pub prices: Option<Prices>,
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
            credit_spent_today_here: None,
            prepaid_expires_at: None,
            prices: None,
        }
    }

    /// What a page of a keyword search costs, where the provider says.
    pub fn search_price(&self) -> Option<f64> {
        self.prices?.search
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

/// The part of a call's `cost` that was prepaid credit, to a millionth,
/// given the allowance left after it (`after`, its response's) and the
/// least seen left before it in the budget day (`before`). OpenAlex spends
/// the allowance first, so a cost is credit only once the allowance is
/// gone (either says none left): past `before`, where that's known (the
/// call that used the allowance up), else all of it. A `before` higher
/// than the truth (other tools spent since; a stale server) counts that
/// one call's credit short; nothing seen before a call that finds the
/// allowance gone counts it all, its allowance part included.
pub(crate) fn credit_part(cost: Option<f64>, after: Option<f64>, before: Option<f64>) -> f64 {
    let Some(cost) = cost.filter(|c| *c > 0.0) else {
        return 0.0;
    };
    let gone = |left: Option<f64>| left.is_some_and(|l| l < 1e-6);
    if !gone(after) && !gone(before) {
        return 0.0;
    }
    let left = before.map_or(0.0, |l| l.max(0.0));
    ((cost - left).max(0.0) * 1e6).round() / 1e6
}

/// Per provider and budget day, a figure kept in this engine.
type Daily<T> = Mutex<Vec<(ProviderId, String, T)>>;

pub(crate) struct Budgets {
    all: Mutex<Vec<(ProviderId, Budget)>>,
    /// Each day's prepaid balances, and the credit spent here, shared with
    /// other processes.
    prepaid_days: Option<Arc<dyn PrepaidDays>>,
    /// The balances for this engine alone: without a store, or while it
    /// fails.
    own_days: Daily<PrepaidDay>,
    /// The credit this engine spent: without a store, or while it fails.
    own_credit: Daily<f64>,
    /// The least allowance left each provider's responses said today: a
    /// call's cost past it is credit ([`credit_part`]).
    least_left: Daily<f64>,
}

impl Budgets {
    pub(crate) fn new(prepaid_days: Option<Arc<dyn PrepaidDays>>) -> Self {
        Budgets {
            all: Mutex::default(),
            prepaid_days,
            own_days: Mutex::default(),
            own_credit: Mutex::default(),
            least_left: Mutex::default(),
        }
    }

    /// Remembers what a response (or a spent-budget error) said. What it
    /// leaves out (the limit, a prepaid balance, its expiry) is kept from
    /// before, and the credit spent today while the period lasts. Prices
    /// are kept until said again.
    pub(crate) fn note(
        &self,
        provider: &ProviderId,
        usage: Option<&Usage>,
        error: Option<&Error>,
        now: Timestamp,
    ) {
        let (cost, remaining, resets_at, limit, prepaid, expires, bought, prices) =
            match (usage, error) {
                (Some(u), _) => (
                    u.cost,
                    u.remaining,
                    u.resets_at,
                    u.limit,
                    u.prepaid_remaining,
                    u.prepaid_expires_at,
                    u.prepaid_bought,
                    u.prices,
                ),
                (None, Some(Error::QuotaExhausted { resets_at })) => {
                    (None, Some(0.0), *resets_at, None, None, None, None, None)
                }
                _ => return,
            };
        if remaining.is_none()
            && resets_at.is_none()
            && limit.is_none()
            && prepaid.is_none()
            && prices.is_none()
        {
            return;
        }
        // Before the budgets' lock: it may wait on the file.
        let today = budget_day(resets_at, now);
        let credit = self.credit_charged(provider, &today, cost, remaining);
        let here = self.spend_here(provider, &today, credit);
        let day = prepaid.map(|balance| self.see_prepaid(provider, &today, balance, bought, now));
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
            credit_spent_today_here: Some(here),
            prepaid_expires_at: expires.or(known.and_then(|k| k.prepaid_expires_at)),
            prices: prices.or(known.and_then(|k| k.prices)),
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
                Err(e) => {
                    tracing::warn!(
                        target: "aspero::budget",
                        provider = %provider,
                        "prepaid_unshared"
                    );
                    tracing::debug!(target: "aspero::budget", provider = %provider, error = %e, "prepaid_detail");
                }
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

    /// The credit a call to `provider` was charged on `day`, given its
    /// `cost` and the allowance its response said was `left`
    /// ([`credit_part`]), which then lowers the least seen left.
    fn credit_charged(
        &self,
        provider: &ProviderId,
        day: &str,
        cost: Option<f64>,
        left: Option<f64>,
    ) -> f64 {
        let mut least = lock(&self.least_left);
        least.retain(|(p, d, _)| p != provider || d == day);
        let known = least.iter_mut().find(|(p, _, _)| p == provider);
        let before = known.as_ref().map(|(_, _, l)| *l);
        let credit = credit_part(cost, left, before);
        match (known, left) {
            (Some((_, _, l)), Some(left)) => *l = l.min(left),
            (None, Some(left)) => least.push((provider.clone(), day.to_owned(), left)),
            _ => {}
        }
        credit
    }

    /// Adds `usd` of credit spent here to `provider`'s `day`, and returns
    /// the day's total: from the shared store, or, without one or if it
    /// fails, this engine's own.
    fn spend_here(&self, provider: &ProviderId, day: &str, usd: f64) -> f64 {
        let own = {
            let mut own = lock(&self.own_credit);
            own.retain(|(p, d, _)| p != provider || d == day);
            if let Some((_, _, total)) = own.iter_mut().find(|(p, _, _)| p == provider) {
                *total = ((*total + usd) * 1e6).round() / 1e6;
                *total
            } else {
                own.push((provider.clone(), day.to_owned(), usd));
                usd
            }
        };
        if let Some(days) = &self.prepaid_days {
            let shared = if usd > 0.0 {
                days.add_credit_here(provider.as_str(), day, usd)
            } else {
                days.credit_here(provider.as_str(), day)
            };
            match shared {
                Ok(total) => return total,
                Err(e) => {
                    tracing::warn!(
                        target: "aspero::budget",
                        provider = %provider,
                        "credit_unshared"
                    );
                    tracing::debug!(target: "aspero::budget", provider = %provider, error = %e, "credit_detail");
                }
            }
        }
        own
    }

    /// The credit spent here on `provider`'s budget day ending at
    /// `resets_at`, as it stands now (other engines sharing the store may
    /// have spent since this one's last response).
    pub(crate) fn spent_here(
        &self,
        provider: &ProviderId,
        resets_at: Option<Timestamp>,
        now: Timestamp,
    ) -> f64 {
        self.spend_here(provider, &budget_day(resets_at, now), 0.0)
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
            .call(entry, crate::observe::op::BUDGET, || async {
                let usage = entry.provider.budget().await?;
                Ok(Response::with_usage((), usage))
            })
            .await;
        if let Err(e) = asked {
            tracing::warn!(
                target: "aspero::budget",
                provider = %entry.provider.id(), error = %e.kind().as_str(), "budget_unread"
            );
            tracing::debug!(target: "aspero::budget", error = %e, "budget_detail");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cost is credit only once the allowance is gone: all of it, or
    /// for the call that used the allowance up, the part past what was
    /// left before it.
    #[test]
    fn a_call_is_charged_credit_once_the_allowance_is_gone() {
        let search = Some(0.001);
        // The allowance left after: none of it credit.
        assert_eq!(credit_part(search, Some(0.5), Some(0.501)), 0.0);
        assert_eq!(credit_part(search, Some(0.5), None), 0.0);
        // Gone, and gone before: all of it.
        assert_eq!(credit_part(search, Some(0.0), Some(0.0)), 0.001);
        // Gone, nothing seen before: all of it, its allowance part too.
        assert_eq!(credit_part(search, Some(0.0), None), 0.001);
        // The call that used it up: past what was left.
        assert_eq!(credit_part(search, Some(0.0), Some(0.0004)), 0.0006);
        assert_eq!(credit_part(search, Some(0.0), Some(0.001)), 0.0);
        assert_eq!(credit_part(search, Some(0.0), Some(0.3)), 0.0);
        // Seen gone before, a stale server saying some left: credit.
        assert_eq!(credit_part(search, Some(0.2), Some(0.0)), 0.001);
        // Free calls, and replays (cost 0), are never credit.
        assert_eq!(credit_part(Some(0.0), Some(0.0), Some(0.0)), 0.0);
        assert_eq!(credit_part(None, Some(0.0), Some(0.0)), 0.0);
    }

    /// The least allowance seen left in the day is what the next call's
    /// cost is counted past, whatever order responses come in.
    #[test]
    fn credit_is_counted_past_the_least_allowance_seen_today() {
        let budgets = Budgets::new(None);
        let openalex = ProviderId::from_static("openalex");
        let day = "2026-11-16";
        let charged = |cost, left| budgets.credit_charged(&openalex, day, Some(cost), Some(left));
        assert_eq!(charged(0.0, 0.0004), 0.0, "a refresh");
        assert_eq!(charged(0.001, 0.0), 0.0006);
        assert_eq!(charged(0.001, 0.0), 0.001);
        assert_eq!(charged(0.001, 0.5), 0.001, "a stale server");
        assert_eq!(budgets.spend_here(&openalex, day, 0.0016), 0.0016);
        assert_eq!(budgets.spend_here(&openalex, day, 0.001), 0.0026);
        // A new day starts afresh.
        let next = "2026-11-17";
        assert_eq!(
            budgets.credit_charged(&openalex, next, Some(0.001), Some(0.9)),
            0.0
        );
        assert_eq!(budgets.spend_here(&openalex, next, 0.0), 0.0);
    }
}
