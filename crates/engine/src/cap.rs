//! A caller's daily caps on a provider's spend (OpenAlex's, in USD): one on
//! its daily allowance, and one on the prepaid credit it spends once the
//! allowance is used up.
//!
//! The day's use is the provider's own figure (`limit − remaining`, from
//! every response's headers or a free refresh), so it counts everything
//! using the key: other apps, scripts, other processes. The credit cap
//! counts only the credit spent "here": charged to calls made through this
//! engine and any other sharing its store
//! ([`Budget::credit_spent_today_here`]). OpenAlex gives one key per
//! account, shared by a researcher's scripts and apps, so a cap counting
//! everything on the key would stop a caller that spent nothing; the
//! allowance's cap is about leaving the rest of the day's allowance to
//! those other tools, so it counts them. Before a paid call ([`reached`]):
//!
//! - **Allowance left**: the call goes ahead while the day's use is under
//!   the allowance's cap ([`daily_cap`](Engine::daily_cap)); past it, it
//!   stops, holding the rest of the allowance back for other tools.
//! - **Allowance used up** (nothing left, by whatever used it), with a
//!   credit cap ([`daily_credit_cap`](Engine::daily_credit_cap)): the call
//!   goes ahead, spending credit, while the credit spent here today is
//!   under it, whatever the allowance's cap; a cap of 0 never spends
//!   credit.
//! - **Allowance used up, no credit cap** (only an engine built without
//!   [`standard`](crate::standard), which always sets one, $0.25 unless
//!   configured): as before credit had a cap of its own. The day's use tops out at the allowance, so an allowance cap
//!   at or under it stops the call (no credit is spent); one above it, or
//!   none, never does (credit is spent until it's gone).
//!
//! A reached cap stops paid calls until the budget resets: searches fail
//! with [`Error::BudgetCapped`], saying which cap ([`CapKind`]), and fall
//! back like any failure (a stale cached page, the next provider in a
//! check), and lookups go
//! [`max_free_batch`](aspero_core::Capabilities::max_free_batch) IDs at a
//! time, which costs nothing, or fall back where none is free.
//!
//! Use unknown (no response seen yet today): with a key, one free
//! refresh is asked first (OpenAlex's `/rate-limit`), so a day already
//! past a cap stops at once; without one, or if the refresh fails, the
//! call is taken as under the caps and they apply from its response on.
//! Either way a call is checked before it's sent and its cost known only
//! after, so the last call (or calls in flight together) can pass a cap
//! by their own cost.

use std::time::Duration;

use aspero_core::{CapKind, Error, ProviderId, Response};

use crate::{Budget, CallOptions, Engine, Entry, lock};

/// After a refresh that couldn't say the day's use, how long before
/// another is tried.
const REFRESH_AGAIN: Duration = Duration::from_secs(60);

/// A provider's caps, in USD.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Caps {
    pub allowance: Option<f64>,
    pub credit: Option<f64>,
}

impl Caps {
    fn any(self) -> bool {
        self.allowance.is_some() || self.credit.is_some()
    }
}

/// The cap a paid call would pass, with the cap and what's spent against
/// it, given the day so far: the allowance's use (`used`), whether it's
/// used up, and the credit spent here today (unknown: none).
pub(crate) fn reached(
    caps: Caps,
    used: f64,
    used_up: bool,
    credit_spent: Option<f64>,
) -> Option<(CapKind, f64, f64)> {
    match caps.credit {
        Some(cap) if used_up => {
            let spent = credit_spent.unwrap_or(0.0);
            (spent >= cap).then_some((CapKind::Credit, cap, spent))
        }
        _ => caps
            .allowance
            .filter(|cap| used >= *cap)
            .map(|cap| (CapKind::Allowance, cap, used)),
    }
}

/// What paid calls may still spend today under `caps`, given the day so
/// far (`b`), in the provider's units (OpenAlex: USD), as [`reached`]
/// decides: the allowance left, held to its cap; then, if paid calls would
/// use the allowance up (its cap at or above it, or it's used up already),
/// the prepaid credit left, held to the credit cap less the credit spent
/// here today (unknown: none). Without a credit cap, credit only where the
/// allowance's cap is above the allowance, or there's none. `None` when
/// the allowance left is unknown; the day's use unknown counts as none.
pub(crate) fn spendable(caps: Caps, b: &Budget) -> Option<f64> {
    let remaining = b.remaining?.max(0.0);
    let used = b.used().unwrap_or(0.0);
    let allowance = caps
        .allowance
        .map_or(remaining, |cap| remaining.min((cap - used).max(0.0)));
    let used_up = remaining < 1e-6;
    let balance = b.prepaid_remaining.unwrap_or(0.0).max(0.0);
    let credit = match caps.credit {
        Some(cap) if used_up || caps.allowance.is_none_or(|a| a >= used + remaining) => {
            balance.min((cap - b.credit_spent_today_here.unwrap_or(0.0)).max(0.0))
        }
        None if caps.allowance.is_none_or(|a| a > used + remaining) => balance,
        _ => 0.0,
    };
    Some(((allowance + credit) * 1e6).round() / 1e6)
}

impl Engine {
    /// What `provider`'s paid calls may still spend today, given `budget`
    /// (as [`budgets`](Self::budgets) has it) and the caps set here: the
    /// allowance left, up to its cap, then the prepaid credit left, up to
    /// the credit cap less the credit spent here today, once paid calls
    /// would use the allowance up. Like the caps, the allowance counts
    /// everything using the key, the credit only what was spent here.
    /// `None` when the allowance left is unknown.
    pub fn spendable_today(&self, provider: &ProviderId, budget: &Budget) -> Option<f64> {
        spendable(self.caps(provider), budget)
    }

    /// The daily cap set on `provider`'s allowance, if any.
    pub fn daily_cap(&self, provider: &ProviderId) -> Option<f64> {
        find(&self.caps, provider)
    }

    /// The daily cap set on `provider`'s prepaid credit spend, if any.
    pub fn daily_credit_cap(&self, provider: &ProviderId) -> Option<f64> {
        find(&self.credit_caps, provider)
    }

    fn caps(&self, provider: &ProviderId) -> Caps {
        Caps {
            allowance: self.daily_cap(provider),
            credit: self.daily_credit_cap(provider),
        }
    }

    /// Runs a call that costs: refused with [`Error::BudgetCapped`] once
    /// the provider's day has reached a cap.
    pub(crate) async fn call_paid<T, F, Fut>(
        &self,
        entry: &Entry,
        op: F,
    ) -> Result<Response<T>, Error>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<Response<T>, Error>>,
    {
        self.check_cap(entry).await?;
        self.call(entry, op).await
    }

    /// `Err(BudgetCapped)` when `entry`'s provider has caps and its day has
    /// reached one; a day of unknown use is refreshed first where that's
    /// free.
    pub(crate) async fn check_cap(&self, entry: &Entry) -> Result<(), Error> {
        let provider = entry.provider.id();
        let caps = self.caps(&provider);
        if !caps.any() {
            return Ok(());
        }
        if self.today(&provider).is_none()
            && entry.provider.capabilities().budget_status
            && !self.options.offline
        {
            // One refresh at a time; callers waiting on it then see its
            // answer.
            let _one = self.refreshing.lock().await;
            let due = lock(&self.refreshed).is_none_or(|at| at.elapsed() >= REFRESH_AGAIN);
            if self.today(&provider).is_none() && due {
                self.quick_refresh(entry).await;
                if self.today(&provider).is_none() {
                    *lock(&self.refreshed) = Some(tokio::time::Instant::now());
                }
            }
        }
        match self.reached_today(&provider, caps) {
            Some((which, cap, used, resets_at)) => {
                tracing::info!(
                    target: "aspero::budget",
                    provider = %provider, cap, used, which = which.as_str(),
                    "paid call stopped at a daily cap"
                );
                Err(Error::BudgetCapped {
                    cap,
                    used,
                    resets_at,
                    which,
                })
            }
            None => Ok(()),
        }
    }

    /// The cap the provider's day has reached, if any, with the cap,
    /// what's spent against it, and when the day resets. The credit spent
    /// here is read again where it decides: other engines sharing the
    /// store may have spent since this one's last response.
    fn reached_today(
        &self,
        provider: &ProviderId,
        caps: Caps,
    ) -> Option<(CapKind, f64, f64, Option<jiff::Timestamp>)> {
        let budget = self.today(provider)?;
        let (used, used_up) = (budget.used()?, budget.allowance_used_up()?);
        let here = if used_up && caps.credit.is_some() {
            Some(
                self.budgets
                    .spent_here(provider, budget.resets_at, (self.clock)()),
            )
        } else {
            budget.credit_spent_today_here
        };
        let (which, cap, used) = reached(caps, used, used_up, here)?;
        Some((which, cap, used, budget.resets_at))
    }

    /// Asks the provider's budget, quickly: 5 s, no retries.
    async fn quick_refresh(&self, entry: &Entry) {
        let quick = CallOptions {
            deadline: Some(Duration::from_secs(5)),
            max_retries: Some(0),
            ..self.options
        };
        self.with_options(quick).refresh_budget(entry).await;
    }

    /// The provider's budget as last reported, if its use today is known;
    /// `None` when unknown, or the budget has reset since.
    fn today(&self, provider: &ProviderId) -> Option<Budget> {
        let now = (self.clock)();
        let (_, budget) = self
            .budgets
            .all()
            .into_iter()
            .find(|(p, _)| p == provider)?;
        if budget.resets_at.is_some_and(|r| now >= r) {
            return None;
        }
        budget.used().map(|_| budget)
    }
}

fn find(caps: &[(ProviderId, f64)], provider: &ProviderId) -> Option<f64> {
    caps.iter()
        .find(|(p, _)| p == provider)
        .map(|(_, cap)| *cap)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every case of the decision: allowance under or over its cap, used
    /// up or not, and a credit cap of none, 0, or some, under or over.
    #[test]
    fn which_cap_a_paid_call_reaches() {
        let caps = |allowance, credit| Caps { allowance, credit };
        let go = None;
        let allowance = |cap, used| Some((CapKind::Allowance, cap, used));
        let credit = |cap, spent| Some((CapKind::Credit, cap, spent));

        // Allowance left: its cap decides, whatever the credit's.
        for credit_cap in [None, Some(0.0), Some(1.0)] {
            assert_eq!(
                reached(caps(Some(0.5), credit_cap), 0.4, false, Some(3.0)),
                go
            );
            assert_eq!(
                reached(caps(Some(0.5), credit_cap), 0.6, false, Some(0.0)),
                allowance(0.5, 0.6),
                "past the allowance's cap, the rest of it is held back"
            );
            assert_eq!(reached(caps(None, credit_cap), 0.9, false, None), go);
        }

        // Used up, with a credit cap: the credit spent today decides,
        // whatever the allowance's cap.
        for allowance_cap in [None, Some(0.5), Some(1.0), Some(2.0)] {
            let c = |credit_cap| caps(allowance_cap, Some(credit_cap));
            assert_eq!(reached(c(1.0), 1.0, true, Some(0.25)), go);
            assert_eq!(reached(c(1.0), 1.0, true, None), go, "none seen: none");
            assert_eq!(reached(c(1.0), 1.0, true, Some(1.02)), credit(1.0, 1.02));
            assert_eq!(reached(c(1.0), 1.0, true, Some(1.0)), credit(1.0, 1.0));
            assert_eq!(
                reached(c(0.0), 1.0, true, Some(0.0)),
                credit(0.0, 0.0),
                "0 never spends credit"
            );
            assert_eq!(reached(c(0.0), 1.0, true, None), credit(0.0, 0.0));
        }

        // Used up, no credit cap: as before, the allowance's cap, which
        // the day's use (topping out at the allowance) reaches only at or
        // under the allowance.
        assert_eq!(reached(caps(None, None), 1.0, true, Some(5.0)), go);
        assert_eq!(
            reached(caps(Some(0.5), None), 1.0, true, Some(0.0)),
            allowance(0.5, 1.0)
        );
        assert_eq!(
            reached(caps(Some(1.0), None), 1.0, true, Some(0.0)),
            allowance(1.0, 1.0)
        );
        assert_eq!(reached(caps(Some(2.0), None), 1.0, true, Some(5.0)), go);
    }

    /// What's spendable agrees with where [`reached`] stops: the allowance
    /// to its cap, then credit to its own.
    #[test]
    fn what_paid_calls_may_still_spend_today() {
        let caps = |allowance, credit| Caps { allowance, credit };
        let day = |remaining, prepaid, spent| {
            let mut b = Budget::new(jiff::Timestamp::UNIX_EPOCH);
            b.limit = Some(1.0);
            b.remaining = remaining;
            b.prepaid_remaining = prepaid;
            b.credit_spent_today_here = spent;
            // Other tools' credit on the key counts for nothing here.
            b.prepaid_spent_today = Some(100.0);
            b
        };
        let spend = |c, b: &Budget| spendable(c, b);

        // No caps: the allowance left, then all the credit.
        assert_eq!(
            spend(caps(None, None), &day(Some(0.6), Some(5.0), None)),
            Some(5.6)
        );
        // The allowance's cap holds the rest back, and no credit is reached.
        assert_eq!(
            spend(
                caps(Some(0.5), Some(0.25)),
                &day(Some(0.6), Some(5.0), None)
            ),
            Some(0.1)
        );
        assert_eq!(
            spend(caps(Some(0.3), None), &day(Some(0.6), Some(5.0), None)),
            Some(0.0)
        );
        // A cap at the allowance: all of it, then credit to the credit cap.
        assert_eq!(
            spend(
                caps(Some(1.0), Some(0.25)),
                &day(Some(0.6), Some(5.0), Some(0.05))
            ),
            Some(0.8)
        );
        // Used up (by anything), with a credit cap: what's left of it, at
        // most the balance; whatever the allowance's cap.
        assert_eq!(
            spend(
                caps(Some(0.5), Some(0.25)),
                &day(Some(0.0), Some(5.0), Some(0.1))
            ),
            Some(0.15)
        );
        assert_eq!(
            spend(caps(None, Some(0.25)), &day(Some(0.0), Some(0.1), None)),
            Some(0.1)
        );
        assert_eq!(
            spend(
                caps(None, Some(0.25)),
                &day(Some(0.0), Some(5.0), Some(0.3))
            ),
            Some(0.0)
        );
        assert_eq!(
            spend(caps(None, Some(0.0)), &day(Some(0.0), Some(5.0), None)),
            Some(0.0)
        );
        // Used up, no credit cap: credit only past an allowance's cap above
        // the allowance.
        assert_eq!(
            spend(caps(Some(1.0), None), &day(Some(0.0), Some(5.0), None)),
            Some(0.0)
        );
        assert_eq!(
            spend(caps(Some(2.0), None), &day(Some(0.0), Some(5.0), None)),
            Some(5.0)
        );
        // No balance known: none; the allowance left unknown: unknown.
        assert_eq!(
            spend(caps(None, None), &day(Some(0.0), None, None)),
            Some(0.0)
        );
        assert_eq!(spend(caps(None, None), &day(None, Some(5.0), None)), None);
    }
}
