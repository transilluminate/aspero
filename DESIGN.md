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

- **Library first.** The CLI, the MCP server (and any future TUI) are thin
  adapters; anything useful lives in a library crate.
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
  mcp/         Model Context Protocol server: the engine's tools for agents
               (via the official SDK, `rmcp`), served over stdio by the CLI
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
- **Choosing sources** (#73): `StandardConfig::disable` (FFI
  `Config.disabled_sources`, CLI `--without` / `ASPERO_DISABLED_SOURCES`)
  takes sources by the names results give (`SOURCES`: openalex,
  crossref, datacite, doi, europepmc, openlibrary). Those are never built,
  so never asked, for lookups, checks or searches, and
  `search_providers` (FFI `searchCapabilities`) leaves them out; a search
  naming one fails with `EngineError::Disabled` (kind unsupported, "Europe
  PMC is turned off"), as `search_supports` says; citation links without
  OpenAlex likewise. An unknown name, or every source, is a setup error
  (CLI: usage, exit 2). Checks need no particular source: any one alone
  verifies (tested with Crossref alone).
- **Search** goes to one provider (named, or the first capable); filters it
  can't apply are stripped and reported as ignored. Fields (author, title,
  container) and sorts (newest, most cited) it can't apply fail
  (`Unsupported`): dropping one would widen or reorder the search
  silently. Relevance never fails; with only fields to rank by, it is the
  source's own order.
- **Since** (`SearchQuery::since`, a day): only works the source added
  on or after it, whenever published, for watching a search; the caller
  keeps what it has seen, and a page's `total` counts what's new. A
  request option, not a tree leaf, so it bounds text, fields and trees
  alike. Each source maps its own "added": Crossref `from-created-date`
  (the first deposit; `from-index-date` moves with every re-index and
  finds mostly old works), DataCite `created:[day TO *]`, Europe PMC
  `FIRST_IDATE:[day TO *]` (first indexed), ANDed with the rest, a
  tree bracketed. It is never ignored: a source without `Filter::Since`
  fails (`UnsupportedQuery`, with the provider's `since_unsupported`
  reason), as `search_supports_since` says beforehand. Without a source,
  the first searching provider with `Since` answers. OpenAlex's
  `from_created_date` and `sort=created_date` need a Member+ plan ("Plan
  upgrade required", HTTP 429, checked 2026-11-13); Open Library has no
  added date.
- **Query trees** (`SearchQuery::tree`, core `query`): all, any and none
  of terms (keywords, title, author, container, abstract, author id,
  identifier), filters and relations, instead of the keywords and fields. The engine
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
- **Identifier terms** (#65) name works in a tree, beside anything else:
  the value is any `WorkId` (else a bad query), and each source takes
  the kinds its syntax can find, refusing others with the reason, so
  `search_supports` and search agree. Checked live 2026-11-14: OpenAlex
  `doi:` (arXiv as `10.48550/arxiv.…`), `ids.pmid:`, `ids.pmcid:PMC…`
  (records hold the bare number; both forms match) and `openalex:`, ORed
  with `|` within one key and negated with `!`, but any across keys can't
  be said, and there is no ISBN filter. Crossref `doi:` filters: repeated
  ones OR, so an any of DOIs is its one any, and all of two (or none) is
  refused; it holds no arXiv DOIs. DataCite `doi:"…"` (arXiv by its DOI,
  stored lowercase). Europe PMC the lookup clauses (`DOI:"…"`,
  `(EXT_ID:… AND SRC:MED)`, `PMCID:`, `BOOK_ID:`), full boolean; its
  arXiv preprints carry no DOI. Open Library `isbn:` in `q`, with
  author and title alike. `searchCapabilities` lists the field for any
  source that takes some kind; which kinds, only the reasons say.

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

## Citation links

Who cites whom, read from **OpenAlex's `referenced_works`**, so nodes are
OpenAlex work ids (`references`, `analyse`; `engine/src/graph.rs`, shapes
in `aspero_json::graph`). Phases (#60): G1 references and the analyses
below; G2 related works (co-citation and coupling, capped and sampled,
from `cites:` pages); G3 OpenCitations as a free source of citing works.

- **Coverage.** OpenAlex gives `referenced_works: []` (and a count of 0)
  both for works it has no reference data for (books such as the DSM-5,
  publishers that deposit none, older work) and for works citing nothing,
  and no other field tells them apart (checked 2026-11: `indexed_in`,
  `has_fulltext` don't). Works citing nothing are rare, so an empty list
  is **unknown** (`SourceRecord::references` is `None`), never "none"; a
  source that can say "none" may give `Some(empty)`. Lists are complete
  (5,000 listed for a count of 5,000) and ordered by id, not as the work
  cites them.
- **Records.** OpenAlex lookups fetch whole works (no `select`), so the
  list costs no extra request; it is mapped onto the record, and the cache
  was rebuilt for it.
- **Resolving seeds** is cheap and never retried (many fail by design):
  fresh cached answers, then one batch per identifier kind (a batch of one
  is a free lookup). Kinds OpenAlex can't look up (PMCIDs, ISBNs) are
  looked up elsewhere and asked again by the DOI or PMID found; without
  one, `unsupported_id`. A seed OpenAlex has no record of by the
  identifier given is looked up as `lookup` does (Crossref, DataCite,
  doi.org, through the cache; free, asked once) and asked about again by
  an OpenAlex id, DOI or PMID the record found carries; only then is it
  `not_found`.
- **Identity.** Seeds, and works found, are one work when they share a
  DOI, PMID or arXiv ID (published records list their preprint's arXiv
  copy), else on the same title (similarity ≥ 0.9, a reply or erratum
  never the work it answers), first author and years ±1. Merged ids are
  kept; counts are over merged seeds ("cited by k of n").
- **Retracted references:** the union of the seeds' references checked
  with `openalex:A|B|…,is_retracted:true`, 100 per request ($0.0001; the
  same as a lookup batch), plus, without calls, notices already cached:
  cached records' own flags and the registry's (Crossref) notices for
  their DOIs.
- **Odd one out:** a seed citing no other seed, cited by none, sharing no
  references, is `no_links`; with unknown references it's
  `no_reference_data`, unless another seed cites it (then it isn't odd).
  Free. Nothing is odd in a set of fewer than two works.
- **Probably missing:** references counted over merged seeds, works of the
  set left out (and, after the lookup, any work sharing an identifier with
  any seed, resolved or not), at least two citing, most cited first (ties
  by id). The
  best are looked up in whole batches (one request for up to 100), so
  works that turn out to be a seed or each other (versions) drop or merge
  at no extra cost; then the best `limit`. A version cited by only one
  seed isn't seen.
- **Related works** (G2, `graph/related.rs`; OpenAlex only: OpenCitations
  lags 6–9 months and truncates long lists). From the works citing the
  set: `cites:A|B|…` filters, 200 a page ($0.0001), most cited first,
  `select=id,publication_year,referenced_works`; which works of the set
  each cites is in its references.
  - *Pages.* Works cited at most 200 times (`cited_by_count`) share
    filters, packed to fill one page (most cited first, first fit), so a
    page each; others are read alone, at most 5 pages (a classic's 1,000
    most cited citing works: `capped`). At most 75 pages a set, taken off
    the works read most (the most cited first). A budget is shared out
    before reading, a page at a time (each filter's first page, then
    second pages, …; coupling's last), keeping back the lookup; filters
    are read four at a time. 25 works cited 300 times each: 50 pages.
  - *Co-citation:* the citing works read that cite a work and at least
    one work of the set, once each however many of the set they cite
    (a review citing ten seeds counts once). A seed citing another seed
    counts too.
  - *Coupling:* how many of the set's references (all its works'
    together) a work cites too, from two (one is chance), counted for every
    work whose references are known: the citing works read, the
    candidates looked up, and works citing the set's most shared
    references: one filter of those at least two works of the set cite
    (the 100 most shared), at most 5 pages, most cited first (a fixed
    sample, not counted as capped). A seed links a work by coupling when
    the work cites two of the seed's own references.
  - *Score:* each count as a share of the highest found, weighted 0.8
    co-citation and 0.2 coupling (all to one when no work has the
    other). Weighted evenly (tried on two sets, 2026-11), coupling's
    long-reference-list favourites (broad reviews, theses, AI in food
    science) outranked the field's core works; co-citation-led, a set of
    eleven sepsis trials gets its peers (VASST, SAFE, CORTICUS, TRICC,
    VISEP) and the guidelines and definitions.
  - The best 100 (per 100 of `limit`) are looked up in one batch, so works
    of the set (any shared identifier with any seed, resolved or not, or
    `same_work` with one), retracted works, drop, and versions merge
    (counts joined), as probably missing does; then the best `limit`,
    ties by id. Each says `co_cited_with`, `shared_references` and the
    `seeds` linking it.
  - Citing works dated more than a year before a work of the set they cite
    are set aside: OpenAlex gave a 2001 paper on mustard 300 references,
    2014 sepsis trials among them.
  - `incomplete` counts citing works read of those the set's
    `cited_by_count`s promise (`done` of `total`): `capped` when a work was
    sampled, `budget`, `provider_failed`.
  - Live (2026-11): eleven sepsis trials, $0.0060 (60 pages, as estimated),
    16 s, 10,647 of 15,357 citing works; eight deep-learning works with
    LeCun 2015 (90,000 citing works), $0.0046, 13 s, 8,000 of 149,057.
    Cached, a second run is free and takes a second.
- **Spend.** Analyses run cheapest first (odd one out, probably missing,
  retracted references, related works). With a budget (US dollars), each
  paid request is refused when it could take the spend past it; what was
  stopped says so: `incomplete: {kind: budget|provider_failed|capped,
  done, total}`. `estimate_only` resolves seeds from the cache and free
  lookups of one id (which the run then finds cached) and counts the rest
  exactly: requests not answerable from the cache, and without a cache
  the seeds' batches too (the run asks for them again). With related
  works it is a ceiling (`exact: false`): their citing works' pages are
  exact (from the seeds' `cited_by_count`s, given they're current), but
  coupling's pages may run out early and the lookup may be cached. A set
  of 11 works with 350 references cost $0.0005 and 3 s (2026-11).
- **Cache.** An `edges` table (work, direction `references`|`cited_by`,
  ids, complete, stored_at): references 30 days, citing works 1 day. A
  work of the set's `cited_by` row lists the citing works read (all of
  them, `complete`, or a sample), each one's references in `references`
  rows; a later run reuses a row covering what it would read. Coupling's
  pages are cached as searches (a day). The table is created with
  `CREATE TABLE IF NOT EXISTS` beside the versioned schema, so adding it
  rebuilt nothing; rebuilds drop it like the rest.

## Storage

Two SQLite stores, split by whether data can be refetched:

- **Cache** (`cache.sqlite`, platform cache dir or `$ASPERO_CACHE_DIR`):
  mapped records with raw bodies, "not found" answers, search pages,
  citation links. WAL, busy timeout, batch reads and single-transaction
  writes. No migrations: bump `SCHEMA_VERSION` on any schema *or
  record-shape/mapping* change and it rebuilds (a table added beside the
  schema, like `edges`, needs no rebuild). Lifetimes (`CachePolicy`):
  records (with their notices: retractions, corrections) 7 days; "not
  found" 1 day; search pages 1 day; references 30 days; `cited_by` pages
  1 day. Provider failures and verdicts are never cached: a failure is
  asked again next time, and a verdict is worked out again from the
  cached answers. A cached record counts only if it covers the requested
  fields. Failed providers fall back to stale entries: any cached record,
  however old, or search page; never an expired "not found". The first
  two are settings (#74): `StandardConfig::cache_policy`, FFI
  `Config.cache_found_days` / `cache_not_found_days` (defaults 7 and 1;
  0 asks every time), CLI `--cache-found-days` / `--cache-not-found-days`
  (and `ASPERO_CACHE_*`); the rest stay as they are.
- **Size** (#74): `Stats::bytes` (FFI `CacheStats.bytes`, `aspero cache
  stats`) is the database's `page_count × page_size`, the file on disk
  once the WAL is checkpointed (SQLite does so every 4 MB or so; the WAL
  isn't counted). `SqliteCache::set_max_bytes` (FFI `Config.cache_max_mb`,
  CLI `--cache-max-mb`, MB of 1,000,000 bytes, at least 1) holds it under
  a limit: after each write (two `PRAGMA` reads) and when set, a cache
  over it loses its least recently used entries (records with their
  identifiers, "not found" answers, search pages, links; never the
  `pacing` slots) until the pages in use are under nine tenths of it,
  oldest first by an estimate of each row's size, and the freed pages go
  back to the file system: new files are made `auto_vacuum = INCREMENTAL`
  and get `incremental_vacuum` (stepped to the end: rusqlite's
  `execute_batch` steps once, freeing one page); a file made before (no
  auto vacuum) is converted by one `VACUUM` the first time it's evicted
  from. Use is tracked by a `used_at` column on those four tables, added
  by `ALTER TABLE … ADD COLUMN` beside the versioned schema rather than a
  `SCHEMA_VERSION` bump, so existing caches are kept and 0.16 shares the
  file unharmed (its writes leave `used_at` empty, read as `stored_at`).
  Writes set it; reads bump it only under a limit, and only when over an
  hour old, so an unlimited cache's reads never write.
- **Fresh** (`CallOptions::fresh`, CLI `--fresh`, FFI `Patience.fresh`),
  for checking again before submitting: no cached answer is served as it
  is, however recent (lookups, "not found", searches, and the `edges`
  references and citing works `references` and `analyse` read), so the
  providers are asked; every answer is still written, replacing the
  cached one, so later ordinary calls see it. Where a provider fails, the
  cached answer is served as `stale`, never `cache`: it isn't the answer
  from now that was asked for (a record of any age, a "not found" within
  its day), so a fresh pass is never worse than an ordinary one. Pacing,
  the cross-process slots, deadlines and retries are unchanged (the FFI's
  `fresh` waits like `batch`); `offline` wins over fresh. The CLI refuses
  `--fresh` with `--no-cache` (a usage error): one refreshes the cache,
  the other leaves it alone.
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
answers from the cache only, however old; `fresh()` asks past the cache
and refreshes it (see Storage).

The engine remembers providers that can't answer, across calls: a spent
quota until it resets, rejected credentials for its life (new credentials
mean a new engine), and a **circuit breaker** (closed → open after N
consecutive failing calls → one probe after the cooldown). Refused calls
fall back like failed ones (next provider, stale cache);
`Engine::unavailable` says which providers are refusing and why. Batches
verify a few claims at once and yield each as it finishes, so a slow
claim doesn't hold up the rest, and a batch never queues so many requests
that a caller sharing the engine waits behind it.

A **daily cap** (`EngineBuilder::daily_cap`, `StandardConfig`/FFI
`openalex_daily_cap`, CLI `--openalex-cap` / `ASPERO_OPENALEX_CAP`, USD)
stops a provider's paid calls short of its own budget (#72). The day's
use is the provider's figure, `limit − remaining` from the last response
or refresh, so it counts everything using the key; once it reaches the
cap, until the reset: searches (and graph pages) fail with
`Error::BudgetCapped { cap, used, resets_at }` (kind `budget_capped`,
category throttled, exit 5) before anything is sent, falling back like
any failure (a stale cached page; the next provider in a check, so
verify carries on with Crossref and the rest); lookups go
`Capabilities::max_free_batch` IDs at a time (OpenAlex: 1, free) rather
than in paid batches. After `resets_at` the use is unknown again. Unknown
use: with a key, one free refresh (`/rate-limit`, no retries, 5 s; one at
a time, and not again for a minute after one that couldn't say) decides
the first paid call, so a day already past the cap stops at once;
without a key the call is taken as under the cap and the cap applies from
its response on. A call is checked before it's sent and its cost known
only after, so the last call, or calls in flight together, can pass the
cap by their own cost. The cap is no part of health: free calls go on,
and `unavailable` doesn't list it.

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
  key: $1/day ($0.10 without). Lookups by ID are free; filter batches
  $0.0001; searches $0.001. Every response, free ones included, carries
  `x-ratelimit-cost-usd`, `-limit-usd` (the daily allowance),
  `-remaining-usd`, `-prepaid-remaining-usd` and `-reset` (seconds to
  midnight UTC); `Usage` and the engine's `Budget` keep them (`limit`,
  `prepaid_remaining`). `GET /rate-limit` says the same as JSON
  (`daily_budget_usd`, `daily_remaining_usd`, `prepaid_remaining_usd`,
  `resets_at`) for free (`daily_used_usd` unchanged across calls, no cost
  header; checked 2026-11-14), but only to a key (401 "Authentication
  required" without; Bearer works): `Engine::refresh_budgets` asks it,
  and asks nothing without a key. Its body echoes the key's first and
  last three characters, masked in the fixture.
  404s are HTML. Lists take up to 200 a page (`per-page`); filters OR up to
  100 values; `select` trims works to the fields named (citing works:
  `id,publication_year,referenced_works`, about 3 KB a work). Batch
  filter results come back unordered with missing IDs
  absent. A DOI re-registered when a journal changed publisher
  (`10.1197/j.aem.2003.09.015`) still finds the work, whose record carries
  only its new DOI: alone, the lookup answers with it; in a batch, a
  record no key claims answers the one key that found nothing, or each
  such key is asked alone (free). arXiv works sit under their DataCite DOI. `conference-paper` is a
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

## Agents (MCP)

`aspero-mcp` makes the engine's work into MCP tools (#58). Two layers:

- **`Tools`**, transport-free: each tool is a function over the engine
  taking its arguments as JSON (serde types whose schemars schemas are the
  input schemas) and answering with structured JSON in `aspero-json`'s
  shapes plus a few lines of text, or a `ToolError` (kind and message).
  Tools: `search` (a query tree, the `QueryTree` serde type in
  `aspero-json` that `search --query` parses too, or keywords),
  `search_supports`, `lookup`, `verify` (one citation or up to 25, results
  in input order), `identify`, and `citing_works` / `references` (a search by the
  relation alone, at the first source with relations). Lean records unless
  `full`; search pages by `cursor`. Output schemas are written by hand
  (the shapes are built as JSON) and left open beyond the keys agents rely
  on; tests check every result against them.
- **The adapter**, the only code that sees `rmcp` (the official SDK,
  pinned exactly): `ServerHandler` over `Tools`, served on any byte stream
  (`serve`) or stdio (`serve_stdio`, which `aspero mcp` runs). Tool
  failures are tool errors (`isError`) the agent reads, unknown tools
  protocol errors. Descriptions and the server's instructions say results
  are third-party data, not instructions.

A **`Session`** is one connection: its `Caller` (empty over stdio; tokens
will go there) and a spend cap in USD (`--budget`, default $0.25). Tools that may spend are refused once the session has spent it,
checked before each call; a call's cost is known only after it, so the
last one can overshoot by its own cost. Spend is the engine's reported
usage (OpenAlex's `cost`), as the CLI totals it; every result carries the
call's `usage` and the session's `spent_usd`, `budget_usd` and `left_usd`.
`search_supports` is offline and never refused.

Later phases (#58): Streamable HTTP on loopback behind a feature, with
bearer tokens, for embedding; host-provided tools; progress notifications
(a batch's results as each finishes).

## Apps (FFI)

`aspero-ffi` (UniFFI) gives apps one `Aspero` handle: `lookup`, `verify`,
`verify_each` (a callback per result as it finishes), `search` (with
`searchCapabilities` saying what each source can search by, and
`searchSupports` whether it can search a query tree, and optionally
`since` a day), offline
`scan`, `split_references` and `split_lines` (laid-out lines with
indents, from PDFKit), `references` and `analyse` (citation links), and
for settings `budgets` (as last
reported: free), `refresh_budgets` (OpenAlex's `/rate-limit` with a key,
free; nothing asked without one), `check_openalex_key` (one free lookup), `cache_stats`,
`clear_cache` and `unavailable`. Network calls take a `Patience` (batch,
interactive, offline, fresh).
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
- **Discovery**: related works (co-citation, coupling: citation links
  G2), OpenCitations (G3), watches ("new since last run"), history and
  sessions.
- **Integration**: CSL-JSON export, DOI-as-citekey builds, Zotero
  interop.
- **hayagriva workarounds** in `bib/src/cite.rs` (a default `<name/>`;
  spaces before punctuation tidied): drop them once a release includes
  typst/hayagriva#520 and fixes empty date-part delimiters.

## Open questions

- Whether to cap OpenAlex spend per CLI command, as MCP sessions are
  (`--openalex-cap` caps the day, not a command).
- The MCP session budget's default (provisional $1.00), and a per-token
  cap, before Streamable HTTP.
- Whether a lookup should ever hedge (ask two providers at once) for
  latency.

## Conventions

- Licence MIT OR Apache-2.0; edition 2024 on current stable, no MSRV yet.
- Conventional commits; small, focused changes; clippy and rustfmt clean.
- Tests: unit tests beside code, property tests for identifiers, fixture
  replay for providers, paused-time tests for retries, pacing and
  concurrency, the live benchmark for scoring.
