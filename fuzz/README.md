# Fuzzing

[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) targets for the code
that reads untrusted input, offline: nothing here makes a request. The
crate is its own workspace, so the main one (tests, CI, publishing) never
builds it.

Needs nightly and cargo-fuzz:

```sh
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz
```

Run a target with its seeds, dictionary and limits (10 s per input, 2 GB):

```sh
just fuzz webpage            # 10 minutes
just fuzz scan 1800 -jobs=4 -workers=4
```

(`scripts/fuzz.sh <target> [seconds] [libFuzzer options…]` without just.)
It builds without a sanitizer: aspero forbids `unsafe`, so panics,
overflow (debug assertions are on), timeouts and memory are what to
catch, and AddressSanitizer's runtime hung at start on macOS 26 with
nightly 2025-12. Add `-fork=4 -ignore_crashes=1` to go on past failures.
New inputs collect in `corpus/<target>`, failures in `artifacts/<target>`;
`cargo +nightly fuzz tmin <target> <artifact>` minimises one.

| Target | What it reads | Checked beyond "no panic" |
| --- | --- | --- |
| `webpage` | `claim_from_page`: address, title, HTML, as a share sheet gives them | ids' spans usable, each id once and canonical |
| `scan` | `scan` and `find_ids` over any text | spans in bounds, on char boundaries, in order; ids round-trip; a mention on one line parses to its id; DOI alternatives canonical |
| `id_parse` | `WorkId` (detected and per kind), author ids, sites, language codes | whatever parses displays canonically and re-parses to itself |
| `bib_read` | BibTeX, RIS, CSL-JSON (`read` and each reader) | |
| `docx` | `.docx` files, or their XML zipped by the target | |
| `references` | `split_references`, and `split_lines` with indents | spans usable and in order; line ranges in bounds |
| `claim` | `Claim` from free text and `verify` stdin lines, scored against a record | ids round-trip; confidence within 0–1 |
| `titled` | free-text claims scored against a record cut from the claim (title, authors, journal, shifted or changed a little) | confidence and similarities within 0–1 |
| `query` | query trees as JSON, and every source's offline `search_supports` | written back, re-parses to itself; normalising is idempotent; the serde type agrees |
| `mcp_args` | the MCP `search_supports` tool's arguments, end to end | never spends |

Seeds (`seeds/`) come from the test fixtures and the bench's citations;
`dicts/` holds each target's tokens. Inputs that once failed are kept in
`artifacts-kept/`, each also a unit test where its fix is.
