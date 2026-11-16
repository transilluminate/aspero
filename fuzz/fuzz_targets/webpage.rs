//! `claim_from_page` as a share sheet calls it: the input is the page's
//! address, a line break, the title the browser gave (empty for none), a
//! line break, then the HTML (absent when there's no second line break,
//! as for a page that didn't load).

#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "common.rs"]
mod common;

fuzz_target!(|data: &[u8]| {
    let Ok(input) = std::str::from_utf8(data) else {
        return;
    };
    let (url, rest) = input.split_once('\n').unwrap_or((input, ""));
    let (title, html) = match rest.split_once('\n') {
        Some((title, html)) => (title, Some(html)),
        None => (rest, None),
    };
    let title = (!title.is_empty()).then_some(title);
    let page = aspero_core::claim_from_page(html, url, title);
    for (i, id) in page.ids.iter().enumerate() {
        common::assert_span(&id.text, &id.mention.span, "page id");
        common::assert_round_trips(&id.mention.id);
        assert!(
            page.ids[..i].iter().all(|p| p.mention.id != id.mention.id),
            "{:?} found twice",
            id.mention.id
        );
    }
    if let Some(claim) = &page.claim {
        assert!(claim.authors.len() <= aspero_core::webpage::MAX_AUTHORS);
        for id in &claim.ids {
            common::assert_round_trips(id);
        }
        let _ = claim.as_read();
        let _ = claim.reference();
    }
});
