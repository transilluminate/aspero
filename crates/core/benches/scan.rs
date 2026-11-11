//! Throughput of `scan` (and `split_references`) on generated documents,
//! each checked against the identifiers put in it before it is timed.
//! `cargo bench -p aspero-core --bench scan [filter]`.

#[path = "../tests/support/corpus.rs"]
mod corpus;

use std::hint::black_box;
use std::time::{Duration, Instant};

use aspero_core::id::scan;
use aspero_core::split_references;
use corpus::{Density, generate};

fn main() {
    let filter = std::env::args().skip(1).find(|a| !a.starts_with('-'));
    const MB: usize = 1 << 20;
    let cases = [
        ("scan/plain/16M", Density::None, 16 * MB, false),
        ("scan/prose/64K", Density::Prose, 64 << 10, false),
        ("scan/prose/1M", Density::Prose, MB, false),
        ("scan/prose/16M", Density::Prose, 16 * MB, false),
        ("scan/references/1M", Density::References, MB, false),
        (
            "split/references/256K",
            Density::References,
            256 << 10,
            true,
        ),
    ];
    println!(
        "{:<24} {:>9} {:>9} {:>10} {:>10} {:>10} {:>10}",
        "case", "bytes", "mentions", "median", "min", "MB/s", "best MB/s"
    );
    for (name, density, bytes, split) in cases {
        if filter.as_deref().is_some_and(|f| !name.contains(f)) {
            continue;
        }
        let c = generate(42, bytes, density);
        let found: Vec<_> = scan(&c.text)
            .into_iter()
            .map(|m| (m.id.to_string(), m.span))
            .collect();
        assert!(
            found == c.expected,
            "{name}: scan disagrees with the corpus"
        );
        let run = || {
            if split {
                black_box(split_references(black_box(&c.text)).len())
            } else {
                black_box(scan(black_box(&c.text)).len())
            }
        };
        let times = time(run);
        let median = times[times.len() / 2];
        let rate = |d: Duration| c.text.len() as f64 / MB as f64 / d.as_secs_f64();
        println!(
            "{:<24} {:>9} {:>9} {:>10} {:>10} {:>10.0} {:>10.0}",
            name,
            c.text.len(),
            c.expected.len(),
            fmt(median),
            fmt(times[0]),
            rate(median),
            rate(times[0])
        );
    }
}

/// Sorted times of at least 5 runs, and of as many as fit in about 2s.
fn time(mut f: impl FnMut() -> usize) -> Vec<Duration> {
    f();
    let mut times = Vec::new();
    let start = Instant::now();
    while times.len() < 5 || (start.elapsed() < Duration::from_secs(2) && times.len() < 1000) {
        let t = Instant::now();
        f();
        times.push(t.elapsed());
    }
    times.sort();
    times
}

fn fmt(d: Duration) -> String {
    if d.as_millis() >= 10 {
        format!("{}ms", d.as_millis())
    } else {
        format!("{}µs", d.as_micros())
    }
}
