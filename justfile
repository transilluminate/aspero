# aspero tasks. `just` lists them.

[private]
default:
    @just --list

# Format check, lints and tests, as CI runs them.
check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features

# Format, and apply clippy's suggested fixes.
fix:
    cargo fmt --all
    cargo clippy --workspace --all-targets --all-features --fix --allow-dirty -- -D warnings

# Verify the known-answer citations live (ASPERO_MAILTO; OPENALEX_API_KEY optional).
bench:
    cargo run -q -p aspero-engine --example verify_bench -- bench/citations.jsonl

# Throughput of `scan` on generated documents, or the cases matching a filter.
scan-bench filter="":
    cargo bench -q -p aspero-core --bench scan -- {{filter}}

# Re-record a provider's fixtures, or just one: `just record europepmc lookup_nbk`.
record provider name="":
    ONLY={{name}} scripts/record-{{provider}}.sh

# Checks plus a crates.io dry run, after bumping the version: everything a
# release needs short of publishing.
release-check: check
    cargo publish --workspace --dry-run --allow-dirty

# Build the macOS app framework and Swift bindings (needs Xcode).
xcframework:
    scripts/xcframework.sh
