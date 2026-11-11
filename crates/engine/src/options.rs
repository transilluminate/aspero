//! Per-call options: how patient one caller is, independent of providers'
//! own policies.

use std::time::Duration;

use aspero_core::ResiliencePolicy;

/// How patient a caller is. The default is a batch caller's: providers'
/// own policies, retries and all. Set on an engine view with
/// [`Engine::with_options`](crate::Engine::with_options); views share the
/// cache, pacing and requests in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct CallOptions {
    /// Caps each provider's overall deadline, retries included.
    pub deadline: Option<Duration>,
    /// Caps each provider's retries.
    pub max_retries: Option<u32>,
    /// Answers from the cache only, however old; what it lacks fails as
    /// [`Error::Offline`](aspero_core::Error::Offline).
    pub offline: bool,
}

impl CallOptions {
    /// Someone is waiting: give up on a provider within a few seconds and
    /// fall back (to the next provider, or a stale cached answer).
    pub fn interactive() -> Self {
        CallOptions {
            deadline: Some(Duration::from_secs(8)),
            max_retries: Some(1),
            offline: false,
        }
    }

    /// Answers from the cache only.
    pub fn offline() -> Self {
        CallOptions {
            offline: true,
            ..CallOptions::default()
        }
    }

    /// `policy`, capped by these options.
    pub(crate) fn apply(&self, policy: &ResiliencePolicy) -> ResiliencePolicy {
        let mut p = policy.clone();
        if let Some(d) = self.deadline {
            p.deadline = p.deadline.min(d);
            p.attempt_timeout = p.attempt_timeout.min(d);
        }
        if let Some(r) = self.max_retries {
            p.max_retries = p.max_retries.min(r);
        }
        p
    }
}
