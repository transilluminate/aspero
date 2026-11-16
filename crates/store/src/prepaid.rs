//! Providers' prepaid balances by budget day, from which the credit spent
//! that day by everything using the key is told; and the credit charged to
//! calls made through engines sharing the store (the cache file), by
//! budget day: spent "here".

use jiff::Timestamp;
use rusqlite::{OptionalExtension, TransactionBehavior, params};

use crate::{Result, SqliteCache};

/// Each provider's prepaid balance over a budget day, from which the
/// credit spent that day is told ([`PrepaidDay`]): kept apart from the
/// versioned schema like the MCP spend, and for the same reason. Days more
/// than [`PREPAID_DAYS`] old are pruned as balances are seen.
pub(crate) const PREPAID: &str = "
CREATE TABLE IF NOT EXISTS prepaid_day (
    provider   TEXT NOT NULL,
    day        TEXT NOT NULL,      -- the budget day, YYYY-MM-DD (OpenAlex: UTC)
    start_usd  REAL NOT NULL,      -- the first balance seen, raised by top-ups
    last_usd   REAL NOT NULL,      -- the balance now: the lowest seen since the provider last said it
    seen_at    INTEGER NOT NULL,   -- when the provider last said it (or the first sighting), unix ms
    bought_usd REAL,               -- the credit bought, as the provider last said it
    PRIMARY KEY (provider, day)
);
";

/// The prepaid credit charged to calls made through engines sharing this
/// store (processes using the cache file), by budget day: what a daily
/// credit cap counts, since other tools sharing the key (one key per
/// OpenAlex account) spend credit of their own. Kept like [`PREPAID`],
/// apart from the versioned schema. Days more than [`PREPAID_DAYS`] old
/// are pruned as credit is added.
pub(crate) const CREDIT_HERE: &str = "
CREATE TABLE IF NOT EXISTS credit_here (
    provider   TEXT NOT NULL,
    day        TEXT NOT NULL,      -- the budget day, YYYY-MM-DD (OpenAlex: UTC)
    usd        REAL NOT NULL,      -- credit charged to calls made here that day
    PRIMARY KEY (provider, day)
);
";

/// How many days of prepaid balances, and of credit spent here, are kept.
const PREPAID_DAYS: i64 = 7;

/// A provider's prepaid balance over one budget day. Credit spent that day
/// is `start_usd − last_usd`.
///
/// Balances come two ways. Every response's headers say one, but not
/// reliably: OpenAlex answers from many servers, some minutes behind, so
/// requests made together say balances up to $0.30 apart (seen
/// 2026-11-16), and the stale ones are higher. So a header's balance only
/// ever lowers `last_usd`: the lowest seen is the nearest the truth. Only
/// the provider's own word on its budget (OpenAlex's `/rate-limit`, which
/// also says the credit bought) sets the balance outright, either way, and
/// only a rise in the credit bought is a top-up, raising `start_usd` by as
/// much, so a top-up is never spend, and a stale balance is never a top-up.
///
/// Undercounts, accepted: a day first seen part-way through misses what
/// was spent before; a first sighting from a stale server starts the day
/// high; and `/rate-limit` itself answered by one sets the balance high
/// until lower ones are seen.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PrepaidDay {
    pub start_usd: f64,
    pub last_usd: f64,
    /// When the provider last said the balance itself (or, until it has,
    /// the first sighting).
    pub seen_at: Timestamp,
    /// The credit bought, as the provider last said it.
    pub bought_usd: Option<f64>,
}

impl PrepaidDay {
    /// The day as it stands after `balance` is seen at `at`: by a
    /// response's headers (`bought` `None`), or by the provider's own word
    /// on its budget, with the credit bought (`bought`). See the type's
    /// docs. The provider's word older than the last one changes nothing.
    pub fn see(
        known: Option<PrepaidDay>,
        balance: f64,
        bought: Option<f64>,
        at: Timestamp,
    ) -> PrepaidDay {
        let Some(mut day) = known else {
            return PrepaidDay {
                start_usd: balance,
                last_usd: balance,
                seen_at: at,
                bought_usd: bought,
            };
        };
        let Some(bought) = bought else {
            day.last_usd = day.last_usd.min(balance);
            return day;
        };
        if at < day.seen_at {
            return day;
        }
        if let Some(before) = day.bought_usd
            && bought > before
        {
            day.start_usd += bought - before;
        }
        day.last_usd = balance;
        day.seen_at = at;
        day.bought_usd = Some(bought);
        day
    }

    /// Credit spent over the day, to a millionth; never negative.
    pub fn spent_usd(&self) -> f64 {
        ((self.start_usd - self.last_usd).max(0.0) * 1e6).round() / 1e6
    }
}

/// Providers' prepaid balances by budget day, shared between processes
/// (the cache file), so every process and restart tells the same credit
/// spent today; and the credit charged to calls made through engines
/// sharing it, by budget day, which a daily credit cap counts.
pub trait PrepaidDays: Send + Sync {
    /// Notes `balance`, seen at `at` (with the credit `bought` where the
    /// provider said it), for `provider` on `day` (`YYYY-MM-DD`), as
    /// [`PrepaidDay::see`] does, and returns the day.
    fn see_prepaid(
        &self,
        provider: &str,
        day: &str,
        balance: f64,
        bought: Option<f64>,
        at: Timestamp,
    ) -> Result<PrepaidDay>;

    /// Adds `usd` of credit charged to a call made here to `provider`'s
    /// `day` (`YYYY-MM-DD`), and returns the day's total, to a millionth.
    fn add_credit_here(&self, provider: &str, day: &str, usd: f64) -> Result<f64>;

    /// The credit charged to calls made here on `provider`'s `day`, to a
    /// millionth: 0 if none.
    fn credit_here(&self, provider: &str, day: &str) -> Result<f64>;
}

impl PrepaidDays for SqliteCache {
    /// In one write transaction (processes may share the file), forgetting
    /// days more than a week before `day`.
    fn see_prepaid(
        &self,
        provider: &str,
        day: &str,
        balance: f64,
        bought: Option<f64>,
        at: Timestamp,
    ) -> Result<PrepaidDay> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let known = tx
            .query_row(
                "SELECT start_usd, last_usd, seen_at, bought_usd FROM prepaid_day \
                 WHERE provider = ?1 AND day = ?2",
                params![provider, day],
                |r| {
                    Ok(PrepaidDay {
                        start_usd: r.get(0)?,
                        last_usd: r.get(1)?,
                        seen_at: Timestamp::from_millisecond(r.get(2)?)
                            .unwrap_or(Timestamp::UNIX_EPOCH),
                        bought_usd: r.get(3)?,
                    })
                },
            )
            .optional()?;
        let seen = PrepaidDay::see(known, balance, bought, at);
        if Some(seen) != known {
            tx.execute(
                "INSERT INTO prepaid_day (provider, day, start_usd, last_usd, seen_at, bought_usd)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (provider, day) DO UPDATE SET start_usd = excluded.start_usd,
                     last_usd = excluded.last_usd, seen_at = excluded.seen_at,
                     bought_usd = excluded.bought_usd",
                params![
                    provider,
                    day,
                    seen.start_usd,
                    seen.last_usd,
                    seen.seen_at.as_millisecond(),
                    seen.bought_usd
                ],
            )?;
        }
        if known.is_none() {
            tx.execute(
                "DELETE FROM prepaid_day WHERE day < date(?1, ?2)",
                params![day, format!("-{PREPAID_DAYS} days")],
            )?;
        }
        tx.commit()?;
        Ok(seen)
    }

    /// In one write transaction (processes may share the file), forgetting
    /// days more than a week before `day`.
    fn add_credit_here(&self, provider: &str, day: &str, usd: f64) -> Result<f64> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO credit_here (provider, day, usd) VALUES (?1, ?2, ?3)
             ON CONFLICT (provider, day) DO UPDATE SET usd = usd + excluded.usd",
            params![provider, day, usd],
        )?;
        let total: f64 = tx.query_row(
            "SELECT usd FROM credit_here WHERE provider = ?1 AND day = ?2",
            params![provider, day],
            |r| r.get(0),
        )?;
        tx.execute(
            "DELETE FROM credit_here WHERE day < date(?1, ?2)",
            params![day, format!("-{PREPAID_DAYS} days")],
        )?;
        tx.commit()?;
        Ok(millionths(total))
    }

    fn credit_here(&self, provider: &str, day: &str) -> Result<f64> {
        let total: Option<f64> = self
            .conn()
            .query_row(
                "SELECT usd FROM credit_here WHERE provider = ?1 AND day = ?2",
                params![provider, day],
                |r| r.get(0),
            )
            .optional()?;
        Ok(millionths(total.unwrap_or(0.0)))
    }
}

/// `usd` to a millionth, as the engine counts it.
fn millionths(usd: f64) -> f64 {
    (usd * 1e6).round() / 1e6
}
