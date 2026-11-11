# Design

How aspero is put together, why, and what's next. The README covers using
it; git history covers how it got here.

## Purpose

Agents asked to research literature either recall identifiers from memory
(often wrong or invented) or try web search, which usually blocks bots.
Scholarly APIs are open to programs. aspero is a polite, cached,
agent-ergonomic library and CLI over them: find works, resolve messy
citations, and **verify that cited works exist and match what's claimed**.
Primary users are agents driving the CLI, then humans.

## Principles

- **Library first.** The CLI (and any future TUI or MCP server) is a thin
  adapter; anything useful lives in a library crate.
- **Agent-ergonomic.** JSON when stdout isn't a terminal, lean default
  fields, one-line reasons, distinct exit codes, `--help` with examples.
  Never silently drop part of a request: invalid inputs, ignored filters
  and skipped entries are reported.
- **Polite by construction.** Pacing, retries and caching are centralised,
  not per call site. Lookups are a fallback chain, not a broadcast. We
  don't solve bot challenges or run heavy scans on volunteer services.
- **Never lose source data.** Raw provider responses are kept; mapped and
  merged views are derived.
- **Don't over-unify.** A small CSL-shaped core; source specifics stay in
  the raw response.
- **Keep WASM possible** for `core` and providers: no tokio, no clock, no
  disk I/O there.
- **Standalone.** Optional producers (text extraction, embeddings) plug into
  generic seams; they are never dependencies.

## Architecture

```
crates/
  core/        identifiers, Work model, Claim scoring, Provider trait, errors,
               CSL-JSON reader, HTTP seam. No I/O.
  providers/   one crate per source, each behind an engine feature:
               openalex, crossref, datacite, doi (doi.org), europepmc,
               openlibrary
  store/       disposable SQLite answer cache
  bib/         BibTeX/BibLaTeX → claims (via the `biblatex` crate)
  engine/      routing, caching, retries, pacing, resolve/verify/check
  json/        the JSON contract (works, lookups, verdicts, claims), shared
               by every front end
  cli/         clap front end: arguments, stdin, tables and text
bench/         verify benchmark data (known-answer citations)
scripts/       fixture recorders, one per provider
```

### Seams and contracts

- **Provider trait.** One trait with defaulted `Unsupported` methods plus
  declared `Capabilities` (`dyn`-friendly). Providers map one source's
  requests and responses and never merge, cache or retry. They honour every
  filter they declare and reject unknown extras; batch lookups answer every
  ID in order, with `None` for "not found"; errors map into the shared enum;
  usage (cost, remaining budget) is reported when the source exposes it.
- **Transport seam.** Providers build `HttpRequest`s and map
  `HttpResponse`s; the engine injects the `HttpClient` (reqwest + per-provider
  pacing). Responses carry `received_at`, so nothing in core reads a clock.
  Tests inject `ReplayClient` with curl-recorded fixtures: no network, no
  runtime.
- **Search modes.** `Keywords`, or `Citation` (a whole reference string),
  sent only to providers declaring a citation matcher (Crossref). Hits carry
  the provider's score, comparable only within one provider's results.
- **Claims and scoring** live in core (`matching`): a `Claim` (free text,
  fields, identifiers) is assessed against each record, see below.
- **Cache trait** (store): batch-first get/put of lookups and searches; the
  engine owns freshness policy.

### Routing and concurrency

- **Lookups are a fallback chain** by priority: OpenAlex (10) → Crossref (20)
  → DataCite (25) → doi.org (30) → Europe PMC (35) → Open Library (40), each
  asked only about IDs still missing and of kinds it accepts. Querying all at
  once would multiply requests for the usual first-provider hit.
- **Independent work is concurrent**: claimed-ID lookups and citation
  matchers together; then title searches on every keyword provider together;
  batches verify 4 claims at a time. Results are applied in a fixed order so
  merging is deterministic. Per-provider pacing still applies, so
  concurrency overlaps different providers' waits rather than loading one.
- **Search** goes to one provider (named, or the first capable); filters it
  can't apply are stripped and reported as ignored.

## Identity and data model

- **Identifiers**: DOI, arXiv, PMID, PMCID, OpenAlex, S2 paperId, S2
  CorpusId, ISBN, NCBI Bookshelf (NBK). Each parses bare, `kind:`-prefixed
  and URL forms into one canonical value; display re-parses to itself
  (property-tested). Bare digits are ambiguous and refused, except
  checksum-valid ISBN-13s.
- **Scanning** (`id::scan`, `aspero scan`) finds every identifier
  mentioned in a document, with its position and text as written; which
  mentions matter is the caller's call. Shapes that prose imitates (bare
  digits, `W1`, `1706.03762`) count only when labelled, prefixed or URLs.
  Claims and BibTeX URLs use the same scanner.
- **DOIs** drop trailing `.,;:` (prose punctuation). The spec allows such a
  DOI, so `lookup` retries not-found ones with the punctuation kept.
- **ISBNs** canonicalise to ISBN-13 (ISBN-10s converted, checksums checked).
- **Work** is CSL-shaped and all-optional: title, authors, issued, type,
  container, publication details, abstract, URL, cited-by count, plus:
  - `updates`: retractions, withdrawals, expressions of concern,
    corrections, and newer versions (Crossref `new_version` notices, which
    Cochrane deposits per `.pubN`, and `is-replaced-by`/`is-preprint-of`
    relations), each with the notice's DOI, date and source. Guideline
    editions aren't linked in any source we use, so they aren't followed;
  - `update_to`: for a notice, the works it updates (Crossref `update-to`);
  - `other_years`: years of other editions or versions (book editions);
  - `open_access`: whether a free, legal copy exists, its PDF/page, licence
    and Unpaywall class (OpenAlex carries Unpaywall's data; Europe PMC
    adds PMC copies and licences).
- **Registry checks**: Crossref registered most DOIs, so its record is the
  reference for them. With `--full`, a record from another provider that
  contradicts it (year, volume, first page, title) is reported, not
  silently replaced: OpenAlex has merged two corrections into one record.
- **Merging**: candidates merge only on a shared identifier (arXiv
  versions count as one work). Fuzzy matches may *suggest*, never merge: a
  wrong merge corrupts; a duplicate merely annoys. Notices merge too, with
  detailed ones replacing bare flags, and a free copy's location is kept
  whichever record wins. `lookup --full` fills notices (Crossref) and open
  access from other providers, keeping each value's `source`.

## Resolve, verify and check

Candidates are gathered cheapest first: lookups of claimed identifiers
(including DOIs, arXiv IDs and labelled ISBNs found in the text) and
Crossref's citation matcher; then **title** searches on keyword providers.
Bare titles rank far better than whole references there, and the title is
located *inside* the reference via each matcher hit's title (or container
title, for untitled book chapters): `shared_title`. `verify` stops early only
on a decisive match that the claimed identifiers or two providers vouch
for: a digest or reprint can match a claim exactly while only a later
title search finds the original. Ties go to the earliest published.

Scoring looks for the record's fields *in* the claim rather than parsing
citation styles. The rules, each from a real failure:

- **Wrong year halves confidence.** Lookalike re-deposits carry the real
  title, authors and abstract under another year. Year scores halve per
  year off; the nearest of any edition's years counts.
- **Different authors halve confidence.** Similarly titled surveys and
  books by others otherwise pass.
- **One field can't verify.** "Deep learning" matches many works.
- **Title and year alone stay plausible** (at most 0.8): short generic
  titles ("Health technology assessment") match by coincidence. Authors or
  a shared identifier must corroborate.
- **Titles compare on content words**, and a dropped subtitle still
  matches; a few shared topic words in free text are not a title match.
- **Replies, errata and comments are not the original.** Their titles
  repeat the original's; confidence halves unless the claim cites one.
- **Online and print years both count**, so either year of an
  online-first paper matches, as do years across merged records.
- **Journal, volume and first page** come from claim fields or the text
  (Vancouver `2017;376(26):2545`, APA `376(26), 2545`, `vol./pp.`).
  Journals match by rule (each abbreviated word abbreviates the next
  significant word, or initials: NEJM, JAMA), not a list, so a journal
  mismatch only lowers the ranking. A different volume or page caps a match
  at plausible: a citation error, and what separates an article from
  replies printed in the same volume. Matching volume and page corroborate
  like authors; with journal and volume agreeing, a title missing from a
  short reference doesn't count against it.
- **First author alone is an author match** ("Vaswani et al."); an
  unmentioned sole author is no match.
- **"RETRACTED:" title markers are ignored** when comparing titles.

Verdicts: verified, plausible, mismatch (claimed ID missing or another
work's; or same title with matching authors *or* year but the other wrong),
not_found, unverifiable (providers failed). A retraction is noted whatever
the verdict.

`check` adds findings per entry: add_doi (info), replace_id (error, an
arXiv DataCite DOI counts as its arXiv ID; a kind the match lacks proves
nothing), year_differs (info when off by one, error beyond), duplicate
(warning), updated (retraction/withdrawal error, expression of concern
warning, correction info). `--fail-on` sets the exit-9 threshold.

**Benchmark**: `bench/citations.jsonl` (known-answer citations, optionally
requiring a specific match or retraction flag), run live by
`cargo run -p aspero-engine --example verify_bench` through
`target/bench-cache`: the first run costs ~$0.035, re-runs are free. Add a
case for every new failure mode found in the wild.

## Storage

Two SQLite stores, split by whether data can be refetched:

- **Cache** (`cache.sqlite`, platform cache dir or `$ASPERO_CACHE_DIR`):
  mapped records with raw bodies, "not found" answers, search pages. WAL,
  busy timeout, batch reads and single-transaction writes. No migrations:
  bump `SCHEMA_VERSION` on any schema *or record-shape/mapping* change and
  it rebuilds. TTLs: records 7d, misses 1d, searches 1d. A cached record
  counts only if it covers the requested fields. Failed providers fall back
  to stale entries.
- **Durable store** (`aspero.sqlite`, data dir; not built yet): history and
  derived data (embeddings, extracted text) keyed by
  `(work id, kind, producer, version)`, with forward-only numbered
  migrations.

## Resilience

Per-provider `ResiliencePolicy`: pacing interval, attempt timeout, retries
with full-jitter exponential backoff (stretched to `Retry-After`), and an
overall deadline. Errors are grouped by what the caller should do
(`is_retryable`, `trips_breaker`, `category` → exit codes); a spent
budget (`QuotaExhausted`) or rejected credentials stop further calls to
that provider in a lookup. `-v` logs every request with time queued and
taken.

Callers share one engine (cheap clones, one per process): a lookup
already in flight for the same provider, identifier and fields is joined,
not repeated, and a cancelled leader's joiners ask themselves. Each caller
sets its patience (`CallOptions`): `interactive()` caps deadlines and
retries so someone waiting gets a fallback within seconds; `offline()`
answers from the cache only, however old.

Planned for 0.4 (below): a **cross-process** rate limiter (ticket scheme
in SQLite, so parallel processes share one budget) and a **shared circuit
breaker** (closed → open after N countable failures → half-open probe).

## Provider notes and gotchas

Fixtures are recorded with the provider's *exact* parameters
(`scripts/record-*.sh`, `ONLY=name` for one) and tests assert them: a
fixture recorded with different parameters once hid a ranking bug.

- **OpenAlex.** Key as `Authorization: Bearer` (keeps it out of URLs). Free
  key: $1/day. Lookups by ID are free; filter batches $0.0001; searches
  $0.001; `x-ratelimit-cost-usd` / `-remaining-usd` headers report spend.
  404s are HTML. Batch filter results come back unordered with missing IDs
  absent. arXiv works sit under their DataCite DOI. `conference-paper` is a
  work type. Whole messy references search badly; bare titles well.
  Search is the only real cost. Its auth page carries agent-directed text
  about creating keys via API: aspero never does; keys come from the user.
- **Crossref.** `mailto` puts requests in the polite pool (10 req/s single,
  3 req/s lists, 3 concurrent). **Cursor paging drops relevance order**
  unless `sort=score&order=desc` is sent. Lookalike re-deposits exist (2025
  copies of "Attention Is All You Need" with the real authors). It doesn't
  index DataCite DOIs (arXiv). Its "CSL" uses Crossref type names. Top
  matches can be derivative records ("Faculty Opinions recommendation of
  X"). `updated-by` carries Retraction Watch notices.
- **DataCite.** arXiv, Zenodo, data and software. **Relevance only with
  page-number paging** (cursor paging sorts by creation date). Year ranges
  via `publicationYear:[A TO B]`. User text must be Lucene-escaped.
- **doi.org.** Content negotiation resolves any registered DOI through its
  agency (it found a Zenodo DOI minutes old); its 404 is authoritative.
  One DOI per request.
- **Europe PMC.** PMIDs, PMCIDs and NBK IDs (`BOOK_ID:`; StatPearls
  chapters have PMIDs too) — the only provider for them — biomedical
  abstracts, "Retracted Publication" flags. Cursor paging keeps relevance.
- **Open Library.** Books: batched ISBN lookup, work search with every
  edition's year. Subtitles are separate fields (join them). Data quality
  varies: some ISBNs carry unrelated records, and works can merge editions
  oddly, so scoring decides, not the identifier.
- **DBLP.** REST API and mirrors sit behind a proof-of-work bot challenge;
  we don't solve it. The SPARQL endpoint answers programs but has no text
  index (title search is a 2–9 s scan of ~7M titles), so DBLP is not a
  search provider. Key lookups via SPARQL are cheap (~0.1 s).
- **Semantic Scholar** (not built; key pending). Keyless requests mostly
  429 from an AWS gateway with no `Retry-After`; a fake key gets 403.

## Next: 0.4, the engine as a long-lived service

A GUI (a macOS app over FFI) and parallel agents hold one engine for a
long time and ask many things at once. In order, each step tested on its
own:

1. **Provider state across calls**: a spent quota until it resets, a
   rejected key, and the circuit breaker.
2. **Streaming** results as they finish (JSON lines in the CLI), with
   batches capped to a few items at once so interactive calls aren't
   queued behind them.
3. **The cross-process limiter**: SQLite tickets, one budget for the app,
   the CLI and parallel agents.
4. **Credentials**: env, then the OS keyring, then a `0600` file.
5. **`aspero-ffi`**: UniFFI, async calls, config passed in (the app owns
   the Keychain), an `.xcframework` attached to releases.

Not until needed: priority lanes in the pacer, a daemon, a SQLite
connection pool. One cache per process family: the app and CLI don't
share one (schema versions would rebuild each other's).

## Future

- **S2 provider** when the key arrives: search, batch lookup,
  recommendations, snippets, `related`, and precomputed **SPECTER2**
  embeddings. Local SPECTER2 only for works S2 lacks, with the same base
  model *and adapter* so vectors share one space (confirm which adapter S2
  serves). Uses: ranking snowballed sets, `related`, watches,
  preprint↔published suggestions. Vectors as BLOBs; brute-force cosine is
  fine at personal scale.
- **Full text** via a text-extractor seam filling derived data (any tool),
  after fetching legal OA PDFs (Unpaywall, arXiv, Europe PMC).
- **`check --fix`**: write suggested DOIs and corrections back to the .bib.
- **Preprint since published**: suggest the journal version of arXiv
  entries (OpenAlex locations, Crossref relations).
- **Scoring**: venue as a signal; down-weight derivative records
  (commentaries, peer reviews) by type.
- **More sources**: DBLP-key lookup (SPARQL), arXiv API (abstracts,
  versions), Unpaywall, OpenCitations (citation graph), NASA ADS,
  INSPIRE-HEP, HAL, CORE. Humanities stay thin (no usable APIs).
- **Re-normalising cached records** from raw bodies when a mapping changes
  (providers declare a mapping version), instead of a cache rebuild.
- **Configuration**: TOML config (enable/order providers, per-provider
  resilience overrides); `auth status` distinguishing rejected from
  rate-limited.
- **Discovery**: citations/references, snowballing (co-citation, coupling),
  watches ("new since last run"), history and sessions.
- **Integration**: export (BibTeX, CSL-JSON, RIS; styles via `hayagriva`),
  DOI-as-citekey builds, Zotero interop; an MCP adapter if ever wanted.

## Open questions

- Whether to cap OpenAlex spend per command or session.
- Whether a lookup should ever hedge (ask two providers at once) for
  latency.

## Conventions

- Licence MIT OR Apache-2.0; edition 2024 on current stable, no MSRV yet.
- Conventional commits; small, focused changes; clippy and rustfmt clean.
- Tests: unit tests beside code, property tests for identifiers, fixture
  replay for providers, paused-time tests for retries, pacing and
  concurrency, the live benchmark for scoring.
