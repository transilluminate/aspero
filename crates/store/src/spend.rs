//! Agents' spend per MCP key and UTC day.

use rusqlite::{OptionalExtension, TransactionBehavior, params};

use crate::{Result, SqliteCache};

/// Agents' spend per MCP key and UTC day, kept apart from the versioned
/// schema like the pacing slots: a rebuild keeps it, and `clear` and
/// eviction leave it, since it's a budget's accounting, not an answer to
/// fetch again. Days more than [`SPEND_DAYS`] old are pruned as spend is
/// added.
pub(crate) const SPEND: &str = "
CREATE TABLE IF NOT EXISTS mcp_spend (
    key_id     TEXT NOT NULL,      -- the host's id for the key
    day        TEXT NOT NULL,      -- UTC date, YYYY-MM-DD
    usd        REAL NOT NULL,
    PRIMARY KEY (key_id, day)
);
";

/// How many days of per-key spend are kept.
const SPEND_DAYS: i64 = 31;

impl SqliteCache {
    /// What the MCP key `key_id` has spent on `day` (a UTC date,
    /// `YYYY-MM-DD`), in USD: 0 if nothing.
    pub fn key_spend(&self, key_id: &str, day: &str) -> Result<f64> {
        let spent: Option<f64> = self
            .conn()
            .query_row(
                "SELECT usd FROM mcp_spend WHERE key_id = ?1 AND day = ?2",
                params![key_id, day],
                |r| r.get(0),
            )
            .optional()?;
        Ok(spent.unwrap_or(0.0))
    }

    /// Adds `usd` to what `key_id` has spent on `day`, in one transaction
    /// (processes may share the file), and forgets days more than a month
    /// before it.
    pub fn add_key_spend(&self, key_id: &str, day: &str, usd: f64) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO mcp_spend (key_id, day, usd) VALUES (?1, ?2, ?3)
             ON CONFLICT (key_id, day) DO UPDATE SET usd = usd + excluded.usd",
            params![key_id, day, usd],
        )?;
        tx.execute(
            "DELETE FROM mcp_spend WHERE day < date(?1, ?2)",
            params![day, format!("-{SPEND_DAYS} days")],
        )?;
        tx.commit()?;
        Ok(())
    }
}
