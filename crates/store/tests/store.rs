// Exact constants: scores and spends rounded or summed from binary-exact values.
#![allow(clippy::float_cmp)]

use std::time::Duration;

use aspero_core::{FieldSet, Hit, Page, ProviderId, Raw, SearchQuery, SourceRecord, Work, WorkId};
use aspero_store::{Cache, Direction, Edges, PrepaidDay, PrepaidDays, SqliteCache};
use jiff::Timestamp;

const OA: ProviderId = ProviderId::from_static("openalex");
const CR: ProviderId = ProviderId::from_static("crossref");

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_second(1_794_312_000 + seconds).unwrap()
}

fn id(s: &str) -> WorkId {
    s.parse().unwrap()
}

fn record(ids: &[&str], title: &str) -> SourceRecord {
    let mut work = Work::default();
    work.title = Some(title.into());
    SourceRecord::new(
        OA,
        ids.iter().map(|i| id(i)).collect(),
        work,
        FieldSet::ALL,
        Raw {
            media_type: "application/json".into(),
            body: format!(r#"{{"title":"{title}"}}"#),
        },
        at(0),
    )
}

fn counts(cache: &SqliteCache) -> [u64; 5] {
    let s = cache.stats().unwrap();
    [s.records, s.identifiers, s.misses, s.searches, s.edges]
}

fn get(
    cache: &SqliteCache,
    provider: &ProviderId,
    key: &str,
) -> Option<aspero_store::CachedLookup> {
    cache
        .get_lookups(provider, &[id(key)])
        .unwrap()
        .pop()
        .unwrap()
}

#[test]
fn a_record_is_found_by_every_identifier_it_asserts() {
    let cache = SqliteCache::open_in_memory().unwrap();
    let r = record(
        &[
            "W2626778328",
            "arXiv:1706.03762",
            "10.48550/arXiv.1706.03762",
        ],
        "Attention",
    );
    let requested = id("arXiv:1706.03762v5");
    cache
        .put_lookups(&OA, &[(&requested, Some(&r))], at(10))
        .unwrap();

    for key in [
        "W2626778328",
        "arXiv:1706.03762",
        "10.48550/arxiv.1706.03762",
        "arXiv:1706.03762v5",
    ] {
        let entry = get(&cache, &OA, key).unwrap_or_else(|| panic!("{key}"));
        assert_eq!(entry.value.as_ref(), Some(&r), "{key}");
        assert_eq!(entry.stored_at, at(10));
    }
    assert!(get(&cache, &CR, "W2626778328").is_none(), "per provider");
}

#[test]
fn batches_answer_every_id_in_order() {
    let cache = SqliteCache::open_in_memory().unwrap();
    let (a, b, missing, unknown) = (id("W1"), id("W2"), id("10.5555/nope"), id("W3"));
    let (ra, rb) = (record(&["W1"], "A"), record(&["W2"], "B"));
    cache
        .put_lookups(
            &OA,
            &[(&a, Some(&ra)), (&missing, None), (&b, Some(&rb))],
            at(0),
        )
        .unwrap();

    let got = cache
        .get_lookups(&OA, &[b.clone(), unknown, missing, a])
        .unwrap();
    let summary: Vec<Option<Option<String>>> = got
        .iter()
        .map(|e| {
            e.as_ref()
                .map(|e| e.value.as_ref().and_then(|r| r.work.title.clone()))
        })
        .collect();
    assert_eq!(
        summary,
        [
            Some(Some("B".into())),
            None,
            Some(None),
            Some(Some("A".into()))
        ]
    );
}

#[test]
fn a_later_find_replaces_a_cached_miss() {
    let cache = SqliteCache::open_in_memory().unwrap();
    let doi = id("10.5555/aspero-does-not-exist");
    cache.put_lookups(&CR, &[(&doi, None)], at(0)).unwrap();
    assert_eq!(
        get(&cache, &CR, "10.5555/aspero-does-not-exist")
            .unwrap()
            .value,
        None
    );

    let now_real = record(&["10.5555/aspero-does-not-exist"], "Now real");
    cache
        .put_lookups(&CR, &[(&doi, Some(&now_real))], at(5))
        .unwrap();
    assert!(
        get(&cache, &CR, "10.5555/aspero-does-not-exist")
            .unwrap()
            .value
            .is_some()
    );
    assert_eq!(cache.stats().unwrap().misses, 0);
}

#[test]
fn freshness_is_judged_from_stored_at() {
    let cache = SqliteCache::open_in_memory().unwrap();
    cache
        .put_lookups(&OA, &[(&id("W1"), Some(&record(&["W1"], "x")))], at(0))
        .unwrap();
    let entry = get(&cache, &OA, "W1").unwrap();
    let day = Duration::from_hours(24);
    assert!(entry.is_fresh(at(3600), day));
    assert!(!entry.is_fresh(at(2 * 86_400), day));
}

#[test]
fn search_pages_round_trip_by_exact_query() {
    let cache = SqliteCache::open_in_memory().unwrap();
    let mut query = SearchQuery::new("attention is all you need");
    query.page.limit = 5;
    let page = Page::new(
        vec![Hit::new(record(&["W1"], "Attention"), Some(295.8))],
        Some(3_384_260),
        None,
    );
    cache.put_search(&OA, &query, &page, at(0)).unwrap();

    assert_eq!(cache.get_search(&OA, &query).unwrap().unwrap().value, page);
    query.page.limit = 10;
    assert!(
        cache.get_search(&OA, &query).unwrap().is_none(),
        "different query"
    );
}

#[test]
fn a_cache_file_persists_and_clears() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("cache.sqlite");
    {
        let cache = SqliteCache::open(&path).unwrap();
        cache
            .put_lookups(&OA, &[(&id("W1"), Some(&record(&["W1"], "kept")))], at(0))
            .unwrap();
    }
    let cache = SqliteCache::open(&path).unwrap();
    assert_eq!(cache.stats().unwrap().records, 1);
    cache.clear().unwrap();
    assert_eq!(counts(&cache), [0; 5]);
}

#[test]
fn a_cache_from_another_schema_version_is_rebuilt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.sqlite");
    {
        let cache = SqliteCache::open(&path).unwrap();
        cache
            .put_lookups(&OA, &[(&id("W1"), Some(&record(&["W1"], "old")))], at(0))
            .unwrap();
    }
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE from_the_future (x);")
            .unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
    }
    let cache = SqliteCache::open(&path).unwrap();
    assert_eq!(counts(&cache), [0; 5]);
    cache
        .put_lookups(&OA, &[(&id("W1"), Some(&record(&["W1"], "new")))], at(0))
        .unwrap();
}

/// Per-key spend outlives reopening, clearing and rebuilding: it's a
/// budget's accounting, not a cached answer.
#[test]
fn key_spend_adds_up_per_day_and_survives_rebuilds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.sqlite");
    {
        let cache = SqliteCache::open(&path).unwrap();
        assert_eq!(cache.key_spend("k1", "2026-11-14").unwrap(), 0.0);
        cache.add_key_spend("k1", "2026-11-14", 0.25).unwrap();
        cache.add_key_spend("k1", "2026-11-14", 0.5).unwrap();
        cache.add_key_spend("k2", "2026-11-14", 1.0).unwrap();
        cache.add_key_spend("k1", "2026-11-13", 2.0).unwrap();
        cache.clear().unwrap();
    }
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
    }
    let cache = SqliteCache::open(&path).unwrap();
    assert_eq!(cache.key_spend("k1", "2026-11-14").unwrap(), 0.75);
    assert_eq!(cache.key_spend("k2", "2026-11-14").unwrap(), 1.0);
    assert_eq!(cache.key_spend("k1", "2026-11-13").unwrap(), 2.0);
    // A month on, the old days go.
    cache.add_key_spend("k1", "2026-12-20", 0.25).unwrap();
    assert_eq!(cache.key_spend("k1", "2026-11-14").unwrap(), 0.0);
    assert_eq!(cache.key_spend("k1", "2026-12-20").unwrap(), 0.25);
}

/// Credit spent today is the day's first balance less the balance now.
/// Headers' balances only ever lower it: requests made together are
/// answered by servers minutes apart, the stale ones higher, so a rise in
/// them is never a top-up (it once ran the day's start up to $5 more than
/// was spent).
#[test]
fn stale_balances_in_headers_are_never_top_ups() {
    let header = |known, balance| PrepaidDay::see(known, balance, None, at(0));
    let mut day = header(None, 10.0);
    // As seen 2026-11-16: interleaved, some servers $0.18 behind.
    for balance in [
        9.70, 9.69, 9.705, 9.68, 9.8436, 9.6539, 9.8414, 9.6517, 9.8445, 9.6528, 9.6649,
    ] {
        day = header(Some(day), balance);
    }
    assert_eq!(day.start_usd, 10.0, "never raised");
    assert_eq!(day.last_usd, 9.6517, "the lowest seen");
    assert_eq!(day.spent_usd(), 0.3483);
}

/// The provider's own word (`/rate-limit`, saying the credit bought) sets
/// the balance either way, and only a rise in the credit bought is a
/// top-up; its word older than the last changes nothing.
#[test]
fn the_providers_own_word_sets_the_balance_and_tells_top_ups() {
    let see = |known, balance, bought, s| PrepaidDay::see(known, balance, bought, at(s));
    let first = see(None, 10.0, Some(10.0), 0);
    assert_eq!((first.start_usd, first.last_usd), (10.0, 10.0));
    assert_eq!(first.spent_usd(), 0.0);

    // Headers too low (a server ahead, or a guess): the word corrects it.
    let low = see(Some(first), 9.5, None, 5);
    assert_eq!(low.spent_usd(), 0.5);
    let said = see(Some(low), 9.75, Some(10.0), 10);
    assert_eq!(said.spent_usd(), 0.25);
    // A top-up of $5 after $0.25 spent: still $0.25 spent, then more.
    let topped = see(Some(said), 14.75, Some(15.0), 20);
    assert_eq!((topped.start_usd, topped.spent_usd()), (15.0, 0.25));
    assert_eq!(see(Some(topped), 14.5, None, 30).spent_usd(), 0.5);
    // A stale header after the top-up is still spend, until the word.
    assert_eq!(see(Some(topped), 14.7, None, 30).spent_usd(), 0.3);
    // The word older than the last changes nothing.
    assert_eq!(see(Some(said), 9.0, Some(10.0), 5), said);
    // Bought falling (credit expiring) is no top-up.
    let expired = see(Some(said), 4.75, Some(5.0), 30);
    assert_eq!(expired.start_usd, 10.0);
}

/// The first shape of the table took stale balances for top-ups: it's
/// dropped, and its days started again.
#[test]
fn a_prepaid_table_of_the_first_shape_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.sqlite");
    drop(SqliteCache::open(&path).unwrap());
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "DROP TABLE prepaid_day;
             CREATE TABLE prepaid_day (provider TEXT NOT NULL, day TEXT NOT NULL,
                 start_usd REAL NOT NULL, last_usd REAL NOT NULL, seen_at INTEGER NOT NULL,
                 PRIMARY KEY (provider, day));
             INSERT INTO prepaid_day VALUES ('openalex', '2026-11-16', 15.0, 9.66, 0);",
        )
        .unwrap();
    }
    let cache = SqliteCache::open(&path).unwrap();
    let day = cache
        .see_prepaid("openalex", "2026-11-16", 9.65, None, at(0))
        .unwrap();
    assert_eq!((day.start_usd, day.spent_usd()), (9.65, 0.0));
}

/// The day's balances are shared through the file (other processes,
/// restarts) and outlive clearing and rebuilding; each day starts again.
#[test]
fn prepaid_days_are_kept_per_provider_and_day_and_survive_rebuilds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.sqlite");
    {
        let cache = SqliteCache::open(&path).unwrap();
        let day = cache
            .see_prepaid("openalex", "2026-11-16", 10.0, None, at(0))
            .unwrap();
        assert_eq!(day.spent_usd(), 0.0);
        cache.clear().unwrap();
    }
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
    }
    let cache = SqliteCache::open(&path).unwrap();
    let other = SqliteCache::open(&path).unwrap();
    let day = other
        .see_prepaid("openalex", "2026-11-16", 9.0, None, at(10))
        .unwrap();
    assert_eq!(day.spent_usd(), 1.0, "another process sees the same day");
    let day = cache
        .see_prepaid("openalex", "2026-11-16", 8.5, None, at(20))
        .unwrap();
    assert_eq!((day.start_usd, day.spent_usd()), (10.0, 1.5));
    // The next day starts from its own first balance.
    let next = cache
        .see_prepaid("openalex", "2026-11-17", 8.5, None, at(86_400))
        .unwrap();
    assert_eq!(next.spent_usd(), 0.0);
    // Another provider's day is its own.
    let mine = cache
        .see_prepaid("other", "2026-11-16", 3.0, None, at(30))
        .unwrap();
    assert_eq!(mine.spent_usd(), 0.0);
    // A week on, the old days go.
    cache
        .see_prepaid("openalex", "2026-11-30", 8.0, None, at(14 * 86_400))
        .unwrap();
    let again = cache
        .see_prepaid("openalex", "2026-11-16", 8.0, None, at(14 * 86_400))
        .unwrap();
    assert_eq!(again.start_usd, 8.0, "forgotten, so started afresh");
}

#[test]
fn citation_links_round_trip_per_work_and_direction() {
    let cache = SqliteCache::open_in_memory().unwrap();
    let (a, b, unknown) = (id("W1"), id("W2"), id("W3"));
    let cites = Edges::new(vec![id("W30"), id("W10"), id("W20")], true);
    let page = Edges::new(vec![id("W40")], false);
    cache
        .put_edges(Direction::References, &[(&a, &cites)], at(0))
        .unwrap();
    cache
        .put_edges(Direction::CitedBy, &[(&a, &page), (&b, &page)], at(5))
        .unwrap();

    let got = cache
        .get_edges(Direction::References, &[unknown, a.clone(), b])
        .unwrap();
    assert!(got[0].is_none() && got[2].is_none());
    let entry = got[1].as_ref().unwrap();
    assert_eq!(entry.value, cites, "in the source's order");
    assert_eq!(entry.stored_at, at(0));
    let cited_by = cache.get_edges(Direction::CitedBy, &[a]).unwrap();
    assert_eq!(cited_by[0].as_ref().unwrap().value, page);
    assert_eq!(cache.stats().unwrap().edges, 3);
}

#[test]
fn records_keep_the_works_they_cite() {
    let cache = SqliteCache::open_in_memory().unwrap();
    let r = record(&["W1"], "Cites two").with_references(Some(vec![id("W2"), id("W3")]));
    cache
        .put_lookups(&OA, &[(&id("W1"), Some(&r))], at(0))
        .unwrap();
    let cached = get(&cache, &OA, "W1").unwrap().value.unwrap();
    assert_eq!(cached.references, Some(vec![id("W2"), id("W3")]));
}

/// The links table is added to a cache of the current version without
/// dropping what it holds.
#[test]
fn links_are_added_to_a_cache_without_rebuilding_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.sqlite");
    {
        let cache = SqliteCache::open(&path).unwrap();
        cache
            .put_lookups(&OA, &[(&id("W1"), Some(&record(&["W1"], "kept")))], at(0))
            .unwrap();
    }
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch("DROP TABLE edges;")
        .unwrap();
    let cache = SqliteCache::open(&path).unwrap();
    assert_eq!(cache.stats().unwrap().records, 1);
    let links = Edges::new(vec![id("W2")], true);
    cache
        .put_edges(Direction::References, &[(&id("W1"), &links)], at(0))
        .unwrap();
    assert_eq!(cache.stats().unwrap().edges, 1);
}

#[test]
fn a_file_that_is_not_a_cache_is_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.sqlite");
    std::fs::write(&path, "not a database, just text. ".repeat(200)).unwrap();
    let cache = SqliteCache::open(&path).unwrap();
    cache
        .put_lookups(&OA, &[(&id("W1"), Some(&record(&["W1"], "new")))], at(0))
        .unwrap();
    assert_eq!(cache.stats().unwrap().records, 1);
}

mod pacing {
    use std::time::Duration;

    use aspero_store::{Pacing, SqliteCache};
    use jiff::{SignedDuration, Timestamp};

    fn at(ms: i64) -> Timestamp {
        Timestamp::from_millisecond(ms).unwrap()
    }

    #[test]
    fn slots_are_spaced_across_every_user_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.sqlite");
        // Two processes, as two connections to one file.
        let (a, b) = (
            SqliteCache::open(&path).unwrap(),
            SqliteCache::open(&path).unwrap(),
        );
        let gap = Duration::from_millis(100);
        assert_eq!(a.claim_slot("crossref", gap, at(1_000)).unwrap(), at(1_000));
        assert_eq!(b.claim_slot("crossref", gap, at(1_010)).unwrap(), at(1_100));
        assert_eq!(a.claim_slot("crossref", gap, at(1_020)).unwrap(), at(1_200));
        // Providers are paced separately, and a free slot starts now.
        assert_eq!(b.claim_slot("openalex", gap, at(1_030)).unwrap(), at(1_030));
        assert_eq!(b.claim_slot("crossref", gap, at(5_000)).unwrap(), at(5_000));
    }

    #[test]
    fn a_clock_that_went_back_does_not_stall_requests() {
        let cache = SqliteCache::open_in_memory().unwrap();
        let gap = Duration::from_millis(100);
        let later = at(0) + SignedDuration::from_hours(1);
        cache.claim_slot("crossref", gap, later).unwrap();
        assert_eq!(cache.claim_slot("crossref", gap, at(0)).unwrap(), at(0));
    }
}
