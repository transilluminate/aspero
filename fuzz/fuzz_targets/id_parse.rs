//! Identifier parsers: whatever parses, as any kind or detected, displays
//! canonically and re-parses to itself. Also the other readers of short
//! untrusted strings: author ids, sites, language codes.

#![no_main]

use aspero_core::{AuthorId, IdKind, WorkId};
use libfuzzer_sys::fuzz_target;

#[path = "common.rs"]
mod common;

fuzz_target!(|data: &[u8]| {
    let Ok(input) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(id) = input.parse::<WorkId>() {
        common::assert_round_trips(&id);
    }
    for kind in IdKind::ALL {
        if let Ok(id) = WorkId::parse_as(*kind, input) {
            assert_eq!(id.kind(), *kind);
            common::assert_round_trips(&id);
        }
    }
    if let Ok(doi) = input.parse::<aspero_core::Doi>() {
        assert_eq!(doi.as_str().parse::<aspero_core::Doi>().as_ref(), Ok(&doi));
    }
    let _ = AuthorId::parse(input);
    let _ = aspero_core::orcid(input);
    let _ = aspero_core::query::language(input);
    if let Some(site) = aspero_core::site::site_of(input) {
        let _ = aspero_core::site::doi_prefixes(&site);
    }
    let _ = aspero_core::site::registrable_domain(input);
});
