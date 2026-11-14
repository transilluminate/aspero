//! The cache's size: reported in bytes, and held under a limit by evicting
//! the least recently used entries.

use std::time::Duration;

use aspero_core::{FieldSet, Page, ProviderId, Raw, SearchQuery, SourceRecord, Work, WorkId};
use aspero_store::{Cache, Pacing, SqliteCache};
use jiff::Timestamp;

const OA: ProviderId = ProviderId::from_static("openalex");

/// Long ago, so a read since counts as more recent than any write.
fn at(seconds: i64) -> Timestamp {
    Timestamp::from_second(1_700_000_000 + seconds).unwrap()
}

fn id(s: &str) -> WorkId {
    s.parse().unwrap()
}

/// A record of about `kb` KB (its raw body), found by `W<n+1>`.
fn sized(n: usize, kb: usize) -> (WorkId, SourceRecord) {
    let key = id(&format!("W{}", n + 1));
    let mut work = Work::default();
    work.title = Some(format!("Work {n}"));
    let record = SourceRecord::new(
        OA,
        vec![key.clone()],
        work,
        FieldSet::ALL,
        Raw {
            media_type: "application/json".into(),
            body: format!("{n:0>width$}", width = kb * 1024),
        },
        at(0),
    );
    (key, record)
}

/// Stores records `from..to`, a second apart, oldest first.
fn fill(cache: &SqliteCache, from: usize, to: usize, kb: usize) {
    for i in from..to {
        let (key, record) = sized(i, kb);
        cache
            .put_lookups(&OA, &[(&key, Some(&record))], at(i as i64))
            .unwrap();
    }
}

fn has(cache: &SqliteCache, n: usize) -> bool {
    let key = id(&format!("W{}", n + 1));
    cache.get_lookups(&OA, &[key]).unwrap()[0].is_some()
}

#[test]
fn stats_say_the_size_in_bytes() {
    let cache = SqliteCache::open_in_memory().unwrap();
    let empty = cache.stats().unwrap().bytes;
    assert!(empty > 0, "an empty schema's pages");
    fill(&cache, 0, 10, 20);
    let full = cache.stats().unwrap().bytes;
    assert!(full >= empty + 200 * 1024, "{empty} → {full}");
    assert_eq!(cache.max_bytes(), None);
}

#[test]
fn over_its_limit_the_least_recently_used_go_first() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.sqlite");
    let cache = SqliteCache::open(&path).unwrap();
    let gap = Duration::from_millis(100);
    let slot = cache.claim_slot("openalex", gap, at(1_000)).unwrap();

    // A roomy limit tracks use without evicting.
    cache.set_max_bytes(Some(100 << 20)).unwrap();
    fill(&cache, 0, 40, 20);
    assert_eq!(cache.stats().unwrap().records, 40);
    // The oldest stored becomes the most recently used.
    assert!(has(&cache, 0));

    let limit = 300 * 1024;
    cache.set_max_bytes(Some(limit)).unwrap();
    let stats = cache.stats().unwrap();
    assert!(stats.bytes <= limit, "{} > {limit}", stats.bytes);
    assert!(stats.records > 0 && stats.records < 40, "{stats:?}");
    assert!(has(&cache, 0), "read recently: kept");
    assert!(has(&cache, 39), "stored last: kept");
    assert!(!has(&cache, 1), "least recently used: gone");
    // Their identifiers went with them.
    assert_eq!(stats.identifiers, stats.records);
    // The pacing slots are never evicted.
    let next = cache.claim_slot("openalex", gap, at(1_000)).unwrap();
    assert_eq!(next, slot.checked_add(gap).unwrap());

    // Writes past the limit evict as they go.
    for i in 40..60 {
        fill(&cache, i, i + 1, 20);
        assert!(cache.stats().unwrap().bytes <= limit);
    }
    assert!(has(&cache, 59) && has(&cache, 0));
    drop(cache);
    // On disk too, once checkpointed (closing does).
    let on_disk = std::fs::metadata(&path).unwrap().len();
    assert!(on_disk <= limit, "{on_disk} > {limit}");
}

#[test]
fn without_a_limit_reads_leave_use_alone() {
    let cache = SqliteCache::open_in_memory().unwrap();
    fill(&cache, 0, 20, 20);
    // Read without a limit: not counted as use.
    assert!(has(&cache, 0));
    let limit = 200 * 1024;
    cache.set_max_bytes(Some(limit)).unwrap();
    assert!(!has(&cache, 0), "oldest stored, never used since: gone");
    assert!(has(&cache, 19));
}

#[test]
fn misses_and_searches_are_evicted_too() {
    let cache = SqliteCache::open_in_memory().unwrap();
    let crossref = ProviderId::from_static("crossref");
    let empty = cache.stats().unwrap().bytes;
    let miss = id("10.1234/missing");
    cache
        .put_lookups(&crossref, &[(&miss, None)], at(0))
        .unwrap();
    let query = SearchQuery::new("sepsis");
    cache
        .put_search(
            &crossref,
            &query,
            &Page::new(Vec::new(), Some(0), None),
            at(1),
        )
        .unwrap();
    fill(&cache, 2, 12, 20);
    let limit = empty + 100 * 1024;
    cache.set_max_bytes(Some(limit)).unwrap();
    assert!(cache.get_lookups(&crossref, &[miss]).unwrap()[0].is_none());
    assert!(cache.get_search(&crossref, &query).unwrap().is_none());
    assert!(has(&cache, 11));
    assert!(cache.stats().unwrap().bytes <= limit);
}

#[test]
fn a_file_made_before_incremental_vacuum_is_converted_when_evicted_from() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.sqlite");
    {
        // As older versions made it: no auto vacuum.
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "auto_vacuum", "NONE").unwrap();
        conn.execute_batch("CREATE TABLE older (x);").unwrap();
    }
    let cache = SqliteCache::open(&path).unwrap();
    fill(&cache, 0, 30, 20);
    let limit = 200 * 1024;
    cache.set_max_bytes(Some(limit)).unwrap();
    assert!(cache.stats().unwrap().bytes <= limit);
    drop(cache);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let mode: i64 = conn
        .pragma_query_value(None, "auto_vacuum", |r| r.get(0))
        .unwrap();
    assert_eq!(mode, 2, "incremental");
}

#[test]
fn a_cache_made_before_use_tracking_gains_it_without_a_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.sqlite");
    {
        let cache = SqliteCache::open(&path).unwrap();
        fill(&cache, 0, 1, 1);
    }
    {
        // As 0.16 left it: no `used_at`.
        let conn = rusqlite::Connection::open(&path).unwrap();
        for table in aspero_store::ACCESS {
            conn.execute_batch(&format!("ALTER TABLE {table} DROP COLUMN used_at;"))
                .unwrap();
        }
    }
    let cache = SqliteCache::open(&path).unwrap();
    assert!(has(&cache, 0), "kept");
    fill(&cache, 1, 2, 1);
    assert_eq!(cache.stats().unwrap().records, 2);
}
