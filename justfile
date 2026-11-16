# aspero tasks. `just` lists them.

[private]
default:
    @just --list

# Format check, navigability gates, lints and tests, as CI runs them.
check:
    cargo fmt --all --check
    scripts/file-sizes.py
    scripts/doc-paths.py
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features
    cargo check -p aspero-engine --no-default-features
    cargo check -p aspero-engine --no-default-features --features crossref

# Format, and apply clippy's suggested fixes.
fix:
    cargo fmt --all
    cargo clippy --workspace --all-targets --all-features --fix --allow-dirty -- -D warnings

# Verify the known-answer citations live (ASPERO_MAILTO; OPENALEX_API_KEY optional).
bench:
    cargo run -q -p aspero-engine --example verify_bench -- bench/citations.jsonl

# Entries, statuses, size and recorded cost per host of the HTTP tapes
# (the bench's and red team's, or those named); scripts/tape.py also
# exports and merges them.
tape-stats *tapes:
    scripts/tape.py stats {{tapes}}

# Throughput of `scan` on generated documents, or the cases matching a filter.
scan-bench filter="":
    cargo bench -q -p aspero-core --bench scan -- {{filter}}

# Re-record a provider's fixtures, or just one: `just record europepmc lookup_nbk`.
record provider name="":
    ONLY={{name}} scripts/record-{{provider}}.sh

# Fuzz a target (fuzz/README.md) for some seconds, default 600; needs nightly
# and cargo-fuzz: `just fuzz webpage 1800 -jobs=4 -workers=4`.
fuzz target seconds="600" *args:
    scripts/fuzz.sh {{target}} {{seconds}} {{args}}

# Write the FFI's and CLI's third-party notices (crates/{ffi,cli}/notices);
# `just notices --check` fails if they're out of date.
notices *args:
    cargo run -q -p notices-gen -- {{args}}

# Checks plus a crates.io dry run, after bumping the version (and running
# `just notices`): everything a release needs short of publishing.
release-check: check
    cargo run -q -p notices-gen -- --check
    cargo publish --workspace --dry-run --allow-dirty

# Build the macOS app framework and Swift bindings (needs Xcode).
xcframework:
    scripts/xcframework.sh
