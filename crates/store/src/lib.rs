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
//!
//! A size limit ([`SqliteCache::set_max_bytes`]) evicts the least recently
//! used entries (records, "not found" answers, searches, links; never the
//! pacing slots) once the database passes it; see [`ACCESS`].

use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
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
/// 9: `http://` ORCIDs read everywhere; Crossref works read as CSL items.
/// 10: records carry the works they cite (OpenAlex `referenced_works`).
const SCHEMA_VERSION: i64 = 10;

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

/// Agents' spend per MCP key and UTC day, kept apart from the versioned
/// schema like the pacing slots: a rebuild keeps it, and `clear` and
/// eviction leave it, since it's a budget's accounting, not an answer to
/// fetch again. Days more than [`SPEND_DAYS`] old are pruned as spend is
/// added.
const SPEND: &str = "
CREATE TABLE IF NOT EXISTS mcp_spend (
    key_id     TEXT NOT NULL,      -- the host's id for the key
    day        TEXT NOT NULL,      -- UTC date, YYYY-MM-DD
    usd        REAL NOT NULL,
    PRIMARY KEY (key_id, day)
);
";

/// How many days of per-key spend are kept.
const SPEND_DAYS: i64 = 31;

/// Citation links, added beside the versioned schema so that adding them
/// keeps the rest of the cache (a rebuild drops and recreates them too).
/// One row per work and direction: the ids it cites (`references`), or a
/// page of those citing it (`cited_by`).
const EDGES: &str = "
CREATE TABLE IF NOT EXISTS edges (
    work       TEXT NOT NULL,      -- the work's id (an OpenAlex work id)
    direction  TEXT NOT NULL,      -- 'references' or 'cited_by'
    ids        TEXT NOT NULL,      -- JSON list of work ids, in the source's order
    complete   INTEGER NOT NULL,   -- 1 if `ids` is every link the source has
    stored_at  INTEGER NOT NULL,   -- unix seconds
    PRIMARY KEY (work, direction)
);
";

/// The tables whose entries carry when they were last used, for evicting
/// the least recently used first under a size limit: a `used_at` column
/// (unix seconds) added beside the versioned schema (`ALTER TABLE … ADD
/// COLUMN`, so adding it rebuilt nothing, and older versions sharing the
/// file still read and write it). Rows written without it count as used
/// when stored. Writes set it; reads bump it only while a limit is set,
/// and at most once per [`TOUCH_EVERY`], so an unlimited cache's reads
/// stay reads. A record's identifiers (`record_id`) go with it.
pub const ACCESS: &[&str] = &["record", "miss", "search", "edges"];

/// A read bumps an entry's `used_at` only when it's at least this old
/// (seconds).
const TOUCH_EVERY: i64 = 3600;

/// Eviction stops below this share of the limit, so it runs now and then
/// rather than after every write once the cache is full.
const EVICT_TO: f64 = 0.9;

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

/// Which way a citation link points from a work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Direction {
    /// The works it cites.
    References,
    /// The works citing it.
    CitedBy,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::References => "references",
            Direction::CitedBy => "cited_by",
        }
    }
}

/// A work's links in one direction.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Edges {
    /// In the source's order.
    pub ids: Vec<WorkId>,
    /// Whether these are every link the source has (not a page of them).
    pub complete: bool,
}

impl Edges {
    pub fn new(ids: Vec<WorkId>, complete: bool) -> Self {
        Edges { ids, complete }
    }
}

/// A provider's cached answer for one ID: `Some(record)`, or `None` for a
/// cached "not found".
pub type CachedLookup = Entry<Option<SourceRecord>>;

/// One ID's stored rows: its record (JSON, stored_at) and its miss
/// (stored_at).
type LookupRow = (Option<(String, i64)>, Option<i64>);

/// A row read, for tracking use: its rowid, and when it was last used.
type Used = (i64, i64);

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

    /// The cached links of each of `works` in `direction`, in order;
    /// `None` where nothing is cached. A cache without links has none.
    fn get_edges(
        &self,
        direction: Direction,
        works: &[WorkId],
    ) -> Result<Vec<Option<Entry<Edges>>>> {
        let _ = direction;
        Ok(vec![None; works.len()])
    }

    /// Stores links in one transaction, replacing what was there.
    fn put_edges(
        &self,
        direction: Direction,
        edges: &[(&WorkId, &Edges)],
        at: Timestamp,
    ) -> Result<()> {
        let _ = (direction, edges, at);
        Ok(())
    }
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
        // Lets eviction give pages back (`incremental_vacuum`). It takes
        // effect only on a new file; an older one is converted when first
        // evicted from.
        conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
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
                    .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name NOT IN ('pacing', 'mcp_spend')")?
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
        conn.execute_batch(EDGES)?;
        conn.execute_batch(SPEND)?;
        add_access(&mut conn)?;
        Ok(SqliteCache {
            conn: Mutex::new(conn),
            max_bytes: AtomicU64::new(u64::MAX),
        })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves SQLite consistent (writes
        // are transactional), so poisoning is ignorable.
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Limits the cache to `max` bytes (as [`Stats::bytes`] counts them),
    /// or lifts the limit. Past it, the least recently used entries go
    /// first (never the pacing slots) until it's under nine tenths of the
    /// limit, and the pages they held are given back to the file system.
    /// Checked now, and after each write (two `PRAGMA` reads; evicting
    /// takes one pass over the entries' ages).
    pub fn set_max_bytes(&self, max: Option<u64>) -> Result<()> {
        self.max_bytes
            .store(max.unwrap_or(u64::MAX), Ordering::Relaxed);
        self.enforce_limit()
    }

    /// The size limit, if one is set.
    pub fn max_bytes(&self) -> Option<u64> {
        Some(self.max_bytes.load(Ordering::Relaxed)).filter(|m| *m != u64::MAX)
    }

    /// Evicts least recently used entries while the cache is over its
    /// limit.
    fn enforce_limit(&self) -> Result<()> {
        let Some(max) = self.max_bytes() else {
            return Ok(());
        };
        let mut conn = self.conn();
        if size(&conn)?.0 <= max {
            return Ok(());
        }
        let target = (max as f64 * EVICT_TO) as u64;
        // Each round deletes the oldest entries whose rows add up to the
        // excess, as estimated; pages only partly freed may need another.
        for _ in 0..64 {
            let (_, used) = size(&conn)?;
            if used <= target || evict(&mut conn, used - target)? == 0 {
                break;
            }
        }
        give_back_pages(&conn);
        Ok(())
    }

    /// Holds the cache to its limit after a write, which stands whether or
    /// not evicting worked (a busy file is evicted from next time).
    fn after_write(&self) -> Result<()> {
        let _ = self.enforce_limit();
        Ok(())
    }

    /// Bumps the `used_at` of rows read (`(rowid, used_at)`), under a limit
    /// and when not bumped within [`TOUCH_EVERY`]. Best effort: a busy file
    /// only delays it.
    fn touch(&self, table: &str, rows: &[Used]) {
        if self.max_bytes().is_none() {
            return;
        }
        let now = Timestamp::now().as_second();
        let stale: Vec<i64> = rows
            .iter()
            .filter(|(_, used)| now - used >= TOUCH_EVERY)
            .map(|(rowid, _)| *rowid)
            .collect();
        if stale.is_empty() {
            return;
        }
        let mut conn = self.conn();
        let bump = || -> rusqlite::Result<()> {
            let tx = conn.transaction()?;
            {
                let mut set = tx
                    .prepare_cached(&format!("UPDATE {table} SET used_at = ?1 WHERE rowid = ?2"))?;
                for rowid in stale {
                    set.execute(params![now, rowid])?;
                }
            }
            tx.commit()
        };
        let _ = bump();
    }

    /// The stored rows for each of `ids`, unparsed, and the record and miss
    /// rows read.
    fn lookup_rows(
        &self,
        provider: &ProviderId,
        ids: &[WorkId],
    ) -> Result<(Vec<LookupRow>, Vec<Used>, Vec<Used>)> {
        let mut conn = self.conn();
        // One read transaction: a consistent snapshot, one lock.
        let tx = conn.transaction()?;
        let mut found = tx.prepare_cached(
            "SELECT r.record, r.stored_at, r.rowid, COALESCE(r.used_at, r.stored_at)
             FROM record_id i
             JOIN record r ON r.provider = i.provider AND r.native_id = i.native_id
             WHERE i.provider = ?1 AND i.work_id = ?2",
        )?;
        let mut missed = tx.prepare_cached(
            "SELECT stored_at, rowid, COALESCE(used_at, stored_at) FROM miss
             WHERE provider = ?1 AND work_id = ?2",
        )?;
        let mut rows = Vec::with_capacity(ids.len());
        let (mut records, mut misses) = (Vec::new(), Vec::new());
        for id in ids {
            let key = params![provider.as_str(), id.to_string()];
            let record: Option<(String, i64, i64, i64)> = found
                .query_row(key, |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .optional()?;
            let miss: Option<(i64, i64, i64)> = missed
                .query_row(key, |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .optional()?;
            if let Some((_, _, rowid, used)) = &record {
                records.push((*rowid, *used));
            }
            if let Some((_, rowid, used)) = miss {
                misses.push((rowid, used));
            }
            rows.push((
                record.map(|(json, at, _, _)| (json, at)),
                miss.map(|(at, _, _)| at),
            ));
        }
        Ok((rows, records, misses))
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
            edges: count("edges")?,
            bytes: size(&conn)?.0,
        })
    }

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

    /// Empties the cache.
    pub fn clear(&self) -> Result<()> {
        self.conn().execute_batch(
            "DELETE FROM record; DELETE FROM record_id; DELETE FROM miss; DELETE FROM search; \
             DELETE FROM edges; VACUUM;",
        )?;
        Ok(())
    }
}

/// Adds the [`ACCESS`] tables' `used_at` column where it's missing, under
/// the write lock (another process may be adding it too).
fn add_access(conn: &mut Connection) -> Result<()> {
    let missing = |c: &Connection, table: &str| -> rusqlite::Result<bool> {
        let n: i64 = c.query_row(
            "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name = 'used_at'",
            params![table],
            |r| r.get(0),
        )?;
        Ok(n == 0)
    };
    let mut todo = Vec::new();
    for table in ACCESS {
        if missing(conn, table)? {
            todo.push(*table);
        }
    }
    if todo.is_empty() {
        return Ok(());
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for table in todo {
        if missing(&tx, table)? {
            tx.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN used_at INTEGER;"))?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// The database's size, and the part of it holding data, in bytes:
/// `page_count × page_size`, and that less the free pages.
fn size(conn: &Connection) -> Result<(u64, u64)> {
    let pragma =
        |name: &str| -> rusqlite::Result<i64> { conn.pragma_query_value(None, name, |r| r.get(0)) };
    let (pages, free, page) = (
        pragma("page_count")?,
        pragma("freelist_count")?,
        pragma("page_size")?,
    );
    let bytes = |n: i64| n.max(0) as u64 * page.max(0) as u64;
    Ok((bytes(pages), bytes(pages - free)))
}

/// Deletes the least recently used entries whose rows add up to about
/// `excess` bytes (at least one), in one transaction; how many went.
fn evict(conn: &mut Connection, excess: u64) -> Result<usize> {
    // A row's size as estimated from its text, plus keys and overhead.
    const OVERHEAD: i64 = 100;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let oldest: Vec<(String, i64, i64)> = {
        let mut stmt = tx.prepare(
            "SELECT kind, rowid, bytes FROM (
                 SELECT 'record' AS kind, rowid, COALESCE(used_at, stored_at) AS used,
                        length(record) + length(native_id) + ?1 AS bytes FROM record
                 UNION ALL
                 SELECT 'miss', rowid, COALESCE(used_at, stored_at), length(work_id) + ?1
                 FROM miss
                 UNION ALL
                 SELECT 'search', rowid, COALESCE(used_at, stored_at),
                        length(query) + length(page) + ?1 FROM search
                 UNION ALL
                 SELECT 'edges', rowid, COALESCE(used_at, stored_at), length(ids) + ?1
                 FROM edges
             ) ORDER BY used, kind, rowid LIMIT 5000",
        )?;
        stmt.query_map(params![OVERHEAD], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    let (mut freed, mut removed) = (0u64, 0);
    {
        // A record's identifiers lead nowhere once it's gone.
        let mut forget_ids = tx.prepare_cached(
            "DELETE FROM record_id WHERE (provider, native_id) =
                 (SELECT provider, native_id FROM record WHERE rowid = ?1)",
        )?;
        for (kind, rowid, bytes) in oldest {
            if freed >= excess && removed > 0 {
                break;
            }
            let table = match kind.as_str() {
                "record" => {
                    forget_ids.execute(params![rowid])?;
                    "record"
                }
                "miss" => "miss",
                "search" => "search",
                _ => "edges",
            };
            tx.execute(
                &format!("DELETE FROM {table} WHERE rowid = ?1"),
                params![rowid],
            )?;
            freed += bytes.max(0) as u64;
            removed += 1;
        }
    }
    tx.commit()?;
    Ok(removed)
}

/// Gives free pages back to the file system: `incremental_vacuum`, or, for
/// a file made before incremental vacuuming (`auto_vacuum` none), one
/// `VACUUM` that converts it. Best effort: on a busy file the pages stay
/// until next time (and are reused meanwhile).
fn give_back_pages(conn: &Connection) {
    let mode: i64 = conn
        .pragma_query_value(None, "auto_vacuum", |r| r.get(0))
        .unwrap_or(2);
    let _ = match mode {
        0 => conn.execute_batch("PRAGMA auto_vacuum = INCREMENTAL; VACUUM;"),
        // Stepped to the end: each step frees a page (`execute_batch`
        // steps once).
        _ => (|| {
            let mut stmt = conn.prepare("PRAGMA incremental_vacuum")?;
            let mut rows = stmt.query([])?;
            while rows.next()?.is_some() {}
            Ok(())
        })(),
    };
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
        let (rows, records, misses) = self.lookup_rows(provider, ids)?;
        self.touch("record", &records);
        self.touch("miss", &misses);
        // Parsed after the lock is released.
        let mut out = Vec::with_capacity(ids.len());
        for (record, miss) in rows {
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
                "INSERT OR REPLACE INTO record (provider, native_id, record, stored_at, used_at) \
                 VALUES (?1, ?2, ?3, ?4, ?4)",
            )?;
            let mut put_id = tx.prepare_cached(
                "INSERT OR REPLACE INTO record_id (provider, work_id, native_id) VALUES (?1, ?2, ?3)",
            )?;
            let mut clear_miss =
                tx.prepare_cached("DELETE FROM miss WHERE provider = ?1 AND work_id = ?2")?;
            let mut put_miss = tx.prepare_cached(
                "INSERT OR REPLACE INTO miss (provider, work_id, stored_at, used_at) \
                 VALUES (?1, ?2, ?3, ?3)",
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
        drop(conn);
        self.after_write()
    }

    fn get_search(
        &self,
        provider: &ProviderId,
        query: &SearchQuery,
    ) -> Result<Option<Entry<Page>>> {
        let row: Option<(String, i64, i64, i64)> = self
            .conn()
            .query_row(
                "SELECT page, stored_at, rowid, COALESCE(used_at, stored_at) FROM search
                 WHERE provider = ?1 AND query = ?2",
                params![provider.as_str(), serde_json::to_string(query)?],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        if let Some((_, _, rowid, used)) = &row {
            self.touch("search", &[(*rowid, *used)]);
        }
        row.map(|(json, at, _, _)| {
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
            "INSERT OR REPLACE INTO search (provider, query, page, stored_at, used_at) \
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![
                provider.as_str(),
                serde_json::to_string(query)?,
                serde_json::to_string(page)?,
                at.as_second()
            ],
        )?;
        self.after_write()
    }

    fn get_edges(
        &self,
        direction: Direction,
        works: &[WorkId],
    ) -> Result<Vec<Option<Entry<Edges>>>> {
        type Row = (String, bool, i64, i64, i64);
        let rows: Vec<Option<Row>> = {
            let mut conn = self.conn();
            let tx = conn.transaction()?;
            let mut get = tx.prepare_cached(
                "SELECT ids, complete, stored_at, rowid, COALESCE(used_at, stored_at) FROM edges
                 WHERE work = ?1 AND direction = ?2",
            )?;
            let mut rows = Vec::with_capacity(works.len());
            for work in works {
                let key = params![work.to_string(), direction.as_str()];
                rows.push(
                    get.query_row(key, |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
                    })
                    .optional()?,
                );
            }
            rows
        };
        let used: Vec<Used> = rows.iter().flatten().map(|r| (r.3, r.4)).collect();
        self.touch("edges", &used);
        // Parsed after the lock is released.
        rows.into_iter()
            .map(|row| {
                row.map(|(json, complete, at, _, _)| {
                    Ok(Entry {
                        value: Edges {
                            ids: serde_json::from_str(&json)?,
                            complete,
                        },
                        stored_at: timestamp(at),
                    })
                })
                .transpose()
            })
            .collect()
    }

    fn put_edges(
        &self,
        direction: Direction,
        edges: &[(&WorkId, &Edges)],
        at: Timestamp,
    ) -> Result<()> {
        // Serialised before the lock is taken.
        let rows: Vec<(String, String, bool)> = edges
            .iter()
            .map(|(work, e)| Ok((work.to_string(), serde_json::to_string(&e.ids)?, e.complete)))
            .collect::<Result<_>>()?;
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        {
            let mut put = tx.prepare_cached(
                "INSERT OR REPLACE INTO edges (work, direction, ids, complete, stored_at, used_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            )?;
            for (work, ids, complete) in rows {
                put.execute(params![
                    work,
                    direction.as_str(),
                    ids,
                    complete,
                    at.as_second()
                ])?;
            }
        }
        tx.commit()?;
        drop(conn);
        self.after_write()
    }
}
