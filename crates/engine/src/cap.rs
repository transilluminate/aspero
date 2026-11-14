//! A caller's daily cap on a provider's spend (OpenAlex's, in USD).
//!
//! The day's use is the provider's own figure (`limit − remaining`, from
//! every response's headers or a free refresh), so it counts everything
//! using the key: other apps, scripts, other processes. Once it reaches
//! the cap, paid calls stop until the budget resets: searches fail with
//! [`Error::BudgetCapped`] (and fall back like any failure: a stale cached
//! page, the next provider in a check), and lookups go
//! [`max_free_batch`](aspero_core::Capabilities::max_free_batch) IDs at a
//! time, which costs nothing, or fall back where none is free.
//!
//! Use unknown (no response seen yet today): with a key, one free
//! refresh is asked first (OpenAlex's `/rate-limit`), so a day already
//! past the cap stops at once; without one, or if the refresh fails, the
//! call is taken as under the cap and the cap applies from its response
//! on. Either way a call is checked before it's sent and its cost known
//! only after, so the last call (or calls in flight together) can pass
//! the cap by their own cost.

use std::time::Duration;

use aspero_core::{Error, ProviderId, Response};
use jiff::Timestamp;

use crate::{CallOptions, Engine, Entry, lock};

/// After a refresh that couldn't say the day's use, how long before
/// another is tried.
const REFRESH_AGAIN: Duration = Duration::from_secs(60);

impl Engine {
    /// The daily cap set on `provider`, if any.
    pub fn daily_cap(&self, provider: &ProviderId) -> Option<f64> {
        self.caps
            .iter()
            .find(|(p, _)| p == provider)
            .map(|(_, cap)| *cap)
    }

    /// Runs a call that costs: refused with [`Error::BudgetCapped`] once
    /// the provider's day has reached its cap.
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

    /// `Err(BudgetCapped)` when `entry`'s provider has a cap and its day's
    /// use has reached it; a day of unknown use is refreshed first where
    /// that's free.
    pub(crate) async fn check_cap(&self, entry: &Entry) -> Result<(), Error> {
        let provider = entry.provider.id();
        let Some(cap) = self.daily_cap(&provider) else {
            return Ok(());
        };
        if self.used_today(&provider).is_none()
            && entry.provider.capabilities().budget_status
            && !self.options.offline
        {
            // One refresh at a time; callers waiting on it then see its
            // answer.
            let _one = self.refreshing.lock().await;
            let due = lock(&self.refreshed).is_none_or(|at| at.elapsed() >= REFRESH_AGAIN);
            if self.used_today(&provider).is_none() && due {
                let quick = CallOptions {
                    deadline: Some(Duration::from_secs(5)),
                    max_retries: Some(0),
                    ..self.options
                };
                self.with_options(quick).refresh_budget(entry).await;
                if self.used_today(&provider).is_none() {
                    *lock(&self.refreshed) = Some(tokio::time::Instant::now());
                }
            }
        }
        match self.used_today(&provider) {
            Some((used, resets_at)) if used >= cap => {
                tracing::info!(
                    target: "aspero::budget",
                    provider = %provider, cap, used, "paid call stopped at the daily cap"
                );
                Err(Error::BudgetCapped {
                    cap,
                    used,
                    resets_at,
                })
            }
            _ => Ok(()),
        }
    }

    /// The provider's use today as last reported, and when it resets;
    /// `None` when unknown, or the budget has reset since.
    fn used_today(&self, provider: &ProviderId) -> Option<(f64, Option<Timestamp>)> {
        let now = (self.clock)();
        let (_, budget) = self
            .budgets
            .all()
            .into_iter()
            .find(|(p, _)| p == provider)?;
        if budget.resets_at.is_some_and(|r| now >= r) {
            return None;
        }
        Some((budget.used()?, budget.resets_at))
    }
}
