//! The size limit: tracking when entries were last used, and evicting the
//! least recently used once the database passes the limit.

use std::sync::atomic::Ordering;

use jiff::Timestamp;
use rusqlite::{Connection, TransactionBehavior, params};

use crate::cache::Used;
use crate::{Result, SqliteCache};

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

impl SqliteCache {
    /// Limits the cache to `max` bytes (as [`Stats::bytes`] counts them),
    /// or lifts the limit. Past it, the least recently used entries go
    /// first (never the pacing slots) until it's under nine tenths of the
    /// limit, and the pages they held are given back to the file system.
    /// Checked now, and after each write (two `PRAGMA` reads; evicting
    /// takes one pass over the entries' ages).
    ///
    /// [`Stats::bytes`]: crate::Stats::bytes
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
        // A u64 times 0.9: non-negative and in range.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
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
    pub(crate) fn after_write(&self) {
        let _ = self.enforce_limit();
    }

    /// Bumps the `used_at` of rows read (`(rowid, used_at)`), under a limit
    /// and when not bumped within [`TOUCH_EVERY`]. Best effort: a busy file
    /// only delays it.
    pub(crate) fn touch(&self, table: &str, rows: &[Used]) {
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
}

/// The database's size, and the part of it holding data, in bytes:
/// `page_count × page_size`, and that less the free pages.
pub(crate) fn size(conn: &Connection) -> Result<(u64, u64)> {
    let pragma =
        |name: &str| -> rusqlite::Result<i64> { conn.pragma_query_value(None, name, |r| r.get(0)) };
    let (pages, free, page) = (
        pragma("page_count")?,
        pragma("freelist_count")?,
        pragma("page_size")?,
    );
    let bytes = |n: i64| n.max(0).unsigned_abs() * page.max(0).unsigned_abs();
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
            freed += bytes.max(0).unsigned_abs();
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
