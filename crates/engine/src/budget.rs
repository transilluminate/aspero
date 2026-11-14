//! Providers' budgets as their last responses reported them, so a front
//! end can show what's left without spending any.

use std::sync::Mutex;

use aspero_core::{Error, ProviderId, Response, Usage};
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
    /// says one.
    pub prepaid_remaining: Option<f64>,
    /// When the provider said so.
    pub seen_at: Timestamp,
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
        }
    }

    /// Spent in the current period (`limit − remaining`, to a millionth),
    /// when both are known.
    pub fn used(&self) -> Option<f64> {
        let used = (self.limit? - self.remaining?).max(0.0);
        Some((used * 1e6).round() / 1e6)
    }
}

#[derive(Debug, Default)]
pub(crate) struct Budgets(Mutex<Vec<(ProviderId, Budget)>>);

impl Budgets {
    /// Remembers what a response (or a spent-budget error) said. What it
    /// leaves out (the limit, a prepaid balance) is kept from before.
    pub(crate) fn note(
        &self,
        provider: &ProviderId,
        usage: Option<&Usage>,
        error: Option<&Error>,
        now: Timestamp,
    ) {
        let (remaining, resets_at, limit, prepaid) = match (usage, error) {
            (Some(u), _) => (u.remaining, u.resets_at, u.limit, u.prepaid_remaining),
            (None, Some(Error::QuotaExhausted { resets_at })) => {
                (Some(0.0), *resets_at, None, None)
            }
            _ => return,
        };
        if remaining.is_none() && resets_at.is_none() && limit.is_none() && prepaid.is_none() {
            return;
        }
        let mut all = lock(&self.0);
        let known = all.iter().find(|(p, _)| p == provider).map(|(_, b)| *b);
        let budget = Budget {
            remaining,
            resets_at,
            limit: limit.or(known.and_then(|k| k.limit)),
            prepaid_remaining: prepaid.or(known.and_then(|k| k.prepaid_remaining)),
            seen_at: now,
        };
        match all.iter_mut().find(|(p, _)| p == provider) {
            Some((_, known)) => *known = budget,
            None => all.push((provider.clone(), budget)),
        }
    }

    pub(crate) fn all(&self) -> Vec<(ProviderId, Budget)> {
        lock(&self.0).clone()
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
