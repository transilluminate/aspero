use std::time::Duration;

use aspero_core::{FieldSet, Hit, Page, ProviderId, Raw, SearchQuery, SourceRecord, Work, WorkId};
use aspero_store::{Cache, Derived, SqliteStore, StoreError};
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

#[test]
fn a_record_is_found_by_every_identifier_it_asserts() {
    let store = SqliteStore::open_in_memory().unwrap();
    let r = record(
        &[
            "W2626778328",
            "arXiv:1706.03762",
            "10.48550/arXiv.1706.03762",
        ],
        "Attention",
    );
    store
        .put_lookup(&OA, &id("arXiv:1706.03762v5"), Some(&r), at(10))
        .unwrap();

    for key in [
        "W2626778328",
        "arXiv:1706.03762",
        "10.48550/arxiv.1706.03762",
        "arXiv:1706.03762v5",
    ] {
        let entry = store
            .get_lookup(&OA, &id(key))
            .unwrap()
            .unwrap_or_else(|| panic!("{key}"));
        assert_eq!(entry.value.as_ref(), Some(&r), "{key}");
        assert_eq!(entry.stored_at, at(10));
    }
    assert!(
        store.get_lookup(&CR, &id("W2626778328")).unwrap().is_none(),
        "per provider"
    );
}

#[test]
fn misses_are_cached_and_cleared_by_a_later_find() {
    let store = SqliteStore::open_in_memory().unwrap();
    let doi = id("10.5555/aspero-does-not-exist");
    store.put_lookup(&CR, &doi, None, at(0)).unwrap();
    let entry = store.get_lookup(&CR, &doi).unwrap().unwrap();
    assert_eq!(entry.value, None);

    store
        .put_lookup(
            &CR,
            &doi,
            Some(&record(&["10.5555/aspero-does-not-exist"], "Now real")),
            at(5),
        )
        .unwrap();
    assert!(
        store
            .get_lookup(&CR, &doi)
            .unwrap()
            .unwrap()
            .value
            .is_some()
    );
    assert_eq!(store.stats().unwrap().misses, 0);
}

#[test]
fn freshness_is_judged_from_stored_at() {
    let store = SqliteStore::open_in_memory().unwrap();
    store
        .put_lookup(&OA, &id("W1"), Some(&record(&["W1"], "x")), at(0))
        .unwrap();
    let entry = store.get_lookup(&OA, &id("W1")).unwrap().unwrap();
    let day = Duration::from_secs(86_400);
    assert!(entry.is_fresh(at(3600), day));
    assert!(!entry.is_fresh(at(2 * 86_400), day));
}

#[test]
fn search_pages_round_trip_by_exact_query() {
    let store = SqliteStore::open_in_memory().unwrap();
    let mut query = SearchQuery::new("attention is all you need");
    query.page.limit = 5;
    let page = Page::new(
        vec![Hit::new(record(&["W1"], "Attention"), Some(295.8))],
        Some(3_384_260),
        None,
    );
    store.put_search(&OA, &query, &page, at(0)).unwrap();

    assert_eq!(store.get_search(&OA, &query).unwrap().unwrap().value, page);
    query.page.limit = 10;
    assert!(
        store.get_search(&OA, &query).unwrap().is_none(),
        "different query"
    );
}

#[test]
fn derived_data_is_found_via_any_identifier_of_the_work() {
    let store = SqliteStore::open_in_memory().unwrap();
    let embedding = Derived {
        kind: "embedding".into(),
        producer: "specter2-proximity".into(),
        version: "1".into(),
        media_type: "application/x-f32le".into(),
        content: [0.25f32, -1.0]
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect(),
    };
    store
        .put_derived(&id("arXiv:1706.03762"), &embedding, at(0))
        .unwrap();

    let work_ids = [id("W2626778328"), id("arXiv:1706.03762")];
    let found = store.get_derived(&work_ids, "embedding", None).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].value, embedding);
    assert!(
        store
            .get_derived(&work_ids, "embedding", Some("other"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_cache_file_persists_and_clears() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("aspero.sqlite");
    {
        let store = SqliteStore::open(&path).unwrap();
        store
            .put_lookup(&OA, &id("W1"), Some(&record(&["W1"], "kept")), at(0))
            .unwrap();
    }
    let store = SqliteStore::open(&path).unwrap();
    assert_eq!(store.stats().unwrap().records, 1);
    store.clear().unwrap();
    assert_eq!(store.stats().unwrap(), Default::default());
}

#[test]
fn refuses_a_cache_from_a_newer_aspero() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("aspero.sqlite");
    drop(SqliteStore::open(&path).unwrap());
    rusqlite::Connection::open(&path)
        .unwrap()
        .pragma_update(None, "user_version", 99)
        .unwrap();
    assert!(matches!(
        SqliteStore::open(&path),
        Err(StoreError::TooNew { found: 99, .. })
    ));
}
