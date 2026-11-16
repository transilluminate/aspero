//! `scan` over any text: spans usable and in order, identifiers canonical,
//! and each mention as written, if lookup's parser reads it, read alike.

#![no_main]

use aspero_core::WorkId;
use libfuzzer_sys::fuzz_target;

#[path = "common.rs"]
mod common;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    common::check_scan(text);
    for m in aspero_core::id::scan(text) {
        let written = &text[m.span.clone()];
        // A mention as written reads the same through lookup's parser,
        // when that reads it at all: a DOI inside another piece
        // (`see:10.…`, a link on an unknown host) or spaced out is found
        // by `scan` alone (and read whole by `Claim::from_input`).
        if let Ok(parsed) = written.parse::<WorkId>() {
            assert_eq!(parsed, m.id, "{written:?} in {text:?}");
        }
    }
    let _ = aspero_core::matching::find_ids(text);
});
