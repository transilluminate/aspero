# Navigability dogfood

How easily a naive agent finds things in the code, with nothing pointing it
at the docs. `scripts/nav-dogfood.py` asks each question in
`bench/nav-tasks.jsonl` of a fresh `claude -p` session in a clean export of
the tree (`git archive`), with only Read, Grep and Glob, no MCP servers, no
user settings or CLAUDE.md, and at most 25 turns. It grades the answer (a
right path and a name in it; partial if only the path) and counts the work
it took. Rerun it after a restructuring and compare against the baseline
below:

```sh
scripts/nav-dogfood.py --runs 2            # current HEAD, sonnet
scripts/nav-dogfood.py --rev <commit>      # another tree
scripts/nav-dogfood.py --rescore --out target/nav-dogfood/<run>   # re-grade
```

Columns: tool calls (Read / Grep / Glob), distinct files read, lines Read
returned, lines Grep returned, wasted searches (Grep or Glob calls before
the first focused touch of a right file: reading it, grepping inside it, or
a search naming it among at most 5 files; all of them if none did), docs (R:
DESIGN.md or README.md read; g: only grep hits in the docs), turns, wall
seconds, input tokens (thousands, cache included).

## Baseline: 2026-11-16, 1e7d18b (0.30.0), sonnet, 2 runs per task

| task | run | ok | calls | R | G | files | lines | grep lines | waste | docs | turns | s | ktok |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| journal-cap | 1 | yes | 2 | 0 | 2 | 0 | 0 | 84 | 1 | g | 3 | 13 | 24 |
| journal-cap | 2 | yes | 5 | 1 | 4 | 1 | 14 | 155 | 2 | R | 6 | 25 | 50 |
| same-work | 1 | yes | 2 | 0 | 2 | 0 | 0 | 44 | 1 | g | 3 | 10 | 22 |
| same-work | 2 | yes | 2 | 0 | 2 | 0 | 0 | 44 | 1 | g | 3 | 10 | 22 |
| canonical-id | 1 | yes | 2 | 1 | 1 | 1 | 45 | 32 | 1 | g | 3 | 10 | 22 |
| canonical-id | 2 | yes | 3 | 1 | 2 | 1 | 50 | 39 | 1 | g | 4 | 14 | 28 |
| openalex-cap | 1 | yes | 3 | 1 | 2 | 1 | 150 | 72 | 2 | g | 4 | 15 | 36 |
| openalex-cap | 2 | yes | 3 | 0 | 3 | 0 | 0 | 76 | 2 | g | 4 | 28 | 32 |
| new-provider | 1 | yes | 5 | 1 | 4 | 1 | 150 | 74 | 0 | g | 6 | 14 | 45 |
| new-provider | 2 | yes | 5 | 1 | 4 | 1 | 73 | 46 | 0 | g | 6 | 20 | 41 |
| wiki-templates | 1 | yes | 4 | 1 | 3 | 1 | 20 | 66 | 1 | R | 5 | 24 | 31 |
| wiki-templates | 2 | yes | 2 | 0 | 2 | 0 | 0 | 52 | 0 | g | 3 | 19 | 21 |
| ffi-verification | 1 | yes | 3 | 0 | 3 | 0 | 0 | 254 | 2 | g | 4 | 11 | 31 |
| ffi-verification | 2 | yes | 4 | 0 | 4 | 0 | 0 | 178 | 3 | g | 5 | 34 | 37 |
| mcp-bearer | 1 | yes | 2 | 1 | 1 | 1 | 30 | 42 | 1 | g | 3 | 8 | 22 |
| mcp-bearer | 2 | yes | 3 | 1 | 2 | 1 | 30 | 46 | 1 | g | 4 | 10 | 22 |
| title-suffix | 1 | yes | 4 | 0 | 4 | 0 | 0 | 99 | 1 | g | 5 | 21 | 30 |
| title-suffix | 2 | yes | 4 | 1 | 3 | 1 | 20 | 80 | 1 | R | 5 | 14 | 33 |
| journal-read | 1 | yes | 8 | 0 | 8 | 0 | 0 | 348 | 8 | g | 9 | 16 | 56 |
| journal-read | 2 | yes | 11 | 1 | 10 | 1 | 20 | 417 | 4 | g | 12 | 30 | 121 |
| json-verify | 1 | yes | 5 | 0 | 5 | 0 | 0 | 76 | 4 | g | 6 | 12 | 37 |
| json-verify | 2 | yes | 5 | 1 | 4 | 1 | 77 | 64 | 4 | g | 6 | 18 | 45 |
| tape-keys | 1 | yes | 3 | 0 | 3 | 0 | 0 | 89 | 2 | g | 4 | 9 | 23 |
| tape-keys | 2 | yes | 3 | 0 | 3 | 0 | 0 | 93 | 2 | g | 4 | 10 | 23 |
| query-validate | 1 | yes | 1 | 0 | 1 | 0 | 0 | 27 | 1 | - | 2 | 8 | 13 |
| query-validate | 2 | yes | 4 | 0 | 4 | 0 | 0 | 53 | 2 | - | 5 | 14 | 26 |
| cli-table | 1 | yes | 3 | 0 | 3 | 0 | 0 | 71 | 1 | g | 4 | 14 | 27 |
| cli-table | 2 | yes | 2 | 0 | 2 | 0 | 0 | 73 | 1 | g | 3 | 13 | 21 |

No Glob calls in any run.

**Totals (28 runs):** 28 correct, 0 partial (100%). Mean 3.7 tool calls,
0.4 files read, 24 lines read, 100 grep result lines, 1.8 wasted searches,
4.7 turns, 16 s, 34k input tokens (756 output). DESIGN.md or README.md read
in 3 runs, grep hits in the docs in 26. All runs ended normally (none hit
the turn cap). $0.87 for the 28 runs.

## Observations

- **Correctness has no headroom.** Every answer was right, so a
  restructuring has to show up in effort: calls, grep lines, tokens, and
  the spread between runs. The measure that matters most is the tail
  (journal-read), not the mean.
- **Models hardly read files.** They grep, often in content mode with
  context lines or a `fn |const ` listing scoped to one file, and answer
  from the line numbers. Several answers say they never opened the
  function's body (same-work, json-verify, tape-keys). Doc comments and
  descriptive names do most of the work: `JOURNAL_MISMATCH_CAP`,
  `TapeKey`, `same_work` and `canonical` are each found by the second
  grep. The file sizes didn't matter because nothing read them whole.
- **Worst: journal-read** (8 and 11 calls, 348 and 417 grep lines, 56k
  and 121k tokens). The question's words are everywhere: `journal`
  matched 170 files and `verify` 67. The model then grepped `journal`
  file by file. The answer, `cited_journal`/`cited_journals`, is
  `pub(crate)` near line 1,370 of `publication.rs` (2,560 lines, mostly
  about journal names), behind `Claim::cited_container` in
  `matching/mod.rs`. Run 2 detoured through `verdict.rs` and `journal_in`
  first. Neither run's searches narrowed to `publication.rs` until the
  end, so its waste is the highest.
- **ffi-verification** (254 and 178 grep lines): `Verification` isn't
  named for what it is (no `Result`/`Record` suffix). `verif` matched 104
  files, and the model listed every `struct|fn|const` in the 3,188-line
  `records.rs` (147 lines) to find it.
- **json-verify** (4 wasted searches both runs): grepping for
  `VerifyResult`, `serde` or `rename` found nothing, because the JSON shape
  is built by hand with `json!` in `crates/json/src/verify.rs`. One run
  found it only by grepping known field names (`best_match`,
  `reason_text`).
- **journal-cap** run 2 and **new-provider**: `plausible` and `providers`
  are broad words (52 and 32+ hits). The model fell back to `#80` issue
  numbers and DESIGN.md before finding the constant.
- **DESIGN.md acts as the index.** An unscoped grep returns its lines
  first, and in 26 of 28 runs the docs showed up in results. tape-keys
  answers cite "DESIGN.md around line 857". A move that leaves DESIGN.md's
  paths stale would cost here.
- **Variance** is small on well-named targets (same-work, tape-keys,
  cli-table were identical across runs) and large on vague ones
  (journal-read 8 vs 11 calls, journal-cap 2 vs 5).
