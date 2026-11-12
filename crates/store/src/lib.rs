//! SQLite storage.
//!
//! [`SqliteCache`] is the **disposable** cache of provider answers: records,
//! "not found" answers and search pages. Everything in it can be fetched
//! again, so it lives in the platform cache directory (which the OS may
//! wipe) and has no migrations: a schema mismatch simply rebuilds it.
//!
//! Durable data that is costly to recompute (history; derived data such as
//! embeddings or extracted text) will live in a separate store in the data
//! directory, with real migrations (DESIGN §5).
//!
//! The cache keeps entries and when they were stored; it applies no policy.
//! Freshness, field coverage and stale-on-error are the engine's decisions.
//! The API is batch-first: lookups of many IDs are one read, and storing
//! many answers is one transaction (one disk sync, not one per ID).
//!
//! SQLite in WAL mode with a busy timeout, so several agent processes can
//! share one cache file. The same file paces them: [`Pacing`] hands out
//! request slots per provider across every process using it.

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use aspero_core::{Page, ProviderId, SearchQuery, SourceRecord, WorkId};
use jiff::Timestamp;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

/// Bump on any schema change, *and* when cached records' shape or mapping
/// changes (records are stored as mapped); older caches are dropped and
/// rebuilt. 2: records gained `updates` (retractions). 3: `other_years`.
/// 4: Crossref and Europe PMC map print/online years into `other_years`.
/// 5: `update_to` (what a notice updates). 6: `open_access`. 7: Crossref
/// relations map to `new_version` updates. 8: notices are typed as such.
const SCHEMA_VERSION: i64 = 8;

const SCHEMA: &str = "
CREATE TABLE record (
    provider   TEXT NOT NULL,
    native_id  TEXT NOT NULL,
    record     TEXT NOT NULL,      -- SourceRecord as JSON, raw response included
    stored_at  INTEGER NOT NULL,   -- unix seconds
    PRIMARY KEY (provider, native_id)
);
-- Every identifier (asserted or requested) that leads to a record.
CREATE TABLE record_id (
    provider   TEXT NOT NULL,
    work_id    TEXT NOT NULL,
    native_id  TEXT NOT NULL,
    PRIMARY KEY (provider, work_id)
);
CREATE TABLE miss (
    provider   TEXT NOT NULL,
    work_id    TEXT NOT NULL,
    stored_at  INTEGER NOT NULL,
    PRIMARY KEY (provider, work_id)
);
CREATE TABLE search (
    provider   TEXT NOT NULL,
    query      TEXT NOT NULL,      -- SearchQuery as JSON
    page       TEXT NOT NULL,      -- Page as JSON
    stored_at  INTEGER NOT NULL,
    PRIMARY KEY (provider, query)
);
";

/// Pacing state, kept apart from the versioned schema: rebuilds keep it,
/// since processes still running depend on it.
const PACING: &str = "
CREATE TABLE IF NOT EXISTS pacing (
    provider   TEXT PRIMARY KEY,
    next_slot  INTEGER NOT NULL    -- unix milliseconds
);
";

/// A claimed slot further ahead than this means the clock went back (or a
/// process misbehaved): start afresh rather than wait.
const MAX_AHEAD: Duration = Duration::from_secs(300);

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

/// A stored value and when it was stored.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry<T> {
    pub value: T,
    pub stored_at: Timestamp,
}

impl<T> Entry<T> {
    /// Whether the entry is at most `ttl` old at `now`.
    pub fn is_fresh(&self, now: Timestamp, ttl: Duration) -> bool {
        now.duration_since(self.stored_at).unsigned_abs() <= ttl
    }
}

/// A provider's cached answer for one ID: `Some(record)`, or `None` for a
/// cached "not found".
pub type CachedLookup = Entry<Option<SourceRecord>>;

/// What the engine needs from a cache; it depends on this, not on SQLite.
pub trait Cache: Send + Sync {
    /// The cached answer for each of `ids` at `provider`, in order; `None`
    /// where nothing is cached.
    fn get_lookups(
        &self,
        provider: &ProviderId,
        ids: &[WorkId],
    ) -> Result<Vec<Option<CachedLookup>>>;

    /// Stores `provider`'s answers in one transaction. A found record is
    /// also reachable by every identifier it asserts.
    fn put_lookups(
        &self,
        provider: &ProviderId,
        answers: &[(&WorkId, Option<&SourceRecord>)],
        at: Timestamp,
    ) -> Result<()>;

    fn get_search(&self, provider: &ProviderId, query: &SearchQuery)
    -> Result<Option<Entry<Page>>>;

    fn put_search(
        &self,
        provider: &ProviderId,
        query: &SearchQuery,
        page: &Page,
        at: Timestamp,
    ) -> Result<()>;
}

/// Request slots shared between processes, so parallel agents (and any
/// other process using the same file) keep to one rate limit per provider.
pub trait Pacing: Send + Sync {
    /// Claims the next start time for a request to `provider`: `now` if
    /// free, else `interval` after the latest slot anyone claimed.
    fn claim_slot(&self, provider: &str, interval: Duration, now: Timestamp) -> Result<Timestamp>;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Stats {
    pub records: u64,
    pub identifiers: u64,
    pub misses: u64,
    pub searches: u64,
}

pub struct SqliteCache {
    conn: Mutex<Connection>,
}

impl SqliteCache {
    /// Opens (creating if needed) the cache at `path`. A file that isn't a
    /// readable SQLite database is deleted and started afresh.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        match Connection::open(path)
            .map_err(StoreError::from)
            .and_then(Self::init)
        {
            Err(StoreError::Sqlite(e)) if is_unreadable(&e) => {
                for suffix in ["", "-wal", "-shm"] {
                    let mut file = path.as_os_str().to_owned();
                    file.push(suffix);
                    match std::fs::remove_file(&file) {
                        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                        _ => {}
                    }
                }
                Self::init(Connection::open(path)?)
            }
            opened => opened,
        }
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let version = |c: &Connection| -> rusqlite::Result<i64> {
            c.pragma_query_value(None, "user_version", |r| r.get(0))
        };
        if version(&conn)? != SCHEMA_VERSION {
            // Take the write lock, then check again: another process may
            // have rebuilt it meanwhile.
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if version(&tx)? != SCHEMA_VERSION {
                // Disposable: drop whatever is there (older or newer) and rebuild.
                let tables: Vec<String> = tx
                    .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name != 'pacing'")?
                    .query_map([], |r| r.get(0))?
                    .collect::<rusqlite::Result<_>>()?;
                for table in tables {
                    tx.execute_batch(&format!("DROP TABLE \"{}\";", table.replace('"', "\"\"")))?;
                }
                tx.execute_batch(SCHEMA)?;
                tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            }
            tx.commit()?;
        }
        conn.execute_batch(PACING)?;
        Ok(SqliteCache {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves SQLite consistent (writes
        // are transactional), so poisoning is ignorable.
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn stats(&self) -> Result<Stats> {
        let conn = self.conn();
        let count = |table: &str| -> Result<u64> {
            let n: i64 =
                conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
            Ok(n as u64)
        };
        Ok(Stats {
            records: count("record")?,
            identifiers: count("record_id")?,
            misses: count("miss")?,
            searches: count("search")?,
        })
    }

    /// Empties the cache.
    pub fn clear(&self) -> Result<()> {
        self.conn().execute_batch(
            "DELETE FROM record; DELETE FROM record_id; DELETE FROM miss; DELETE FROM search; VACUUM;",
        )?;
        Ok(())
    }
}

/// Whether `e` says the file isn't a database, or is a damaged one.
fn is_unreadable(e: &rusqlite::Error) -> bool {
    matches!(
        e.sqlite_error_code(),
        Some(rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt)
    )
}

fn timestamp(seconds: i64) -> Timestamp {
    Timestamp::from_second(seconds).unwrap_or(Timestamp::UNIX_EPOCH)
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
        let ahead = MAX_AHEAD.as_millis() as i64;
        let slot = next
            .filter(|n| *n - now_ms <= ahead)
            .map_or(now_ms, |n| n.max(now_ms));
        tx.execute(
            "INSERT INTO pacing (provider, next_slot) VALUES (?1, ?2)
             ON CONFLICT (provider) DO UPDATE SET next_slot = excluded.next_slot",
            params![provider, slot + interval.as_millis() as i64],
        )?;
        tx.commit()?;
        Ok(Timestamp::from_millisecond(slot).unwrap_or(now))
    }
}

impl Cache for SqliteCache {
    fn get_lookups(
        &self,
        provider: &ProviderId,
        ids: &[WorkId],
    ) -> Result<Vec<Option<CachedLookup>>> {
        let mut conn = self.conn();
        // One read transaction: a consistent snapshot, one lock.
        let tx = conn.transaction()?;
        let mut found = tx.prepare_cached(
            "SELECT r.record, r.stored_at FROM record_id i
             JOIN record r ON r.provider = i.provider AND r.native_id = i.native_id
             WHERE i.provider = ?1 AND i.work_id = ?2",
        )?;
        let mut missed =
            tx.prepare_cached("SELECT stored_at FROM miss WHERE provider = ?1 AND work_id = ?2")?;

        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let key = params![provider.as_str(), id.to_string()];
            let record: Option<(String, i64)> = found
                .query_row(key, |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()?;
            let miss: Option<i64> = missed.query_row(key, |r| r.get(0)).optional()?;
            let not_found = |at: i64| Entry {
                value: None,
                stored_at: timestamp(at),
            };
            // The newer answer wins if both exist.
            out.push(match (record, miss) {
                (Some((_, found_at)), Some(missed_at)) if missed_at > found_at => {
                    Some(not_found(missed_at))
                }
                (Some((json, at)), _) => Some(Entry {
                    value: Some(serde_json::from_str(&json)?),
                    stored_at: timestamp(at),
                }),
                (None, Some(at)) => Some(not_found(at)),
                (None, None) => None,
            });
        }
        Ok(out)
    }

    fn put_lookups(
        &self,
        provider: &ProviderId,
        answers: &[(&WorkId, Option<&SourceRecord>)],
        at: Timestamp,
    ) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        {
            let (p, at) = (provider.as_str(), at.as_second());
            let mut put_record = tx.prepare_cached(
                "INSERT OR REPLACE INTO record (provider, native_id, record, stored_at) VALUES (?1, ?2, ?3, ?4)",
            )?;
            let mut put_id = tx.prepare_cached(
                "INSERT OR REPLACE INTO record_id (provider, work_id, native_id) VALUES (?1, ?2, ?3)",
            )?;
            let mut clear_miss =
                tx.prepare_cached("DELETE FROM miss WHERE provider = ?1 AND work_id = ?2")?;
            let mut put_miss = tx.prepare_cached(
                "INSERT OR REPLACE INTO miss (provider, work_id, stored_at) VALUES (?1, ?2, ?3)",
            )?;

            for (requested, record) in answers {
                match record {
                    Some(record) => {
                        // A record without any identifier can't be found again.
                        let Some(native) = record.native_id().map(ToString::to_string) else {
                            continue;
                        };
                        put_record.execute(params![
                            p,
                            native,
                            serde_json::to_string(record)?,
                            at
                        ])?;
                        let ids = record
                            .ids
                            .iter()
                            .map(ToString::to_string)
                            .chain([requested.to_string()]);
                        for work_id in ids {
                            put_id.execute(params![p, work_id, native])?;
                            clear_miss.execute(params![p, work_id])?;
                        }
                    }
                    None => {
                        put_miss.execute(params![p, requested.to_string(), at])?;
                    }
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn get_search(
        &self,
        provider: &ProviderId,
        query: &SearchQuery,
    ) -> Result<Option<Entry<Page>>> {
        let row: Option<(String, i64)> = self
            .conn()
            .query_row(
                "SELECT page, stored_at FROM search WHERE provider = ?1 AND query = ?2",
                params![provider.as_str(), serde_json::to_string(query)?],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        row.map(|(json, at)| {
            Ok(Entry {
                value: serde_json::from_str(&json)?,
                stored_at: timestamp(at),
            })
        })
        .transpose()
    }

    fn put_search(
        &self,
        provider: &ProviderId,
        query: &SearchQuery,
        page: &Page,
        at: Timestamp,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT OR REPLACE INTO search (provider, query, page, stored_at) VALUES (?1, ?2, ?3, ?4)",
            params![
                provider.as_str(),
                serde_json::to_string(query)?,
                serde_json::to_string(page)?,
                at.as_second()
            ],
        )?;
        Ok(())
    }
}
