//! Cached answers: records by identifier, "not found" answers and search
//! pages, each with when it was stored ([`Cache`]).

use std::time::Duration;

use aspero_core::{Page, ProviderId, SearchQuery, SourceRecord, WorkId};
use jiff::Timestamp;
use rusqlite::{OptionalExtension, params};

use crate::{Direction, Edges, Result, SqliteCache, timestamp};

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

/// One ID's stored rows: its record (JSON, `stored_at`) and its miss
/// (`stored_at`).
type LookupRow = (Option<(String, i64)>, Option<i64>);

/// A row read, for tracking use: its rowid, and when it was last used.
pub(crate) type Used = (i64, i64);

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

impl SqliteCache {
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
        self.after_write();
        Ok(())
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
        self.after_write();
        Ok(())
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
        self.after_write();
        Ok(())
    }
}
