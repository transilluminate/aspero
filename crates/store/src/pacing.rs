//! Request slots per provider, shared between processes using one file.

use std::time::Duration;

use jiff::Timestamp;
use rusqlite::{OptionalExtension, TransactionBehavior, params};

use crate::{Result, SqliteCache};

/// Pacing state, kept apart from the versioned schema: rebuilds keep it,
/// since processes still running depend on it.
pub(crate) const PACING: &str = "
CREATE TABLE IF NOT EXISTS pacing (
    provider   TEXT PRIMARY KEY,
    next_slot  INTEGER NOT NULL    -- unix milliseconds
);
";

/// A claimed slot further ahead than this means the clock went back (or a
/// process misbehaved): start afresh rather than wait.
const MAX_AHEAD: Duration = Duration::from_secs(300);

/// Request slots shared between processes, so parallel agents (and any
/// other process using the same file) keep to one rate limit per provider.
pub trait Pacing: Send + Sync {
    /// Claims the next start time for a request to `provider`: `now` if
    /// free, else `interval` after the latest slot anyone claimed.
    fn claim_slot(&self, provider: &str, interval: Duration, now: Timestamp) -> Result<Timestamp>;
}

impl Pacing for SqliteCache {
    fn claim_slot(&self, provider: &str, interval: Duration, now: Timestamp) -> Result<Timestamp> {
        let mut conn = self.conn();
        // Take the write lock up front, so claims never interleave.
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now_ms = now.as_millisecond();
        let next: Option<i64> = tx
            .query_row(
                "SELECT next_slot FROM pacing WHERE provider = ?1",
                params![provider],
                |r| r.get(0),
            )
            .optional()?;
        let ahead = i64::try_from(MAX_AHEAD.as_millis()).unwrap_or(i64::MAX);
        let slot = next
            .filter(|n| *n - now_ms <= ahead)
            .map_or(now_ms, |n| n.max(now_ms));
        tx.execute(
            "INSERT INTO pacing (provider, next_slot) VALUES (?1, ?2)
             ON CONFLICT (provider) DO UPDATE SET next_slot = excluded.next_slot",
            params![
                provider,
                slot.saturating_add(i64::try_from(interval.as_millis()).unwrap_or(i64::MAX))
            ],
        )?;
        tx.commit()?;
        Ok(Timestamp::from_millisecond(slot).unwrap_or(now))
    }
}
