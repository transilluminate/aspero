//! SQLite storage.
//!
//! [`SqliteCache`] is the **disposable** cache of provider answers: records,
//! "not found" answers, search pages and citation links ([`Edges`]).
//! Everything in it can be fetched again, so it lives in the platform cache
//! directory (which the OS may wipe) and has no migrations: a schema
//! mismatch simply rebuilds it. Tables and columns added beside the
//! versioned schema (`CREATE TABLE IF NOT EXISTS`, as the links are; `ALTER
//! TABLE … ADD COLUMN`, as use tracking is) need no rebuild.
//!
//! Durable data that is costly to recompute (history; derived data such as
//! embeddings or extracted text) will live in a separate store in the data
//! directory, with real migrations (DESIGN "Storage").
//!
//! The cache keeps entries and when they were stored; it applies no policy.
//! Freshness, field coverage and stale-on-error are the engine's decisions.
//! The API is batch-first: lookups of many IDs are one read, and storing
//! many answers is one transaction (one disk sync, not one per ID).
//!
//! SQLite in WAL mode with a busy timeout, so several agent processes can
//! share one cache file. The same file paces them: [`Pacing`] hands out
//! request slots per provider across every process using it.
//!
//! A file a newer version's schema has written (an embedder and the CLI
//! sharing the default path, one upgraded first) isn't dropped: the older
//! binary runs without its answers, so neither wipes the other's cache.
//!
//! A size limit ([`SqliteCache::set_max_bytes`]) evicts the least recently
//! used entries (records, "not found" answers, searches, links; never the
//! pacing slots) once the database passes it; see [`ACCESS`].
//!
//! [`SqliteTape`] is the HTTP tape for benchmark and red-team runs: whole
//! responses recorded once and replayed, kept in a file of its own.

use std::sync::atomic::AtomicU64;
use std::sync::{Mutex, PoisonError};

use jiff::Timestamp;
use rusqlite::Connection;

mod cache;
mod edges;
mod evict;
mod pacing;
mod prepaid;
mod schema;
mod spend;
mod tape;

pub use cache::{Cache, CachedLookup, Entry};
pub use edges::{Direction, Edges};
pub use evict::ACCESS;
pub use pacing::Pacing;
pub use prepaid::{PrepaidDay, PrepaidDays};
pub use tape::{SqliteTape, TapeKey};

use evict::size;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    #[error("cache database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("cache entry is corrupt: {0}")]
    Corrupt(#[from] serde_json::Error),
    #[error("could not create cache directory: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T, E = StoreError> = std::result::Result<T, E>;

/// `mb` megabytes (1,000,000 bytes, as size limits are given) in bytes,
/// saturating.
pub fn megabytes(mb: u64) -> u64 {
    mb.saturating_mul(1_000_000)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Stats {
    pub records: u64,
    pub identifiers: u64,
    pub misses: u64,
    pub searches: u64,
    /// Works with cached citation links (per direction).
    pub edges: u64,
    /// The database's size, `page_count × page_size`: the file on disk
    /// once its write-ahead log is checkpointed (SQLite does so every 4 MB
    /// or so). A size limit applies to this.
    pub bytes: u64,
}

pub struct SqliteCache {
    conn: Mutex<Connection>,
    /// The size limit in bytes; `u64::MAX` for none.
    max_bytes: AtomicU64,
    /// Whether the file's answers are a newer version's schema: then they
    /// are neither read, written nor evicted, and this process runs as if
    /// no cache were configured, the file's ledgers (pacing, spend)
    /// shared as ever ([`SqliteCache::answers_unused`]).
    newer: bool,
}

impl SqliteCache {
    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves SQLite consistent (writes
        // are transactional), so poisoning is ignorable.
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn stats(&self) -> Result<Stats> {
        let conn = self.conn();
        let count = |table: &str| -> Result<u64> {
            let n: i64 =
                conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
            // A count: never negative.
            Ok(n.unsigned_abs())
        };
        Ok(Stats {
            records: count("record")?,
            identifiers: count("record_id")?,
            misses: count("miss")?,
            searches: count("search")?,
            edges: count("edges")?,
            bytes: size(&conn)?.0,
        })
    }

    /// Empties the cache.
    pub fn clear(&self) -> Result<()> {
        self.conn().execute_batch(
            "DELETE FROM record; DELETE FROM record_id; DELETE FROM miss; DELETE FROM search; \
             DELETE FROM edges; VACUUM;",
        )?;
        Ok(())
    }
}

fn timestamp(seconds: i64) -> Timestamp {
    Timestamp::from_second(seconds).unwrap_or(Timestamp::UNIX_EPOCH)
}
