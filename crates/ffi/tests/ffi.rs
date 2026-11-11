//! The handle as an app sees it, without the network: typed answers,
//! offline from the cache, and awaitable from any executor.

use aspero_ffi::records::{
    Claim, ErrorKind, KeyStatus, LookupStatus, Reason, SearchRequest, Verdict,
};
use aspero_ffi::{Aspero, AsperoError, CallToken, Config, Patience};

fn text(text: &str) -> Claim {
    Claim {
        text: Some(text.into()),
        ..Claim::default()
    }
}

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
    let found = aspero(None).scan("see PMID 40536112".into());
    assert_eq!(found[0].id, "pmid:40536112");
    assert_eq!(found[0].kind, "pmid");
}

#[test]
fn mentions_carry_utf16_ranges() {
    // "é" is one UTF-16 unit, "👩‍⚕️" five; NSString counts these.
    let text = "é 👩‍⚕️ doi:10.1234/x\r\nPMID 40536112";
    let found = aspero(None).scan(text.into());
    let utf16: Vec<u16> = text.encode_utf16().collect();
    for m in &found {
        let range = m.utf16.start as usize..m.utf16.end as usize;
        assert_eq!(String::from_utf16(&utf16[range]).unwrap(), m.text);
    }
    assert_eq!(found.len(), 2);
}

#[test]
fn pasted_lists_split_into_references() {
    let list = "1. Smith J. One.\n   2020.\n2. Jones K. Two. 2021.";
    let refs = aspero(None).split_references(list.into());
    assert_eq!(refs[0].text, "Smith J. One. 2020.");
    let utf16: Vec<u16> = list.encode_utf16().collect();
    let second = refs[1].utf16.start as usize..refs[1].utf16.end as usize;
    assert_eq!(String::from_utf16(&utf16[second]).unwrap(), refs[1].text);
}

#[test]
fn dois_broken_at_a_hyphen_carry_an_alternative() {
    let text = "1. Smith J. doi:10.1038/nat-\n   ure14539\n2. Jones K. Two. 2021.";
    let handle = aspero(None);
    let refs = handle.split_references(text.into());
    assert_eq!(
        refs[0].alternative_ids[0].alternative,
        "doi:10.1038/nature14539"
    );
    let found = handle.scan(text.into());
    assert_eq!(
        found[0].alternative_id.as_deref(),
        Some("doi:10.1038/nature14539")
    );
}

#[test]
fn laid_out_lines_split_by_indent() {
    let line = |text: &str, indent| aspero_ffi::records::Line {
        text: text.into(),
        indent,
    };
    let refs = aspero(None).split_lines(vec![
        line("World Health Organization. Global report.", 72.0),
        line("Geneva: WHO; 2020.", 90.0),
        line("Smith J. A study. Nature. 2021.", 72.0),
    ]);
    assert_eq!(refs.len(), 2);
    assert_eq!((refs[0].lines.start, refs[0].lines.end), (0, 2));
}

#[test]
fn offline_lookups_answer_without_the_network() {
    let dir = std::env::temp_dir().join(format!("aspero-ffi-{}", std::process::id()));
    let handle = aspero(Some(dir.display().to_string()));
    let found = block_on(handle.lookup(
        vec!["10.1234/never-seen".into(), "not an id".into()],
        false,
        Patience::Offline,
        None,
    ))
    .unwrap();
    let never_seen = &found.results[0];
    assert_eq!(never_seen.status, LookupStatus::NotCached);
    assert_eq!(never_seen.id.as_deref(), Some("doi:10.1234/never-seen"));
    assert_eq!(never_seen.attempts[0].kind, Some(ErrorKind::NotCached));
    assert_eq!(found.results[1].status, LookupStatus::Invalid);
    assert!(found.results[1].error.is_some());
    drop(handle);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unreadable_claims_answer_at_once() {
    let checked = block_on(aspero(None).verify(
        vec![Claim {
            key: Some("7".into()),
            ..Claim::default()
        }],
        false,
        Patience::Offline,
        None,
    ))
    .unwrap();
    let v = &checked.results[0];
    assert_eq!(v.verdict, Verdict::Invalid);
    assert!(matches!(v.why, Reason::InvalidClaim { .. }));
    assert_eq!(v.key.as_deref(), Some("7"));
}

#[test]
fn claims_can_be_fields() {
    let claim = Claim {
        title: Some("Deep learning".into()),
        year: Some(2015),
        ids: vec!["10.1234/never-seen".into()],
        ..Claim::default()
    };
    let checked =
        block_on(aspero(None).verify(vec![claim], false, Patience::Offline, None)).unwrap();
    let v = &checked.results[0];
    assert_eq!(v.input, "Deep learning. 2015. 10.1234/never-seen");
    assert_eq!(v.verdict, Verdict::Unverifiable, "{}", v.reason);
}

#[test]
fn failures_say_what_kind() {
    let checked = block_on(aspero(None).verify(
        vec![text(""), text("doi:10.1234/never-seen")],
        false,
        Patience::Offline,
        None,
    ))
    .unwrap();
    let v = &checked.results[1];
    assert_eq!(v.index, 1);
    assert_eq!(v.verdict, Verdict::Unverifiable, "{}", v.reason);
    assert!(matches!(v.why, Reason::ProvidersFailed));
    assert!(!v.failures.is_empty());
    assert!(v.failures.iter().all(|f| f.kind == ErrorKind::NotCached));
}

#[test]
fn a_cancelled_token_ends_calls_at_once() {
    let token = CallToken::new();
    token.cancel();
    let result = block_on(aspero(None).verify(
        vec![text("Some citation, 2020")],
        false,
        Patience::Batch,
        Some(token),
    ));
    assert!(matches!(result, Err(AsperoError::Cancelled)));
}

#[test]
fn failed_searches_say_why() {
    let request = SearchRequest {
        text: "deep learning".into(),
        year_from: None,
        year_to: None,
        work_type: None,
        open_access_only: false,
        limit: 5,
        source: None,
        cursor: None,
    };
    let found = block_on(aspero(None).search(request, Patience::Offline, None)).unwrap();
    assert!(found.hits.is_empty());
    assert_eq!(found.failure.unwrap().kind, ErrorKind::NotCached);
}

#[test]
fn caches_report_and_clear() {
    let dir = std::env::temp_dir().join(format!("aspero-ffi-cache-{}", std::process::id()));
    let handle = aspero(Some(dir.display().to_string()));
    assert_eq!(handle.cache_stats().unwrap().unwrap().records, 0);
    handle.clear_cache().unwrap();
    assert!(aspero(None).cache_stats().unwrap().is_none());
    drop(handle);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn blank_keys_are_rejected_without_asking() {
    let checked = block_on(aspero(None).check_openalex_key("  ".into(), None)).unwrap();
    assert_eq!(checked.status, KeyStatus::Rejected);
}

/// Asks OpenAlex: `cargo test -p aspero-ffi -- --ignored`.
#[test]
#[ignore = "network"]
fn live_key_checks() {
    let handle = aspero(None);
    let checked = block_on(handle.check_openalex_key("not-a-real-key".into(), None)).unwrap();
    eprintln!("{checked:?}");
    assert_eq!(checked.status, KeyStatus::Rejected);
    if let Ok(key) = std::env::var("OPENALEX_API_KEY") {
        let checked = block_on(handle.check_openalex_key(key, None)).unwrap();
        eprintln!("{checked:?}");
        assert_eq!(checked.status, KeyStatus::Valid);
        assert!(checked.budget.is_some_and(|b| b.remaining.is_some()));
    }
}

/// Times the path where a DOI's reading without its line-break hyphen is
/// needed: `cargo test -p aspero-ffi alternative_timing -- --ignored --nocapture`.
#[test]
#[ignore = "network"]
fn alternative_timing() {
    let lists = [
        (
            "alt",
            "1. LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015;521:436-444. doi:10.1038/nature-\n14539",
        ),
        (
            "whole",
            "1. LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015;521:436-444. doi:10.1038/nature14539",
        ),
    ];
    for run in 1..=3 {
        for (name, list) in lists {
            let handle = aspero(None);
            let claims: Vec<Claim> = handle
                .split_references(list.into())
                .into_iter()
                .map(|r| Claim {
                    text: Some(r.text),
                    alternative_ids: r.alternative_ids,
                    ..Claim::default()
                })
                .collect();
            let start = std::time::Instant::now();
            let v = block_on(handle.verify(claims, false, Patience::Interactive, None)).unwrap();
            eprintln!(
                "{run} {name}: {:.1}s {:?} {}",
                start.elapsed().as_secs_f64(),
                v.results[0].verdict,
                v.results[0].reason
            );
        }
    }
}
