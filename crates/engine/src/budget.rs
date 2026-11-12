//! Providers' budgets as their last responses reported them, so a front
//! end can show what's left without spending any.

use std::sync::Mutex;

use aspero_core::{Error, ProviderId, Usage};
use jiff::Timestamp;

use crate::lock;

/// What a provider last said was left of its budget.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Budget {
    pub remaining: Option<f64>,
    pub resets_at: Option<Timestamp>,
    /// When the provider said so.
    pub seen_at: Timestamp,
}

#[derive(Debug, Default)]
pub(crate) struct Budgets(Mutex<Vec<(ProviderId, Budget)>>);

impl Budgets {
    /// Remembers what a response (or a spent-budget error) said.
    pub(crate) fn note(
        &self,
        provider: &ProviderId,
        usage: Option<&Usage>,
        error: Option<&Error>,
        now: Timestamp,
    ) {
        let (remaining, resets_at) = match (usage, error) {
            (Some(u), _) => (u.remaining, u.resets_at),
            (None, Some(Error::QuotaExhausted { resets_at })) => (Some(0.0), *resets_at),
            _ => return,
        };
        if remaining.is_none() && resets_at.is_none() {
            return;
        }
        let budget = Budget {
            remaining,
            resets_at,
            seen_at: now,
        };
        let mut all = lock(&self.0);
        match all.iter_mut().find(|(p, _)| p == provider) {
            Some((_, known)) => *known = budget,
            None => all.push((provider.clone(), budget)),
        }
    }

    pub(crate) fn all(&self) -> Vec<(ProviderId, Budget)> {
        lock(&self.0).clone()
    }
}
