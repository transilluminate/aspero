use std::time::Duration;

use aspero_core::{FieldSet, Hit, Page, ProviderId, Raw, SearchQuery, SourceRecord, Work, WorkId};
use aspero_store::{Cache, SqliteCache};
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
    let day = Duration::from_secs(86_400);
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
    assert_eq!(cache.stats().unwrap(), Default::default());
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
    assert_eq!(cache.stats().unwrap(), Default::default());
    cache
        .put_lookups(&OA, &[(&id("W1"), Some(&record(&["W1"], "new")))], at(0))
        .unwrap();
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
