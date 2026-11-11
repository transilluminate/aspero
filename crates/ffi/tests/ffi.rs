//! The handle as an app sees it, without the network: answers in the JSON
//! contract, offline from the cache, and awaitable from any executor.

use aspero_ffi::{Aspero, Config, Patience};
use serde_json::Value;

fn aspero(cache_dir: Option<String>) -> std::sync::Arc<Aspero> {
    Aspero::new(Config {
        mailto: None,
        openalex_key: None,
        cache_dir,
    })
    .unwrap()
}

/// Polls `f` on a separate runtime, as an app's executor would: the
/// handle's calls run on its own.
fn block_on<T>(f: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(f)
}

#[test]
fn scans_offline() {
    let found: Value =
        serde_json::from_str(&aspero(None).scan("see PMID 40536112".into())).unwrap();
    assert_eq!(found["mentions"][0]["id"], "pmid:40536112");
}

#[test]
fn offline_lookups_answer_without_the_network() {
    let dir = std::env::temp_dir().join(format!("aspero-ffi-{}", std::process::id()));
    let handle = aspero(Some(dir.display().to_string()));
    let json = block_on(handle.lookup(
        vec!["10.1234/never-seen".into(), "not an id".into()],
        false,
        Patience::Offline,
    ))
    .unwrap();
    let doc: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(doc["results"][0]["status"], "failed");
    assert!(
        doc["results"][0]["attempts"][0]["error"]
            .as_str()
            .unwrap()
            .contains("offline")
    );
    assert_eq!(doc["results"][1]["status"], "invalid");
    drop(handle);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unreadable_claims_answer_at_once() {
    let json = block_on(aspero(None).verify(
        vec![r#"{"titel": "typo", "key": 7}"#.into()],
        false,
        Patience::Offline,
    ))
    .unwrap();
    let doc: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(doc["results"][0]["verdict"], "invalid");
    assert_eq!(doc["results"][0]["key"], 7);
}
