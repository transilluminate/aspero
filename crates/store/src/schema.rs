//! Opening the cache: the versioned schema, rebuilt on a mismatch, and the
//! tables and columns added beside it.

use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::{Mutex, Once};
use std::time::Duration;

use rusqlite::{Connection, TransactionBehavior, params};

use crate::edges::EDGES;
use crate::evict::ACCESS;
use crate::pacing::PACING;
use crate::prepaid::{CREDIT_HERE, PREPAID};
use crate::spend::SPEND;
use crate::{Result, SqliteCache, StoreError};

/// Bump on any schema change, *and* when cached records' shape or mapping
/// changes (records are stored as mapped); older caches are dropped and
/// rebuilt, newer ones left alone ([`SqliteCache::answers_unused`]). 2: records gained `updates` (retractions). 3: `other_years`.
/// 4: Crossref and Europe PMC map print/online years into `other_years`.
/// 5: `update_to` (what a notice updates). 6: `open_access`. 7: Crossref
/// relations map to `new_version` updates. 8: notices are typed as such.
/// 9: `http://` ORCIDs read everywhere; Crossref works read as CSL items.
/// 10: records carry the works they cite (OpenAlex `referenced_works`).
/// 11: OpenAlex's free copy isn't another version's (a Cochrane review's
/// `.pub4` for its `.pub5`); no abstract from an OpenAlex index with holes
/// or a full text's.
const SCHEMA_VERSION: i64 = 11;

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
        let mut newer = version(&conn)? > SCHEMA_VERSION;
        if !newer && version(&conn)? != SCHEMA_VERSION {
            // Take the write lock, then check again: another process may
            // have rebuilt it meanwhile, or a newer version.
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let found = version(&tx)?;
            newer = found > SCHEMA_VERSION;
            if found < SCHEMA_VERSION {
                // Disposable: drop what an older version left and rebuild.
                let tables: Vec<String> = tx
                    .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name NOT IN ('pacing', 'mcp_spend', 'prepaid_day', 'credit_here')")?
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
        if newer {
            NEWER_ONCE.call_once(|| {
                tracing::debug!(
                    "the cache was written by a newer version (schema {} here); running without its answers",
                    SCHEMA_VERSION
                );
            });
        } else {
            // The answers' own tables and columns, beside the schema.
            conn.execute_batch(EDGES)?;
            add_access(&mut conn)?;
        }
        // The ledgers, shared by every version.
        conn.execute_batch(PACING)?;
        conn.execute_batch(SPEND)?;
        drop_old_prepaid(&mut conn)?;
        conn.execute_batch(PREPAID)?;
        conn.execute_batch(CREDIT_HERE)?;
        Ok(SqliteCache {
            conn: Mutex::new(conn),
            max_bytes: AtomicU64::new(u64::MAX),
            newer,
        })
    }

    /// Whether this process runs without the file's cached answers: a
    /// newer version wrote them (its schema is newer than this one's), and
    /// dropping them, as an older schema is, would wipe that version's
    /// cache each time this one opened it (an embedder and the CLI on one
    /// file, one of them upgraded first). Lookups miss and nothing is
    /// stored or evicted, as if no cache were configured; the ledgers the
    /// file shares across processes (request pacing, spend) work as ever.
    pub fn answers_unused(&self) -> bool {
        self.newer
    }
}

/// The newer schema is said once per process, however often it's opened.
static NEWER_ONCE: Once = Once::new();

/// Drops a `prepaid_day` table of the first shape (no `bought_usd`), under
/// the write lock: it took stale balances for top-ups, so its days' starts
/// can be far too high, and a day's accounting is all it held.
fn drop_old_prepaid(conn: &mut Connection) -> Result<()> {
    let old = |c: &Connection| -> rusqlite::Result<bool> {
        let (table, column): (i64, i64) = c.query_row(
            "SELECT (SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'prepaid_day'),
                    (SELECT COUNT(*) FROM pragma_table_info('prepaid_day') WHERE name = 'bought_usd')",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok(table > 0 && column == 0)
    };
    if old(conn)? {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if old(&tx)? {
            tx.execute_batch("DROP TABLE prepaid_day;")?;
        }
        tx.commit()?;
    }
    Ok(())
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

/// Whether `e` says the file isn't a database, or is a damaged one.
fn is_unreadable(e: &rusqlite::Error) -> bool {
    matches!(
        e.sqlite_error_code(),
        Some(rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt)
    )
}
