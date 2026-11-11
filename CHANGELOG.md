# Changelog

## 0.5.0

Breaking:

- A lookup that is offline and not in the cache has status `not_cached`
  (it was `failed`).
- `Verification` is `#[non_exhaustive]` and gains `why` and `notes`.
- FFI `verify` and `verify_each` take typed `Claim`s (text and/or fields,
  and a `key`) instead of strings.
- FFI calls return typed records instead of JSON strings: `lookup` →
  `Lookups`, `verify` → `Verifications`, `verify_each` hands the listener
  a `Verification` (with its claim's `index`) and returns usage, `scan` →
  `[Mention]`, `unavailable` → `[Unavailable]`. `AsperoError` gains
  `Cache`.

Added:

- `aspero verify --split` (and `resolve`) takes a pasted reference list
  (numbered, bulleted, one per paragraph or line, wrapped lines rejoined,
  DOIs and URLs broken by PDF extraction mended) and checks each
  reference; `split_references` in `aspero-core`, and
  `splitReferences` over FFI with UTF-16 ranges.
- `Engine::budgets()`: each provider's budget as its last response
  reported it, so it can be shown without spending any.
- FFI: `search`, `budgets`, `checkOpenalexKey` (one free lookup: valid,
  rejected or unreachable, and what's left), `cacheStats` and
  `clearCache`.
- Verdicts say why as data as well as in English: `Verification::why`
  (a `Reason`, with the fields that differ and their values) and `notes`
  (retraction, newer version). JSON adds `why: {code, differs, cited}`.
- Errors have a fine-grained `ErrorKind` (`rate_limited`,
  `budget_spent`, `key_rejected`, `not_cached`, …) beside their category.
  JSON adds `kind` to failed attempts and `error_kinds` to
  verifications.
- A verification's `errors` include failed lookups of its identifiers, not
  only failed searches.

Fixed:

- Labelled identifiers glued to a colon (`ISBN-13:979…`, `PMID:123`)
  are found by `scan` and parse as `scan` finds them.

## 0.4.1

- Blank claims (`""`, whitespace, or JSON with only blank fields) are
  `invalid` instead of being searched for.
- FFI: `lookup`, `verify` and `verify_each` take an optional `cancel:
  CallToken`; cancelling it aborts the call, which throws the new
  `AsperoError.Cancelled`. Swift `Task` cancellation doesn't reach Rust
  through UniFFI 0.32, so 0.4.0's claim that it did was wrong.
- FFI: `scan` mentions add `utf16: {start, end}` for `NSString` ranges.
- The xcframework is Apple silicon only and built for macOS 26.0
  (`UNIVERSAL=1` and `MACOSX_DEPLOYMENT_TARGET` for other builds),
  which also silences linker warnings about newer macOS versions.

## 0.4.0

Breaking:

- `verify --format jsonl` prints each result as it finishes, in any order
  (match by `input` or `key`); a slow claim no longer holds up the rest.
  Other formats keep input order. `Engine::verify_many` yields
  `(index, verification)` as each completes.
- The JSON contract moves to a new crate, `aspero-json`.

Added:

- A long-lived engine (for apps and parallel agents): lookups already in
  flight are shared rather than repeated; `CallOptions` set a caller's
  patience (`interactive()`: give up within seconds, `offline()`: cache
  only); a spent quota, rejected key or failing provider (circuit
  breaker) is skipped across calls, and `Engine::unavailable` says which
  and why.
- Parallel aspero processes (agents) share each provider's rate limit
  through the cache file, even with `--no-cache`.
- `aspero-ffi`: Swift bindings (UniFFI) for apps, answering in the same
  JSON, with results streamed as they finish; `AsperoFFI.xcframework.zip`
  and `aspero_ffi.swift` are attached to each release.
- `Engine::lookup_inputs`: identifiers as written, with `Extras`, so every
  front end answers as `aspero lookup` does.
- Help: where notice dates come from, and why they can differ from a plain
  lookup of the notice DOI.

## 0.3.4

- `canonical` on every work in JSON: one key per work whichever identifier
  found it (DOI, PMID or PMCID lookups of one work agree), for deduping.
- `lookup --full`: notices are dated by when they appeared (earliest of
  online and print), with the source's date kept as `deposited` when later;
  notices themselves are typed `erratum` or `retraction`; and a record
  that contradicts the DOI's registry (Crossref) gains `disagreements`, as
  when an aggregator merged two notices into one record. Contradicting
  records aren't used to date notices.

## 0.3.3

- Publisher URLs with a DOI in the path give that DOI, without landing-page
  suffixes (`/full`, `/abstract`, `/pdf`, `.pdf`, …): in `scan`, and as
  `lookup` input. doi.org URLs are taken as they are.
- A retraction or withdrawal notice that OpenAlex flags as retracted names
  itself as the `notice` (with `lookup --full`), so rows about one event
  share a `notice` to dedupe on.
- Help: `scan`'s `id` joins to `lookup`'s; `open_access` "closed" means no
  free copy is known to the source.

## 0.3.2

- Newer versions: Crossref `new_version` notices (Cochrane reviews,
  `.pub6` → `.pub7`) and `is-replaced-by`/`is-preprint-of` relations
  become `new_version` updates. `lookup --full` follows the chain and adds
  `latest`; `verify` notes "superseded by …"; `check` warns.

## 0.3.1

- `verify` and `resolve` check the journal (abbreviations and initials
  included: N Engl J Med, NEJM), volume and first page, from claim fields
  or Vancouver/APA-style text. A wrong volume or page makes a match
  plausible, with the reason naming the right one; short references
  without a title ("Daum RS et al. N Engl J Med 2017;376:2545") verify.
  `checks` gains `container`, `volume` and `page`.
- Structured claims: JSON lines take `journal` (or `container`), `volume`,
  `page`, and a `key` echoed back untouched; results carry the `claim` as
  understood. `--journal`, `--volume`, `--page` flags.
- Plausible verdicts say which details differ.

## 0.3.0

- **Breaking (library):** `IdKind::ALL` and `Field::ALL` are slices, so
  future kinds and fields aren't breaking changes. (They were arrays whose
  length the new NBK kind and open-access field changed.)
- `open_access` on works: whether a free, legal copy exists, its PDF and
  page, licence, version and Unpaywall class (gold, diamond, hybrid,
  bronze, green, closed), from OpenAlex and Europe PMC. Lookup tables gain
  a "Free" column; text reports a "Free copy" line. `lookup --full` fills
  it from other providers when the first didn't say.
- `aspero scan`: every identifier mentioned in text or Markdown files, with
  line, column, the text as written and context, as JSON(L) or a table.
  Offline. `lookup -` reads its JSON lines and echoes each as `from`.
- NCBI Bookshelf IDs (`NBK526056`, Bookshelf URLs), looked up via Europe
  PMC: StatPearls chapters with title, series and date.
- Identifiers in claims and BibTeX URLs are found by the same scanner:
  PMCIDs, NBK IDs and labelled PMIDs now count; brackets inside DOIs stay.

## 0.2.0

- **Breaking (JSON):** `authors` is a list of objects in every mode, each
  with a `name` (and `orcid` when known); `--full` adds `given` and
  `family`. Lean output used to give plain strings.
- `update_to` on notices: what an erratum corrects or a retraction notice
  retracts (Crossref `update-to`). `lookup --full` asks Crossref for
  notices in both directions when another provider answered.
- Verification: generic titles with only a year stay plausible; replies,
  errata and comments no longer pass for the original; online and print
  years both match; ties go to the earliest published; `verify` stops
  early only on a match its identifiers or two providers vouch for.
- Identifiers pasted from prose lose wrapping brackets, quotes, asterisks
  and trailing punctuation; balanced brackets inside DOIs stay.
- Batch exit codes documented.

## 0.1.0

First release.

- `verify` citations (free text, fields, JSON lines, batches): verified,
  plausible, mismatch (wrong or nonexistent identifier, wrong year or
  authors, with the right work named), not found; retractions noted.
- `check` BibTeX/BibLaTeX bibliographies, CI-style: missing and wrong DOIs,
  year corrections, duplicates, retracted works; `--fail-on`.
- `lookup` DOIs, arXiv IDs, PMIDs, PMCIDs, ISBNs and their URLs; `search`
  one provider with year/type/open-access filters and cursor paging;
  `resolve` messy citations to scored candidates; `--abstract` everywhere.
- Providers: OpenAlex, Crossref, DataCite, doi.org, Europe PMC, Open
  Library. Lookups fall back provider by provider; independent searches
  run concurrently; per-provider pacing, retries and backoff.
- SQLite answer cache with stale-on-error; `cache stats|clear|path`.
- Tables on a terminal, JSON when piped (`--format table|text|json|jsonl`);
  `-v` shows every request.
