//! Per-provider politeness and failure-handling settings, and the pure maths
//! behind them. The engine applies the policy; nothing here sleeps or reads a
//! clock, so it is deterministic to test.

use std::time::Duration;

/// How the engine paces, retries and gives up on one provider. Each provider
/// supplies defaults matching its terms of use; config may override them.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ResiliencePolicy {
    /// Minimum gap between request starts (1 / rate limit).
    pub min_interval: Duration,
    /// Most requests in flight at once from this process; 0 for no limit.
    pub max_in_flight: usize,
    /// Timeout for a single HTTP attempt.
    pub attempt_timeout: Duration,
    /// Retries after the first attempt, for retryable errors only.
    pub max_retries: u32,
    /// First backoff ceiling; doubles per retry.
    pub backoff_base: Duration,
    /// Upper bound on any single backoff sleep.
    pub backoff_cap: Duration,
    /// Overall budget for a request, including all retries and sleeps.
    pub deadline: Duration,
    /// Consecutive breaker-tripping failures before the circuit opens.
    pub breaker_threshold: u32,
    /// How long an open circuit stays open before a single probe.
    pub breaker_cooldown: Duration,
}

impl Default for ResiliencePolicy {
    /// Conservative defaults for an unknown provider: one request per second.
    fn default() -> Self {
        ResiliencePolicy {
            min_interval: Duration::from_secs(1),
            max_in_flight: 0,
            attempt_timeout: Duration::from_secs(20),
            max_retries: 4,
            backoff_base: Duration::from_millis(500),
            backoff_cap: Duration::from_secs(30),
            deadline: Duration::from_secs(90),
            breaker_threshold: 5,
            breaker_cooldown: Duration::from_secs(300),
        }
    }
}

impl ResiliencePolicy {
    /// The ceiling for the sleep before retry number `retry` (0-based):
    /// `min(cap, base * 2^retry)`. Saturates rather than overflowing.
    pub fn backoff_ceiling(&self, retry: u32) -> Duration {
        let factor = 1u32.checked_shl(retry).unwrap_or(u32::MAX);
        self.backoff_base
            .checked_mul(factor)
            .unwrap_or(Duration::MAX)
            .min(self.backoff_cap)
    }

    /// "Full jitter" backoff: a uniformly random point below the ceiling.
    /// `jitter` is a random sample in `[0, 1)`; out-of-range values are
    /// clamped and NaN counts as 0. A server-supplied `Retry-After` overrides
    /// this when longer.
    pub fn backoff(&self, retry: u32, jitter: f64) -> Duration {
        let jitter = if jitter.is_nan() {
            0.0
        } else {
            jitter.clamp(0.0, 1.0)
        };
        self.backoff_ceiling(retry).mul_f64(jitter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> ResiliencePolicy {
        ResiliencePolicy {
            backoff_base: Duration::from_millis(100),
            backoff_cap: Duration::from_secs(5),
            ..ResiliencePolicy::default()
        }
    }

    #[test]
    fn ceiling_doubles_then_caps() {
        let p = policy();
        let ceilings: Vec<_> = (0..8).map(|r| p.backoff_ceiling(r).as_millis()).collect();
        assert_eq!(ceilings, [100, 200, 400, 800, 1600, 3200, 5000, 5000]);
    }

    #[test]
    fn huge_retry_counts_saturate_at_the_cap() {
        let p = policy();
        assert_eq!(p.backoff_ceiling(31), p.backoff_cap);
        assert_eq!(p.backoff_ceiling(u32::MAX), p.backoff_cap);
    }

    #[test]
    fn jitter_scales_within_the_ceiling() {
        let p = policy();
        assert_eq!(p.backoff(3, 0.0), Duration::ZERO);
        assert_eq!(p.backoff(3, 0.5), Duration::from_millis(400));
        assert_eq!(p.backoff(3, 7.0), p.backoff_ceiling(3));
        assert_eq!(p.backoff(3, f64::NAN), Duration::ZERO);
    }
}
