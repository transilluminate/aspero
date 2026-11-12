//! What the engine remembers about each provider between calls, so a
//! long-lived engine stops asking providers that can't answer:
//!
//! - a spent quota, until it resets;
//! - rejected credentials, for the engine's life (new credentials mean a
//!   new engine);
//! - a circuit breaker: after `breaker_threshold` consecutive failing
//!   calls the provider is skipped for `breaker_cooldown`, then one probe
//!   call decides whether to close it again or wait another cooldown.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aspero_core::{Error, ProviderId, ResiliencePolicy};
use jiff::Timestamp;
use tokio::time::Instant;

use crate::lock;

/// How long to wait out a spent quota whose reset time is unknown.
const UNKNOWN_RESET: Duration = Duration::from_secs(3600);

/// Stands in for a cooldown too long to add to an `Instant`: about 30 years.
const FAR_FUTURE: Duration = Duration::from_secs(30 * 365 * 86_400);

#[derive(Default)]
pub(crate) struct Health {
    providers: Mutex<HashMap<ProviderId, State>>,
}

#[derive(Default)]
struct State {
    /// Consecutive calls that failed in a way that trips the breaker.
    failures: u32,
    open_until: Option<Instant>,
    probing: bool,
    /// Refusing calls: the error to answer with, and until when (`None`:
    /// for good).
    blocked: Option<(Error, Option<Instant>)>,
}

/// Permission for one call; report how it went with [`Pass::record`].
pub(crate) struct Pass {
    health: Arc<Health>,
    provider: ProviderId,
    probe: bool,
    recorded: bool,
}

impl Health {
    /// Whether `provider` may be called now, or the error to answer with.
    pub(crate) fn admit(
        self: &Arc<Self>,
        provider: &ProviderId,
        now: Timestamp,
    ) -> Result<Pass, Error> {
        let mut providers = lock(&self.providers);
        let state = providers.entry(provider.clone()).or_default();
        let at = Instant::now();
        if let Some((error, until)) = &state.blocked {
            match until {
                Some(until) if at >= *until => state.blocked = None,
                _ => return Err(error.clone()),
            }
        }
        let mut probe = false;
        if let Some(until) = state.open_until {
            if at < until || state.probing {
                return Err(Error::CircuitOpen {
                    until: Some(wall(now, until.saturating_duration_since(at))),
                });
            }
            state.probing = true;
            probe = true;
        }
        drop(providers);
        Ok(Pass {
            health: self.clone(),
            provider: provider.clone(),
            probe,
            recorded: false,
        })
    }

    /// Providers refusing calls now, and why.
    pub(crate) fn unavailable(&self, now: Timestamp) -> Vec<(ProviderId, Error)> {
        let providers = lock(&self.providers);
        let at = Instant::now();
        let mut out: Vec<(ProviderId, Error)> = providers
            .iter()
            .filter_map(|(p, s)| {
                if let Some((error, until)) = &s.blocked
                    && until.is_none_or(|u| at < u)
                {
                    return Some((p.clone(), error.clone()));
                }
                let until = s.open_until.filter(|u| at < *u)?;
                let until = Some(wall(now, until - at));
                Some((p.clone(), Error::CircuitOpen { until }))
            })
            .collect();
        drop(providers);
        out.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
        out
    }
}

impl Pass {
    /// Records the call's outcome under `policy`'s breaker settings.
    pub(crate) fn record(
        mut self,
        policy: &ResiliencePolicy,
        error: Option<&Error>,
        now: Timestamp,
    ) {
        self.recorded = true;
        let mut providers = lock(&self.health.providers);
        let state = providers.entry(self.provider.clone()).or_default();
        if self.probe {
            state.probing = false;
        }
        let at = Instant::now();
        match error {
            Some(e @ Error::AuthRejected { .. }) => state.blocked = Some((e.clone(), None)),
            Some(e @ Error::QuotaExhausted { resets_at }) => {
                let wait = resets_at
                    .and_then(|r| Duration::try_from(r.duration_since(now)).ok())
                    .unwrap_or(UNKNOWN_RESET);
                // A reset too far off to represent blocks for good.
                state.blocked = Some((e.clone(), at.checked_add(wait)));
            }
            Some(e) if e.trips_breaker() => {
                state.failures += 1;
                if self.probe || state.failures >= policy.breaker_threshold {
                    let until = at.checked_add(policy.breaker_cooldown);
                    state.open_until = Some(until.unwrap_or(at + FAR_FUTURE));
                }
            }
            // Answered, even if not as hoped: the provider is up.
            _ => {
                state.failures = 0;
                state.open_until = None;
            }
        }
    }
}

impl Drop for Pass {
    /// A probe abandoned before it finished (its caller cancelled) frees
    /// the way for another.
    fn drop(&mut self) {
        if self.probe && !self.recorded {
            let mut providers = lock(&self.health.providers);
            if let Some(state) = providers.get_mut(&self.provider) {
                state.probing = false;
            }
        }
    }
}

/// Wall-clock time `after` from `now`.
fn wall(now: Timestamp, after: Duration) -> Timestamp {
    jiff::SignedDuration::try_from(after)
        .ok()
        .and_then(|d| now.checked_add(d).ok())
        .unwrap_or(now)
}
