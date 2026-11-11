//! The error contract between providers, the engine and front ends.
//!
//! Variants are grouped by what the caller should *do*, not by where the
//! failure happened. Providers map their HTTP/transport failures into these;
//! the engine decides on retry, fallback or giving up from [`Error::is_retryable`]
//! and [`Error::category`]; the CLI maps [`ErrorCategory`] to exit codes.
//!
//! `Error` is `Clone` (sources are behind `Arc`) so one in-flight request's
//! result can be shared with every caller that was waiting on it.

use std::sync::Arc;
use std::time::Duration;

use jiff::Timestamp;

use crate::id::IdError;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// A boxed, shareable underlying cause.
pub type Source = Arc<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Too many requests right now; back off and retry.
    #[error("rate limited{}", retry_hint(.retry_after))]
    RateLimited { retry_after: Option<Duration> },

    /// The provider's usage budget (e.g. OpenAlex daily credits) is spent.
    /// Not retryable until `resets_at`; fall back to other providers.
    #[error("usage quota exhausted{}", reset_hint(.resets_at))]
    QuotaExhausted { resets_at: Option<Timestamp> },

    #[error("network error: {0}")]
    Network(#[source] Source),

    #[error("request timed out")]
    Timeout,

    #[error("server error (HTTP {status}){}", message_hint(.message))]
    Server {
        status: u16,
        message: Option<String>,
    },

    /// The caller asked for cached answers only, and there was none.
    #[error("offline, and not in the cache")]
    Offline,

    /// The engine's circuit breaker for this provider is open.
    #[error("provider temporarily disabled after repeated failures")]
    CircuitOpen { until: Option<Timestamp> },

    /// Credentials missing, invalid or not yet activated. Retrying won't help.
    #[error("authentication rejected{}", message_hint(.message))]
    AuthRejected { message: Option<String> },

    /// The addressed entity doesn't exist at this provider. Batch lookups
    /// report per-item absence in their result instead.
    #[error("not found")]
    NotFound,

    /// The provider rejected the request as malformed.
    #[error("bad request: {0}")]
    BadRequest(String),

    /// The provider doesn't offer this operation or identifier kind.
    #[error("unsupported: {0}")]
    Unsupported(&'static str),

    /// The response arrived but couldn't be understood.
    #[error("could not decode response: {0}")]
    Decode(#[source] Source),

    #[error(transparent)]
    InvalidId(#[from] IdError),
}

/// What a caller should do about an error. Stable: the CLI derives exit
/// codes from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorCategory {
    /// Transient; retrying later may succeed.
    Unavailable,
    /// Out of budget or rate; wait or use another provider.
    Throttled,
    /// Credentials need fixing.
    Auth,
    NotFound,
    /// The caller's input or request was wrong.
    Usage,
    /// The provider can't do this.
    Unsupported,
    /// The provider sent something unexpected; likely an aspero bug or an
    /// upstream API change.
    Protocol,
}

/// Exactly what went wrong, for front ends that word or localise it
/// themselves. Finer than [`ErrorCategory`]; new kinds may be added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    RateLimited,
    /// The provider's usage budget is spent until it resets.
    BudgetSpent,
    Network,
    Timeout,
    /// The provider answered with a server error.
    Server,
    /// Offline, and not in the cache.
    NotCached,
    /// Skipped after repeated failures, for a cooldown.
    CircuitOpen,
    /// The API key (or other credential) was rejected.
    KeyRejected,
    NotFound,
    BadRequest,
    Unsupported,
    /// The answer couldn't be understood.
    Decode,
    InvalidId,
}

impl ErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::RateLimited => "rate_limited",
            ErrorKind::BudgetSpent => "budget_spent",
            ErrorKind::Network => "network",
            ErrorKind::Timeout => "timeout",
            ErrorKind::Server => "server",
            ErrorKind::NotCached => "not_cached",
            ErrorKind::CircuitOpen => "circuit_open",
            ErrorKind::KeyRejected => "key_rejected",
            ErrorKind::NotFound => "not_found",
            ErrorKind::BadRequest => "bad_request",
            ErrorKind::Unsupported => "unsupported",
            ErrorKind::Decode => "decode",
            ErrorKind::InvalidId => "invalid_id",
        }
    }
}

impl Error {
    pub fn network(e: impl std::error::Error + Send + Sync + 'static) -> Self {
        Error::Network(Arc::new(e))
    }

    pub fn decode(e: impl std::error::Error + Send + Sync + 'static) -> Self {
        Error::Decode(Arc::new(e))
    }

    /// Whether retrying the *same* request against the *same* provider, after
    /// backoff, could plausibly succeed.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Error::RateLimited { .. } | Error::Network(_) | Error::Timeout | Error::Server { .. }
        )
    }

    /// Whether this failure counts towards opening the circuit breaker.
    /// Rate limiting is handled by backoff and does not count.
    pub fn trips_breaker(&self) -> bool {
        matches!(
            self,
            Error::Network(_) | Error::Timeout | Error::Server { .. }
        )
    }

    pub fn kind(&self) -> ErrorKind {
        match self {
            Error::RateLimited { .. } => ErrorKind::RateLimited,
            Error::QuotaExhausted { .. } => ErrorKind::BudgetSpent,
            Error::Network(_) => ErrorKind::Network,
            Error::Timeout => ErrorKind::Timeout,
            Error::Server { .. } => ErrorKind::Server,
            Error::Offline => ErrorKind::NotCached,
            Error::CircuitOpen { .. } => ErrorKind::CircuitOpen,
            Error::AuthRejected { .. } => ErrorKind::KeyRejected,
            Error::NotFound => ErrorKind::NotFound,
            Error::BadRequest(_) => ErrorKind::BadRequest,
            Error::Unsupported(_) => ErrorKind::Unsupported,
            Error::Decode(_) => ErrorKind::Decode,
            Error::InvalidId(_) => ErrorKind::InvalidId,
        }
    }

    pub fn category(&self) -> ErrorCategory {
        match self {
            Error::Network(_)
            | Error::Timeout
            | Error::Server { .. }
            | Error::CircuitOpen { .. }
            | Error::Offline => ErrorCategory::Unavailable,
            Error::RateLimited { .. } | Error::QuotaExhausted { .. } => ErrorCategory::Throttled,
            Error::AuthRejected { .. } => ErrorCategory::Auth,
            Error::NotFound => ErrorCategory::NotFound,
            Error::BadRequest(_) | Error::InvalidId(_) => ErrorCategory::Usage,
            Error::Unsupported(_) => ErrorCategory::Unsupported,
            Error::Decode(_) => ErrorCategory::Protocol,
        }
    }
}

fn retry_hint(d: &Option<Duration>) -> String {
    d.map(|d| format!(" (retry after {}s)", d.as_secs()))
        .unwrap_or_default()
}

fn reset_hint(t: &Option<Timestamp>) -> String {
    t.map(|t| format!(" (resets at {t})")).unwrap_or_default()
}

fn message_hint(m: &Option<String>) -> String {
    m.as_ref().map(|m| format!(": {m}")).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limits_retry_but_do_not_trip_the_breaker() {
        let e = Error::RateLimited { retry_after: None };
        assert!(e.is_retryable());
        assert!(!e.trips_breaker());
        assert_eq!(e.category(), ErrorCategory::Throttled);
    }

    #[test]
    fn quota_and_auth_are_not_retryable() {
        assert!(!Error::QuotaExhausted { resets_at: None }.is_retryable());
        assert!(!Error::AuthRejected { message: None }.is_retryable());
    }

    #[test]
    fn server_errors_retry_and_trip() {
        let e = Error::Server {
            status: 503,
            message: Some("Service Unavailable".into()),
        };
        assert!(e.is_retryable() && e.trips_breaker());
        assert_eq!(
            e.to_string(),
            "server error (HTTP 503): Service Unavailable"
        );
    }

    #[test]
    fn errors_are_cloneable_with_sources() {
        let e = Error::network(std::io::Error::other("reset"));
        let shared = e.clone();
        assert_eq!(shared.to_string(), "network error: reset");
        assert!(std::error::Error::source(&shared).is_some());
    }
}
