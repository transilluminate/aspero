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
  bib/         BibTeX/BibLaTeX (via the `biblatex` crate), RIS, CSL-JSON →
               claims, and .docx citation fields (`docx` feature: Zotero,
               Mendeley); works → BibTeX, RIS, and citation styles (`styles`
               feature: hayagriva, with the official CSL styles bundled)
  engine/      routing, caching, retries, pacing, resolve/verify/check
  json/        the JSON contract (works, lookups, verdicts, claims, checks),
               shared by every front end
  cli/         clap front end: arguments, stdin, tables and text
  ffi/         UniFFI bindings for apps (Swift), shipped as an .xcframework
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
  can't apply are stripped and reported as ignored. Fields (author, title,
  container) and sorts (newest, most cited) it can't apply fail
  (`Unsupported`): dropping one would widen or reorder the search
  silently. Relevance never fails; with only fields to rank by, it is the
  source's own order.
- **Query trees** (`SearchQuery::tree`, core `query`): all, any and none
  of terms (keywords, title, author, container, abstract, author id),
  filters and relations, instead of the keywords and fields. The engine
  normalises a tree (blank leaves and empty groups dropped, same-kind
  groups flattened, an all or any of one node unwrapped), checks its
  values (author ids, language codes, year ranges, identifiers), folds the
  request's own filters in as more of all of it, and asks the provider's
  `express`, which translates it or says why not: one decision behind
  both `search` and the offline `search_supports`. Nothing in a tree is
  ignored. `check_supported` holds what `Capabilities` alone decide
  (fields, filters, relations, any, none, nesting); Lucene sources share
  `render_lucene`. Relations name a work by any identifier: the engine
  looks it up (the cache first) and searches by its OpenAlex work id,
  asking again by DOI or PMID when the record found (Europe PMC's, for a
  PMCID) has none. Newest first, a tree stops at this year unless a years
  filter that bounds every hit (the tree itself or one of its top-level
  all) has an upper year; years under any or none don't count.

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
  Claims and BibTeX URLs use the same scanner. It parses only pieces
  holding a byte from `/` to `:` (digits, `/`, `:`), found a chunk at a
  time, and reads the piece before as a label only then: ~1 GB/s on
  prose without identifiers, ~600 MB/s with some, ~200 MB/s on reference
  lists (`just scan-bench`, on seeded generated documents whose every
  identifier is known; `tests/scan_corpus.rs` checks the same corpora).
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
  a shared identifier must corroborate. Without a title, authors, an
  identifier or the place, nothing is plausible: a year or journal alone,
  or a title of a word or two found in free text, matches by chance.
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
  short reference doesn't count against it. Journal, volume and first page
  all agreeing verify on their own (0.9; 0.8 a year off), so running
  headers (`Injury 47 (2016) 797–804`, `Nature 521, 436–444 (2015)`) are
  found by an OpenAlex search on volume, first page and year (±1). A
  reference whose every word is the work's authors, journal, numbers or
  furniture gives no title rather than another one, but then needs an
  identifier or the place. A reference that names no one (no "et al", no
  surname with initials) has no author mismatch.
- **First author alone is an author match** ("Vaswani et al."); an
  unmentioned sole author is no match.
- **"RETRACTED:" title markers are ignored** when comparing titles.

Verdicts: verified, plausible, mismatch (claimed ID missing or another
work's; or same title with matching authors *or* year but the other wrong),
not_found, unverifiable (providers failed). A retraction is noted whatever
the verdict. Reasons that list differences (`partly_matches`, `closest`,
`id_partly_matches`) always list one; agreeing with nothing to corroborate
is `unconfirmed`, and a reply or correction carrying the cited title
differs in `kind`.

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

**Identify** names the work a document is from its first page: none of
the page's identifiers is taken on trust (pages print related articles,
data and the first references too). Each one, header and footer first (at
most six), is looked up in one batch, and its work must be confirmed: its
title matching the title given (the app's largest type on the page, or
title metadata unless junk), or its title and first author on the page
(the earliest-placed title wins). Otherwise the top of the page is
verified as a citation; a given title not on the page (a banner, "PLOS
ONE") only joins the text. `decided_by` says which.

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

The engine remembers providers that can't answer, across calls: a spent
quota until it resets, rejected credentials for its life (new credentials
mean a new engine), and a **circuit breaker** (closed → open after N
consecutive failing calls → one probe after the cooldown). Refused calls
fall back like failed ones (next provider, stale cache);
`Engine::unavailable` says which providers are refusing and why. Batches
verify a few claims at once and yield each as it finishes, so a slow
claim doesn't hold up the rest, and a batch never queues so many requests
that a caller sharing the engine waits behind it.

Pacing is **shared across processes** through the cache file: each
request claims its provider's next slot in a `pacing` table (one write
transaction), so parallel agents keep to one rate between them, even with
`--no-cache`. The table sits outside the versioned schema so rebuilds keep
it; a slot more than five minutes ahead (a clock gone back) is reset; a
failing store falls back to pacing one process alone.

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
  Filters only AND; but `search=` and the `.search` filters take boolean
  queries (`AND`, `OR`, `NOT`, quotes, brackets), `|` ORs one filter's
  values and `!` negates a whole filter. So a tree's any must keep within
  one text field or one filter, its none negates each node (not a year
  range, nor an all across fields), and `cites:`/`cited_by:` take work
  ids. ORCIDs match bare or as URLs.
  `sort=relevance_score:desc` is refused without a search (text or a
  `.search` filter), so relevance is never sent: it is the default. Works
  can't be filtered by journal name, so a container search first finds
  the sources so named (another $0.001) and ORs the best 100 IDs. An
  any of journals finds each one's sources and ORs them all, each journal
  given an equal share of the 100 values a filter takes.
- **Crossref.** `mailto` puts requests in the polite pool (10 req/s single,
  3 req/s lists, 3 concurrent). **Cursor paging drops relevance order**
  unless `sort=score&order=desc` is sent. Lookalike re-deposits exist (2025
  copies of "Attention Is All You Need" with the real authors). Field
  queries: `query.author`, `query.title` (titles only, unlike
  `query.bibliographic`), `query.container-title`. It doesn't
  index DataCite DOIs (arXiv). Its "CSL" uses Crossref type names. Top
  matches can be derivative records ("Faculty Opinions recommendation of
  X"). `updated-by` carries Retraction Watch notices.
- **DataCite.** arXiv, Zenodo, data and software. **Relevance only with
  page-number paging** (cursor paging sorts by creation date). Year ranges
  via `publicationYear:[A TO B]`. User text must be Lucene-escaped.
  `sort=-published` doesn't order by year, and `container.title:` matches
  nothing, so no newest-first or container search. ORCIDs sit in
  `creators.nameIdentifiers.nameIdentifier` mostly as URLs, some bare;
  `language` holds `en` mostly, some `eng`: trees ask for both.
- **doi.org.** Content negotiation resolves any registered DOI through its
  agency (it found a Zenodo DOI minutes old); its 404 is authoritative.
  One DOI per request.
- **Europe PMC.** PMIDs, PMCIDs and NBK IDs (`BOOK_ID:`; StatPearls
  chapters have PMIDs too) — the only provider for them — biomedical
  abstracts, "Retracted Publication" flags. Cursor paging keeps relevance.
  Full Lucene: `NOT` alone and nested; `LANG` takes MEDLINE's codes (`ger`,
  `fre`), `AUTHORID` a bare ORCID (the URL finds nothing); "not
  retracted" is `NOT PUB_TYPE:"Retracted Publication"`.
- **Open Library.** Books: batched ISBN lookup, work search with every
  edition's year. Subtitles are separate fields (join them). Data quality
  varies: some ISBNs carry unrelated records, and works can merge editions
  oddly, so scoring decides, not the identifier. `q` takes fielded Solr
  queries (`title:`, `author_name:`, `language:fre`), but one that only
  excludes fails (HTTP 500), so a tree's none needs something beside it
  to find.
- **DBLP.** REST API and mirrors sit behind a proof-of-work bot challenge;
  we don't solve it. The SPARQL endpoint answers programs but has no text
  index (title search is a 2–9 s scan of ~7M titles), so DBLP is not a
  search provider. Key lookups via SPARQL are cheap (~0.1 s).
- **Semantic Scholar** (not built; key pending). Keyless requests mostly
  429 from an AWS gateway with no `Retry-After`; a fake key gets 403.

## Apps (FFI)

`aspero-ffi` (UniFFI) gives apps one `Aspero` handle: `lookup`, `verify`,
`verify_each` (a callback per result as it finishes), `search` (with
`searchCapabilities` saying what each source can search by, and
`searchSupports` whether it can search a query tree), offline
`scan`, `split_references` and `split_lines` (laid-out lines with
indents, from PDFKit), and for settings `budgets` (as last
reported: free), `check_openalex_key` (one free lookup), `cache_stats`,
`clear_cache` and `unavailable`. Network calls take a `Patience` (batch,
interactive, offline).
They return typed records (`Lookup`, `Verification`, `Work`, …), mirrors
of the engine's types converted in `aspero-ffi` so the engine stays free
of UniFFI. Reasons and failures are data (`Reason`, `ErrorKind`) as well
as English, so the app can word and localise them; `Work.json` carries
everything the records leave out. The app links its own copy of aspero,
so typed records cost no compatibility, unlike the CLI's JSON. The app passes everything in: contact
address, keys (it keeps them in the Keychain), cache directory; nothing is
read from the environment. Calls run on the handle's own Tokio runtime,
so any executor can await them. Cancellation is explicit: UniFFI's Swift
bindings (0.32) don't pass a `Task`'s cancellation to Rust, so calls take
an optional `CallToken` the app cancels from `withTaskCancellationHandler`;
the call's task is aborted and it throws `Cancelled`. Mentions carry
`utf16` ranges for `NSString`. Hold one handle per app. `just xcframework` builds `AsperoFFI.xcframework` (static, arm64,
macOS 26+; `UNIVERSAL=1` adds x86_64, `MACOSX_DEPLOYMENT_TARGET`
overrides) and the Swift bindings; a workflow attaches both to each
release. Not published to crates.io.

Not until needed: priority lanes in the pacer, a daemon, a SQLite
connection pool, and CLI keys anywhere but the environment (agents run
headless, where a Keychain prompt after an upgrade would hang them).
The app and the CLI keep separate caches (the sandbox puts the app's in
its container; different schema versions would rebuild each other's).

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
- **Integration**: CSL-JSON export, DOI-as-citekey builds, Zotero
  interop; an MCP adapter if ever wanted.
- **hayagriva workarounds** in `bib/src/cite.rs` (a default `<name/>`;
  spaces before punctuation tidied): drop them once a release includes
  typst/hayagriva#520 and fixes empty date-part delimiters.

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
