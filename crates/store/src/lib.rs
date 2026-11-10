//! SQLite store: cached provider records, "not found" answers, search pages,
//! and derived data (embeddings, extracted text, …) for later.
//!
//! The store keeps entries and when they were stored; it applies no policy.
//! Freshness (TTLs), field coverage and stale-on-error are the engine's
//! decisions, made from [`Entry::stored_at`]. Records are kept whole,
//! including the provider's raw response, so normalisation can be improved
//! and rerun without refetching.
//!
//! SQLite in WAL mode with a busy timeout, so several agent processes can
//! share one cache file.

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use aspero_core::{Page, ProviderId, SearchQuery, SourceRecord, WorkId};
use jiff::Timestamp;
use rusqlite::{Connection, OptionalExtension, params};

/// Schema version written to `PRAGMA user_version`.
const SCHEMA_VERSION: i64 = 1;

const SCHEMA_V1: &str = "
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
-- Anything computed from a work: embeddings, extracted text, …
CREATE TABLE derived (
    work_id    TEXT NOT NULL,
    kind       TEXT NOT NULL,      -- e.g. 'embedding', 'text'
    producer   TEXT NOT NULL,      -- e.g. 'specter2-proximity'
    version    TEXT NOT NULL,
    media_type TEXT NOT NULL,
    content    BLOB NOT NULL,
    stored_at  INTEGER NOT NULL,
    PRIMARY KEY (work_id, kind, producer, version)
);
";

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    #[error("cache database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("cache entry is corrupt: {0}")]
    Corrupt(#[from] serde_json::Error),
    #[error("could not create cache directory: {0}")]
    Io(#[from] std::io::Error),
    #[error("cache schema version {found} is newer than this aspero supports ({supported})")]
    TooNew { found: i64, supported: i64 },
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

/// What a lookup cache needs; the engine depends on this, not on SQLite.
pub trait Cache: Send + Sync {
    /// The cached answer for `id` at `provider`: `Some(record)`, or `None`
    /// for a cached "not found".
    fn get_lookup(
        &self,
        provider: &ProviderId,
        id: &WorkId,
    ) -> Result<Option<Entry<Option<SourceRecord>>>>;

    /// Stores `provider`'s answer for `id`. A record is also reachable by
    /// every identifier it asserts.
    fn put_lookup(
        &self,
        provider: &ProviderId,
        id: &WorkId,
        record: Option<&SourceRecord>,
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

/// Something computed from a work.
#[derive(Debug, Clone, PartialEq)]
pub struct Derived {
    pub kind: String,
    pub producer: String,
    pub version: String,
    pub media_type: String,
    pub content: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Stats {
    pub records: u64,
    pub identifiers: u64,
    pub misses: u64,
    pub searches: u64,
    pub derived: u64,
}

pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    /// Opens (creating if needed) the cache at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        match version {
            0 => {
                conn.execute_batch(SCHEMA_V1)?;
                conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            }
            SCHEMA_VERSION => {}
            found => {
                return Err(StoreError::TooNew {
                    found,
                    supported: SCHEMA_VERSION,
                });
            }
        }
        Ok(SqliteStore {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves SQLite consistent (each
        // write is one statement or transaction), so poisoning is ignorable.
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
            derived: count("derived")?,
        })
    }

    /// Empties every table, keeping the schema.
    pub fn clear(&self) -> Result<()> {
        self.conn().execute_batch(
            "DELETE FROM record; DELETE FROM record_id; DELETE FROM miss;
             DELETE FROM search; DELETE FROM derived; VACUUM;",
        )?;
        Ok(())
    }

    pub fn put_derived(&self, work: &WorkId, d: &Derived, at: Timestamp) -> Result<()> {
        self.conn().execute(
            "INSERT OR REPLACE INTO derived
                 (work_id, kind, producer, version, media_type, content, stored_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                work.to_string(),
                d.kind,
                d.producer,
                d.version,
                d.media_type,
                d.content,
                at.as_second()
            ],
        )?;
        Ok(())
    }

    /// Derived data of `kind` stored under any of `ids` (a work's
    /// identifiers), optionally from one producer.
    pub fn get_derived(
        &self,
        ids: &[WorkId],
        kind: &str,
        producer: Option<&str>,
    ) -> Result<Vec<Entry<Derived>>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT kind, producer, version, media_type, content, stored_at FROM derived
             WHERE work_id = ?1 AND kind = ?2 AND (?3 IS NULL OR producer = ?3)",
        )?;
        let mut out = Vec::new();
        for id in ids {
            let rows = stmt.query_map(params![id.to_string(), kind, producer], |r| {
                Ok(Entry {
                    value: Derived {
                        kind: r.get(0)?,
                        producer: r.get(1)?,
                        version: r.get(2)?,
                        media_type: r.get(3)?,
                        content: r.get(4)?,
                    },
                    stored_at: timestamp(r.get(5)?),
                })
            })?;
            for row in rows {
                let entry = row?;
                if !out.contains(&entry) {
                    out.push(entry);
                }
            }
        }
        Ok(out)
    }
}

fn timestamp(seconds: i64) -> Timestamp {
    Timestamp::from_second(seconds).unwrap_or(Timestamp::UNIX_EPOCH)
}

impl Cache for SqliteStore {
    fn get_lookup(
        &self,
        provider: &ProviderId,
        id: &WorkId,
    ) -> Result<Option<Entry<Option<SourceRecord>>>> {
        let conn = self.conn();
        let (provider, id) = (provider.as_str(), id.to_string());
        let found: Option<(String, i64)> = conn
            .query_row(
                "SELECT r.record, r.stored_at FROM record_id i
                 JOIN record r ON r.provider = i.provider AND r.native_id = i.native_id
                 WHERE i.provider = ?1 AND i.work_id = ?2",
                params![provider, id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let missed: Option<i64> = conn
            .query_row(
                "SELECT stored_at FROM miss WHERE provider = ?1 AND work_id = ?2",
                params![provider, id],
                |r| r.get(0),
            )
            .optional()?;
        let miss = |at: i64| Entry {
            value: None,
            stored_at: timestamp(at),
        };
        // The newer answer wins if both exist.
        Ok(match (found, missed) {
            (Some((_, found_at)), Some(missed_at)) if missed_at > found_at => Some(miss(missed_at)),
            (Some((json, at)), _) => Some(Entry {
                value: Some(serde_json::from_str(&json)?),
                stored_at: timestamp(at),
            }),
            (None, Some(at)) => Some(miss(at)),
            (None, None) => None,
        })
    }

    fn put_lookup(
        &self,
        provider: &ProviderId,
        id: &WorkId,
        record: Option<&SourceRecord>,
        at: Timestamp,
    ) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let (p, requested, at) = (provider.as_str(), id.to_string(), at.as_second());
        match record.and_then(|r| r.native_id().map(|n| (r, n.to_string()))) {
            Some((record, native)) => {
                tx.execute(
                    "INSERT OR REPLACE INTO record (provider, native_id, record, stored_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![p, native, serde_json::to_string(record)?, at],
                )?;
                let ids = record
                    .ids
                    .iter()
                    .map(ToString::to_string)
                    .chain([requested.clone()]);
                for work_id in ids {
                    tx.execute(
                        "INSERT OR REPLACE INTO record_id (provider, work_id, native_id) VALUES (?1, ?2, ?3)",
                        params![p, work_id, native],
                    )?;
                    tx.execute(
                        "DELETE FROM miss WHERE provider = ?1 AND work_id = ?2",
                        params![p, work_id],
                    )?;
                }
            }
            // A record without any identifier can't be found again; skip it.
            None if record.is_some() => {}
            None => {
                tx.execute(
                    "INSERT OR REPLACE INTO miss (provider, work_id, stored_at) VALUES (?1, ?2, ?3)",
                    params![p, requested, at],
                )?;
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
