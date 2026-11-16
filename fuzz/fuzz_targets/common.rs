//! Shared checks, included by the targets that use them (`#[path]`).

#![allow(dead_code)]

use std::ops::Range;

use aspero_core::{IdKind, WorkId};

/// `span` is a usable range of `text`: in bounds, non-empty, on char
/// boundaries.
pub fn assert_span(text: &str, span: &Range<usize>, what: &str) {
    assert!(
        span.start < span.end && span.end <= text.len(),
        "{what}: span {span:?} out of bounds of {} bytes in {text:?}",
        text.len()
    );
    assert!(
        text.get(span.clone()).is_some(),
        "{what}: span {span:?} off a char boundary in {text:?}"
    );
}

/// The canonical form re-parses to the same identifier: as a `WorkId`, as
/// its own kind, and through serde.
pub fn assert_round_trips(id: &WorkId) {
    let shown = id.to_string();
    assert_eq!(
        shown.parse::<WorkId>().as_ref(),
        Ok(id),
        "{shown:?} doesn't re-parse"
    );
    let (prefix, bare) = shown.split_once(':').expect("a kind prefix");
    assert_eq!(prefix, id.kind().prefix());
    assert_eq!(
        WorkId::parse_as(id.kind(), bare).as_ref(),
        Ok(id),
        "{bare:?} doesn't re-parse as {}",
        id.kind()
    );
    let json = serde_json::to_string(id).expect("serialises");
    assert_eq!(
        serde_json::from_str::<WorkId>(&json).ok().as_ref(),
        Some(id),
        "{json} doesn't deserialise"
    );
}

/// Each mention's span is usable, mentions come in order, each identifier
/// round-trips, and each DOI's other reading is another canonical DOI.
pub fn check_scan(text: &str) {
    let mentions = aspero_core::id::scan(text);
    let mut last = 0;
    for m in &mentions {
        assert_span(text, &m.span, "mention");
        assert!(m.span.start >= last, "mentions out of order: {mentions:?}");
        last = m.span.start;
        assert!(m.line >= 1 && m.column >= 1, "{m:?}");
        assert_round_trips(&m.id);
        if let Some(other) = m.alternative(text) {
            assert_eq!(other.kind(), IdKind::Doi, "{text:?}");
            assert_ne!(other, m.id, "{text:?}");
            assert_round_trips(&other);
        }
    }
}
