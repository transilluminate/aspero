//! Failures as data: what went wrong (`ErrorKind`), a provider's
//! `Failure`, and providers refusing calls for now (`Unavailable`).

use std::time::SystemTime;

use aspero_engine as engine;

use super::BudgetCap;

/// What went wrong, exactly; new kinds may be added (`Other`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ErrorKind {
    RateLimited,
    /// The provider's usage budget is spent until it resets.
    BudgetSpent,
    Network,
    Timeout,
    Server,
    /// Offline, and not in the cache.
    NotCached,
    /// Skipped for a while after repeated failures.
    CircuitOpen,
    /// The API key was rejected.
    KeyRejected,
    NotFound,
    BadRequest,
    Unsupported,
    /// The answer couldn't be understood.
    Decode,
    InvalidId,
    Other,
    /// The app's daily cap on the provider's spend (`Config.openalexDailyCapUsd`)
    /// is reached until the budget resets: see `Failure.budgetCap`. Last, so
    /// the cases before keep their places.
    BudgetCapped,
}

impl From<aspero_core::ErrorKind> for ErrorKind {
    fn from(kind: aspero_core::ErrorKind) -> Self {
        use aspero_core::ErrorKind as K;
        match kind {
            K::RateLimited => ErrorKind::RateLimited,
            K::BudgetSpent => ErrorKind::BudgetSpent,
            K::BudgetCapped => ErrorKind::BudgetCapped,
            K::Network => ErrorKind::Network,
            K::Timeout => ErrorKind::Timeout,
            K::Server => ErrorKind::Server,
            K::NotCached => ErrorKind::NotCached,
            K::CircuitOpen => ErrorKind::CircuitOpen,
            K::KeyRejected => ErrorKind::KeyRejected,
            K::NotFound => ErrorKind::NotFound,
            K::BadRequest => ErrorKind::BadRequest,
            K::Unsupported => ErrorKind::Unsupported,
            K::Decode => ErrorKind::Decode,
            K::InvalidId => ErrorKind::InvalidId,
            _ => ErrorKind::Other,
        }
    }
}

/// A provider's failure.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Failure {
    pub provider: Option<String>,
    pub kind: ErrorKind,
    /// In English.
    pub message: String,
    /// For `budgetCapped`: the cap, what's used and when it resets, to
    /// word ("Stopped at your $0.50 daily cap; resets at 01:00").
    #[uniffi(default = None)]
    pub budget_cap: Option<BudgetCap>,
}

impl From<&engine::EngineError> for Failure {
    fn from(e: &engine::EngineError) -> Self {
        let (provider, budget_cap) = match e {
            engine::EngineError::Provider { provider, error } => {
                (Some(provider.to_string()), BudgetCap::of(error))
            }
            engine::EngineError::Disabled(provider) => (Some(provider.to_string()), None),
            engine::EngineError::Unresolved {
                provider, error, ..
            } => (
                provider.as_ref().map(ToString::to_string),
                error.as_deref().and_then(BudgetCap::of),
            ),
            _ => (None, None),
        };
        Failure {
            provider,
            kind: e.kind().into(),
            message: e.to_string(),
            budget_cap,
        }
    }
}

/// A provider refusing calls for now.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Unavailable {
    pub provider: String,
    pub kind: ErrorKind,
    pub message: String,
    /// When it will be tried again; never, for a rejected key.
    pub until: Option<SystemTime>,
}
