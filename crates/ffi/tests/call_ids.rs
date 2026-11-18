//! A [`CallToken`]'s id is the id its call's diagnostics events carry
//! (DESIGN "Diagnostics events"). Its own test binary: the handle's calls
//! run on its runtime's threads, so the subscriber is global.

use aspero_core::testing::{Captured, Events};
use aspero_ffi::records::Claim;
use aspero_ffi::{Aspero, CallToken, Config, Patience};

fn block_on<T>(f: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(f)
}

fn id(e: &Captured, field: &str) -> Option<u64> {
    e.field(field).map(|v| v.parse().unwrap())
}

fn doi(doi: &str) -> Claim {
    Claim {
        text: Some(format!("doi:{doi}")),
        ..Claim::default()
    }
}

#[test]
fn a_tokens_id_is_its_calls() {
    let events = Events::default();
    tracing::subscriber::set_global_default(events.clone()).unwrap();
    let aspero = Aspero::new(Config::default()).unwrap();
    let calls = || events.named("aspero::engine", "call");

    // A lookup is one engine call: the token's.
    let token = CallToken::new();
    assert_ne!(token.id(), CallToken::new().id(), "unique");
    let ids = vec!["doi:10.1234/never-seen".to_owned()];
    block_on(aspero.lookup(ids.clone(), false, Patience::Offline, Some(token.clone()))).unwrap();
    let said: Vec<_> = calls().iter().map(|e| id(e, "call")).collect();
    assert_eq!(said, [Some(token.id())]);

    // Without a token, an id of its own.
    block_on(aspero.lookup(ids, false, Patience::Offline, None)).unwrap();
    let other = id(&calls()[1], "call").unwrap();
    assert!(other > token.id());

    // Verifying more than one claim: each claim's call, and the prefetch
    // of their identifiers, an id of its own, the token's as its parent.
    let batch = CallToken::new();
    let claims = vec![doi("10.1234/a"), doi("10.1234/b")];
    block_on(aspero.verify(claims, false, Patience::Offline, Some(batch.clone()))).unwrap();
    let parts: Vec<_> = calls()
        .into_iter()
        .filter(|e| id(e, "parent") == Some(batch.id()))
        .collect();
    let ops: Vec<_> = parts.iter().map(|e| e.field("op").unwrap()).collect();
    assert_eq!(ops, ["lookup", "verify", "verify"]);
    assert!(parts.iter().all(|e| id(e, "call") != Some(batch.id())));
}
