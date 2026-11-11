//! `scan` on generated documents finds exactly the identifiers put in them.

#[path = "support/corpus.rs"]
mod corpus;

use aspero_core::id::scan;
use corpus::{Density, generate};

fn assert_finds_all(seed: u64, bytes: usize, density: Density) {
    let c = generate(seed, bytes, density);
    let found: Vec<(String, std::ops::Range<usize>)> = scan(&c.text)
        .into_iter()
        .map(|m| (m.id.to_string(), m.span))
        .collect();
    for (i, (want, got)) in c.expected.iter().zip(&found).enumerate() {
        assert_eq!(
            want,
            got,
            "mention {i} (seed {seed}): wanted {:?}, found {:?}",
            &c.text[want.1.clone()],
            &c.text[got.1.clone()]
        );
    }
    assert_eq!(found.len(), c.expected.len(), "seed {seed}");
}

#[test]
fn prose_corpora() {
    for seed in 0..20 {
        assert_finds_all(seed, 64 * 1024, Density::Prose);
    }
}

#[test]
fn reference_list_corpora() {
    for seed in 0..20 {
        assert_finds_all(seed, 64 * 1024, Density::References);
    }
}
