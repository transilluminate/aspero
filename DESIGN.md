# Design

An index to the code and the decisions it can't show. The README covers
using aspero; each module's `//!` comment holds its own rules; git and the
CHANGELOG hold the history.

## Purpose

Agents asked to research literature recall identifiers from memory (often
wrong or invented) or try web search, which blocks bots. Scholarly APIs are
open to programs. aspero is a polite, cached, agent-ergonomic library and
CLI over them: find works, resolve messy citations, and **verify that cited
works exist and match what's claimed**. Agents first, then humans.

## Principles

- **Library first.** The CLI, the MCP server and the FFI are thin adapters;
  anything useful lives in a library crate.
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
  generic seams; they are never dependencies. Those seams (full text, a
  durable store of derived data) are designed with their first producer in
  hand, not ahead of it.
- **Work identifiers are a contract.** Callers store canonical `kind:value`
  strings durably and join on them, so a change to an identifier's
  canonical form (its normalisation or display) or to the ranking in
  `id::canonical` (`crates/core/src/id/mod.rs`) is a breaking change: it
  needs a migration story (old form to new, for stored keys) and a minor
  version, not only a changelog line. Property tests pin that display
  re-parses to itself.

## Architecture

```
crates/
  core/        identifiers, Work, Claim scoring, Provider trait, query trees,
               page reading, HTTP seam. No I/O.
  providers/   a crate per source (openalex, crossref, datacite, doi,
               europepmc, openlibrary), each behind an engine feature
  store/       disposable SQLite answer cache; the benchmarks' HTTP tape
  bib/         BibTeX, RIS, CSL-JSON, .docx ↔ claims and works; CSL styles
  engine/      routing, caching, retries, pacing, spend caps,
               resolve/verify/check, citation links
  json/        the JSON contract, shared by every front end
  mcp/         MCP server (`rmcp`): stdio, and Streamable HTTP for apps
  cli/         clap front end
  ffi/         UniFFI bindings for apps (Swift), shipped as an .xcframework
  notices-gen/ third-party notices for the FFI and CLI (not published)
bench/         known-answer citations, red team, navigability dogfood
scripts/       fixture recorders, red team and tape tools, the gates
```

### Where things live

Identifiers and text, `crates/core/src/`:
- `id/mod.rs`: kinds, parsing, canonical display, `WorkId`,
  `id::canonical` (which identifier names a work, in rank order); a file
  per kind (`doi.rs`, `arxiv.rs`, `isbn.rs`, `pii.rs`, `numeric.rs` for
  PMID/PMCID/NBK, `s2.rs`); the grammar seam `id/syntax.rs`.
- `id/scan.rs` (`scan`): identifiers in documents (`aspero scan`).
  `references.rs` (`split_references`): a pasted list split, after
  `fold.rs` cleans PDF text. `tagged.rs` (`template`): Wikipedia
  `{{cite journal}}` templates, RIS and EndNote pasted as text.
- `webpage/`: `mod.rs` (`claim_from_page`), `html.rs` (the tag scanner),
  `jsonld.rs`, `title.rs` (site suffixes off: `strip_suffix`,
  `is_title`), `web.rs` (the web page record, `normalise_url`).
  Publishers' sites and DOI prefixes: `site.rs` (`PUBLISHERS`).

Scoring, pure, `crates/core/src/`:
- `matching/claim.rs`: what a claim asserts (`Claim`, `cited_container`,
  `cited_years`, `byline`, `cited_venue`). `matching/assess.rs`
  (`assess`): one record scored against a claim, every cap in order.
  `matching/caps.rs`: the numbers (weights, penalties, caps, `VERIFIED`,
  `PLAUSIBLE`, `JOURNAL_MISMATCH_CAP`, …).
- `matching/`: `titles.rs`, `authors.rs`, `text.rs` (fields found in free
  text), `derivative.rs` (replies, errata), `preprint.rs`, `publisher.rs`
  (a book's publisher cited, `publishers_differ`), `same_work.rs`
  (`same_work`: two records one work).
- `publication/`: `journal.rs` (abbreviations, `other_journal_read`,
  sister journals), `cited_journal.rs` (`cited_journals`: the journal a
  free-text reference cites), `locator.rs` (volume and first page by
  style), `conference.rs` (`CONFERENCES`, `proceedings_agree`), `bibtex.rs`.

Engine, `crates/engine/src/`:
- `lib.rs` (`EngineBuilder`, `CachePolicy`); `standard.rs`
  (`StandardConfig`, `standard`, `SOURCES`: the built-in providers wired).
- `lookup.rs` (the fallback chain), `inputs.rs`, `inflight.rs` (shared
  calls in flight); `search.rs` (routing); query trees in
  `crates/core/src/query.rs` (`validate`, `check_supported`,
  `render_lucene`) and `crates/json/src/query.rs` (`parse_query`).
- `resolve/`: `gather.rs` (candidates), `merge.rs` (`Accumulator`,
  `absorb`: records of one work into a candidate), `rank.rs` (`rank`,
  `prefer_site`). `verdict.rs` (`judge`), `batch.rs`, `check.rs`,
  `identify.rs`; `enrich.rs` (notices, open access), `registry.rs`,
  `versions.rs`, `abstracts.rs`.
- `graph.rs` and `graph/` (citation links): `seeds.rs`, `spend.rs`,
  `missing.rs`, `retracted.rs`, `references.rs`, `related.rs`,
  `related/plan.rs`, `related/score.rs`, `types.rs`.
- `retry.rs`, `health.rs` (breaker, spent quotas); `cap.rs` (`reached`,
  `spendable_today`); `budget.rs` (`Budget`, `PrepaidDay`,
  `credit_part`); `transport.rs`
  (HTTP client, pacing, `TapeClient`).

Storage, `crates/store/src/`: `lib.rs` (`SqliteCache`), `schema.rs`
(`SCHEMA_VERSION`), `cache.rs` (the `Cache` trait), `edges.rs`,
`evict.rs`, `pacing.rs`, `prepaid.rs`, `spend.rs`, `tape.rs` (`TapeKey`).

Front ends:
- JSON, built by hand with `json!`: `crates/json/src/` (`verify.rs`:
  `verification`, the verify result's fields; `work.rs`, `lookup.rs`,
  `check.rs`, `graph.rs`, `claim.rs`, `scan.rs`).
- CLI `crates/cli/src/`: `args.rs` (flags and help), `cmd/` (a module per
  subcommand), `output.rs`, `grid.rs` (`Grid`: tables, column widths),
  `exit.rs`, `report.rs` (`verify --report`).
- MCP `crates/mcp/src/`: `tools.rs` (`Tools`), `args.rs`, `schema.rs`,
  `session.rs`, `server.rs`, `http/` (`mod.rs`; `auth.rs`: bearer keys,
  `authenticate`; `handler.rs`; `host.rs`).
- FFI `crates/ffi/src/`: `lib.rs` (`Aspero`), `config.rs`, `calls/` and
  `records/` (a file per domain; `records/verify.rs`: `Verification`),
  `mcp.rs` (`McpServer`, `McpHost`).
- Bibliographies `crates/bib/src/`: `lib.rs` (BibTeX), `ris.rs`,
  `csl.rs`, `docx.rs`, `write.rs`, `cite.rs` (styles).

### Seams and contracts

- **Provider trait** (`crates/core/src/provider.rs`; its doc lists the
  rules the engine relies on): defaulted `Unsupported` methods plus
  declared `Capabilities`. Providers map one source's requests and
  responses; they never merge, cache or retry.
- **Transport seam** (`crates/core/src/http.rs`): the engine injects the
  `HttpClient`; responses carry `received_at`, so core reads no clock.
  Tests replay curl-recorded fixtures (`ReplayClient`): no network.
- **Search modes**: `Keywords`, or `Citation` (a whole reference) where a
  provider has a citation matcher (Crossref). Scores compare only within
  one provider. **Cache trait**: batch-first; the engine owns freshness.

### Routing and concurrency

- **Lookups are a fallback chain** by priority: OpenAlex (10) → Crossref
  (20) → DataCite (25) → doi.org (30) → Europe PMC (35) → Open Library
  (40), each asked only about IDs still missing and of kinds it accepts.
  Asking all at once would multiply requests for the usual first hit.
- **Independent work is concurrent** (ID lookups with citation matchers,
  then title searches; batches verify 4 claims at a time), applied in a
  fixed order so merging is deterministic. Pacing holds per provider, so
  concurrency overlaps different providers' waits.
- **Sources off** (`StandardConfig::disable`, CLI `--without`) are never
  built or asked. Any one source alone can verify.
- **Search goes to one provider** (rankings aren't comparable): the one
  named, else the first that can search the whole query. Filters it can't
  apply are stripped and reported; fields and sorts it can't apply fail,
  since dropping them would widen or reorder the search silently; nothing
  in a query tree or `since` is ignored. `search_supports` is the same
  decision, offline (`crates/engine/src/search.rs`). Query trees are
  checked in core and translated or refused by each provider's `express`.

## Identity and data model

- **Identifiers**: DOI, arXiv, PMID, PMCID, OpenAlex, S2 paperId and
  CorpusId, ISBN, NCBI Bookshelf (NBK), Elsevier PII; bare, `kind:` and
  URL forms parse to one canonical value. Bare digits are refused
  (ambiguous) but for checksum-valid ISBN-13s. DOIs drop trailing `.,;:`,
  and `lookup` retries a not-found one with it.
- **Publishers' article URLs** read as the identifier they show (a DOI in
  the path, Nature's `/articles/<suffix>`, Elsevier PIIs, arXiv). A PII is
  looked up only at Crossref, then its DOI through the chain. **Not
  read**, as no free source maps them: JAMA's `/fullarticle/<n>`, Oxford
  Academic's `/article/<vol>/<issue>/<page>/<id>`, OpenReview ids; for
  those a caller hands over the page.
- **Pages** (`webpage::claim_from_page`, `verify --html FILE --url URL`):
  a claim from an article page's markup, address and dragged title. Pure:
  aspero never fetches a page. The journal is only the page's metadata,
  never guessed from the host (a wrong guess caps `verify` at plausible);
  the host is the claim's `site`, a publisher hint (Sites, below).
- **Scholarly work or web page** (#119): a page is a work's when it gives
  an identifier, citation tags or a scholarly JSON-LD type, is on a
  publisher's site, or comes without markup. Any other titled page is a
  web page: no claim (an app mustn't report it "not found") but a
  `WebPage` record cited as itself (CSL `webpage`, with the caller's
  access date: core has no clock). Titles lose only what evidence marks
  as the site's name, never by per-site rules: nothing downstream edits
  them.
- **Scanning** (`id::scan`): every mention, where, and whether written as
  a web address (`from_url`); which matter is the caller's call. Shapes
  prose imitates (bare digits, `W1`, `1706.03762`) count only labelled,
  prefixed or as URLs. ~1 GB/s on prose (`just scan-bench`). A reference
  isn't prose: `Claim::from_text` takes its one bare arXiv-shaped number,
  after a colon, as a table's cell or last, for its ID (models write
  them so), since nothing else in it could be that number.
- **Work** (`crates/core/src/work.rs`): CSL-shaped, all-optional, plus
  `updates` (retractions, corrections, newer versions), `update_to`,
  `other_years`, `open_access`. Guideline editions aren't linked in any
  source, so aren't followed.
- **Registry checks**: Crossref registered most DOIs, so its record is the
  reference; with `--full` a record contradicting it is reported, not
  silently replaced (OpenAlex has merged two corrections into one record).
- **Merging** only on a shared identifier (arXiv versions are one work).
  Fuzzy matches may *suggest*, never merge: a wrong merge corrupts, a
  duplicate merely annoys. Values sources disagree on are made the same
  on every merged record (#81, `resolve/merge.rs`), so a work doesn't
  change with the source that won: cited-by is OpenAlex's (the broadest
  index, and first for lookups, so `verify` and `lookup` agree).

## Resolve, verify and check

Candidates are gathered cheapest first (`resolve/gather.rs`): claimed
identifiers and Crossref's citation matcher (free), then at most two
**title** searches ($0.001 each at OpenAlex). Bare titles rank far better
than whole references, so the title is located *inside* the reference
from the matcher hits' titles (`shared_title`), else from its shape
(`probable_title`). A hit's run is the title only where it stands where
the reference's shape puts the title (`gather.rs`: `beside_title`): a
hit sharing a publisher's words, or a review titled with the whole
citation, would otherwise take the title's search. `verify` stops early only on a decisive match vouched
for by the claimed identifiers or two providers: a digest or reprint can
match exactly while only a later search finds the original.

Scoring looks for the record's fields *in* the claim rather than parsing
citation styles. Each rule came from a real failure; its reasons and
examples are comments at the rule in `matching/assess.rs`, its numbers in
`matching/caps.rs`:

- **Wrong year halves confidence** (`YEAR_MISMATCH_PENALTY`): lookalike
  re-deposits carry the real title and authors under another year. Online
  and print years count, as do other editions'. A year in the title or a
  link is not the year cited ("Cancer statistics, 2020 … 2018").
- **Different authors halve confidence** (`AUTHORS_MISMATCH_PENALTY`).
  The first author alone matches ("Vaswani et al."); an invented
  co-author caps at plausible (`STRANGERS_CAP`, `authors.rs`).
- **One field can't verify** (`SINGLE_FIELD_CAP`); **title and year alone
  stay plausible** (`UNCORROBORATED_CAP`): generic titles match by chance.
- **Titles compare on content words**; a dropped subtitle still matches,
  words added or replaced cap at plausible (`EXTENDED_TITLE_CAP`,
  `titles.rs`), and a year in place of the title's own is another in a
  series (`OTHER_IN_SERIES`).
- **Replies, errata and comments are not the original**
  (`derivative.rs`), nor the original a correction cited.
- **Wrong volume or first page caps at plausible**
  (`LOCATION_MISMATCH_CAP`; `publication/locator.rs`); journal, volume and
  first page agreeing verify alone (`PLACED`): running headers.
- **The wrong journal caps a journal article at plausible**
  (`JOURNAL_MISMATCH_CAP`, #80): a common error and a tell of invented
  references. The journal is read from free text only where it can be
  told (`cited_journal.rs`); names match by rule, not a list
  (`journal.rs`). A sister journal ("JAMA" for JAMA Internal Medicine) is
  another; a preprint is another version; a record with no place can't
  confirm one (`UNPLACED_CAP`).
- **Another venue is plausible**: another conference (`conference.rs`),
  an article cited as an edited book's chapter, a chapter cited as an
  article (`claim.rs`: `cites_edited_book`; `locator.rs`: `cites_issue`).
- **An identified work with other authors, another edition or version**
  (`EDITION`, `VERSION`): plausible, an article reprinted later as a
  chapter too (the free sources may hold only the reprint). A book cited
  with another publisher alone is an imprint or a reissue; with another
  year than any of its own too, plausible (`OTHER_EDITION_CAP`), the
  publisher and year named. "RETRACTED:" markers are ignored.

**Sites** (#98, `crates/core/src/site.rs`). A claim's `site` is evidence
of the publisher, never a claimed field, and only counts for a claim
without identifiers on a site in `PUBLISHERS` (registrable domain, no
Public Suffix List; prefixes checked against Crossref's `/prefixes`).
Crossref is asked for the title among the publisher's DOI prefixes
(free), and the first-ranked gives way to the publisher's best within
`SITE_MARGIN` (0.05: less than any whole field's disagreement costs;
`rank.rs`: `prefer_site`). `chosen_by_site` says when the site decided.

**Verdicts** (`verdict.rs`): verified (≥ 0.85), plausible (≥ 0.6),
mismatch (claimed ID missing or another work's; or same title with
authors *or* year right and the other wrong), not_found, unverifiable
(providers failed); a retraction is noted whatever the verdict. What each
kind of citation should get: the table in
[bench/redteam/README.md](bench/redteam/README.md) ("Verdict policy").
**`check`** (`check.rs`) adds findings (add_doi, replace_id, year_differs,
duplicate, updated) with severities; `--fail-on` sets the exit-9 bar.

**Accuracy**: the promise, the red team, the release gate and results:
[docs/accuracy.md](docs/accuracy.md). Wrong verdicts come in by the issue
form (`verify --report`) and become red-team cases
(`scripts/issue-to-case.py`). **Benchmark**: `bench/citations.jsonl`, run
live by `just bench` through the HTTP tape (~$0.035 once, then free); add
a case for every failure mode found in the wild. **Identify**
(`identify.rs`): no identifier on a first page is taken on trust (pages
print related articles, data, references); its title or first author
must be on the page too.

### HTTP tape

A developer tool, not in `--help`: `ASPERO_HTTP_TAPE=<file>` puts a
`TapeClient` (engine `transport.rs`, store `tape.rs`) in front of every
provider, so benchmark and red-team runs pay for each distinct request
once, ever, and replay offline (`ASPERO_HTTP_TAPE_MODE=replay-only`).
`just tape-stats` and `scripts/tape.py` inspect, export and merge tapes.
It records 2xx and meaningful 404s, never 429s, 5xx, a spent budget or
`/rate-limit`; holds no credentials or contact addresses (`TapeKey`), so
it can be shared; and replays free (no spend, no cap moved). Under a tape
identifiers are looked up one per request (`EngineBuilder::taped`): a
batch's key holds every ID in it, so a code change moving which claims
carry identifiers missed every later batch. Splitting batch responses
per ID at the tape was rejected: it needs each source's response format
and ID matching in the transport (OpenAlex finds works by DOIs their
records no longer carry), for no saving. Each taped run starts from an
empty answer cache, so runs differ only by code.

## Citation links

Who cites whom, from **OpenAlex's `referenced_works`**, so nodes are
OpenAlex work ids (`references`, `analyse`; `crates/engine/src/graph.rs`).
Phases (#60): G1 references and analyses; G2 related works; G3
OpenCitations for citing works (not built).

- **Coverage.** OpenAlex gives `referenced_works: []` both for works it
  has no reference data for (books, older work) and for works citing
  nothing, with no field telling them apart. The latter are rare, so an
  empty list is **unknown** (`SourceRecord::references` is `None`).
- **Analyses**, cheapest first: odd one out and probably missing
  (`graph/missing.rs`, free), retracted references (`graph/retracted.rs`,
  $0.0001 per 100), related works (`graph/related.rs`).
- **Related works** (OpenAlex only: OpenCitations lags 6–9 months and
  truncates): co-citation and coupling from `cites:` pages, weighted 0.8
  and 0.2, each citing work counting 1/√(its references)
  (`related/score.rs` says why). Without a key, OpenAlex refuses cursor
  paging past page one, so `Capabilities::max_pages` is 1. Live:
  eleven sepsis trials, $0.0060, 16 s.
- **Spend** (`graph/spend.rs`): with a budget each paid request is refused
  when it could pass it; `incomplete` lists every stop. `estimate_only`
  counts requests exactly (a ceiling with related works).

## Storage

Two SQLite stores, split by whether data can be refetched.

- **Cache** (`cache.sqlite`, platform cache dir or `$ASPERO_CACHE_DIR`):
  records with raw bodies, "not found", search pages, citation links. WAL
  and a busy timeout, so agent processes share it. No migrations: bump
  `SCHEMA_VERSION` on any schema *or record-shape/mapping* change and it
  rebuilds; tables and columns added beside the schema need none.
- **Lifetimes** (`CachePolicy`): records (with their notices) 7 days, "not
  found" 1 day (both settings; 0 asks every time), search pages 1 day,
  references 30 days, citing-work pages 1 day. Failures and verdicts are
  never cached. A record counts only if it covers the requested fields. A
  failed provider falls back to stale entries, never an expired "not
  found"; `--fresh` asks past the cache and still serves the cached
  answer, marked `stale`, where a provider fails.
- **Accounting tables** survive rebuilds, `clear` and eviction: `pacing`,
  `mcp_spend` (MCP keys' spend per UTC day), `prepaid_day`.
- **Size** (`--cache-max-mb`): least recently used out first; reads mark
  use only under a limit, so an unlimited cache's reads never write.
- **Durable store** (`aspero.sqlite`, data dir; not built): history and
  derived data keyed by `(work id, kind, producer, version)`, with
  forward-only migrations.

## Resilience

Per-provider `ResiliencePolicy`: pacing, attempt timeout, full-jitter
retries stretched to `Retry-After`, an overall deadline. Errors group by
what the caller should do (`is_retryable`, `trips_breaker`, `category` →
exit codes). One engine serves many callers: lookups in flight are
joined, each caller sets its patience (`CallOptions`), and providers that
can't answer are remembered (`health.rs`: a spent quota until reset,
rejected credentials for the engine's life, a circuit breaker) and fall
back like failures. Pacing is **shared across processes** through the
cache file's `pacing` table, so parallel agents keep to one rate, even
with `--no-cache`.

### Spending policy

OpenAlex is the only paid source. Its day's use is its own figure (`limit
− remaining`, from every response or a free `/rate-limit`), so it counts
everything using the key. Two caps (`crates/engine/src/cap.rs`): the
**allowance cap** (`--openalex-cap-usd`) on the free daily allowance, and
the **credit cap** (#110, `--openalex-credit-cap-usd`, default $0.25, 0
never; no unlimited setting) on prepaid credit, which OpenAlex spends only
once the allowance is used up. The allowance cap counts everything on the
key; the credit cap only the credit **spent here** (#136): charged to
calls made through engines sharing this cache file. OpenAlex gives one key
per account, shared by a researcher's scripts, test harnesses and apps, so
a credit cap counting the key would stop a caller that spent nothing,
while the allowance cap's job is leaving the rest of the allowance to
those other tools. Before a paid call (`cap::reached`):

1. allowance used today < the allowance cap: go, on the allowance;
2. allowance cap ≤ used < the allowance: stop (the rest is held back for
   other tools);
3. the allowance all gone (by anything) and credit spent here today < the
   credit cap: go, on credit;
4. the allowance gone and credit spent here today ≥ the credit cap: stop
   until the reset.

So with an allowance cap below the allowance, a caller never takes the key
into credit itself; only other tools can. A stop is `BudgetCapped` (exit
5); searches fall back like any failure and lookups go one free ID at a
time. With the day's use unknown and a key, one free `/rate-limit`
decides the first paid call. Overshoots: a call's cost is known only
after it, and others' spending shows only in the next response.

**Credit spent here** is each paid response's cost (`x-ratelimit-cost-usd`)
once the allowance is gone, added up per budget day in the cache file's
`credit_here` table (`PrepaidDays::add_credit_here`; kept a week, through
`clear`, eviction and rebuilds, read again before each check, so every
process sharing the file agrees). A cost is credit once the allowance is
gone: its response says none left, or one did earlier today (a stale
server may say otherwise). Of the call that used the allowance up, only
the part past the least left seen before it counts; with none seen, all
of it, its allowance part too; other tools' spend since the last sighting
counts that one call short (`budget::credit_part`). Replays cost 0:
nothing is added.

**Credit spent today** on the key isn't reported, so it's told from the
prepaid balance (`budget.rs`, `PrepaidDay`; in `prepaid_day`, so every
process agrees); `aspero budget` shows it beside the credit spent here.
Header balances can't be trusted one by one: OpenAlex's servers
lag by minutes (9.84 and 9.66 within a second, 2026-11-16), and reading
each rise as a top-up once counted $5.33 for $0.34 spent. So a header
only lowers the balance; only `/rate-limit` sets it, and only a rise in
the credit bought it reports is a top-up. A day first seen part-way
through undercounts: accepted. `Engine::spendable_today` follows the four
cases; over the search price it is `aspero budget`'s "searches left".

## Provider notes and gotchas

Fixtures are recorded with the provider's *exact* parameters (`just
record <provider> [name]`) and tests assert them: a fixture recorded with
other parameters once hid a ranking bug. Each provider's `lib.rs` doc
has its API's quirks; these shape the design:

- **OpenAlex.** Key as `Authorization: Bearer` (out of URLs). Free key
  $1/day ($0.10 without). Lookups by ID free; filter batches (≤100 values)
  $0.0001; searches $0.001, the only real cost. Every response carries
  `x-ratelimit-*` cost, allowance, remaining, prepaid and reset headers;
  `GET /rate-limit` says the same free, to a key only. A failure is a
  spent budget (`QuotaExhausted`) when the body says `not_enough_credits`
  or neither allowance nor credit is left. Batch results come back
  unordered, missing IDs absent. arXiv works sit under their DataCite
  DOI. Filters only AND; `search=` and `.search` filters take boolean
  queries, `|` ORs a filter's values, `!` negates one
  (`providers/openalex/src/tree.rs`). No journal-name filter: a container
  search finds the sources so named ($0.001) and ORs the best 100.
  `sort=relevance_score:desc` is refused without a search, so never sent.
  `from_created_date` needs a Member+ plan: no `since`. Its auth page
  carries agent-directed text about creating keys via API: aspero never
  does; keys come from the user.
- **Crossref.** `mailto` puts requests in the polite pool (10 req/s
  single, 3 req/s lists, 3 concurrent). **Cursor paging drops relevance
  order** unless `sort=score&order=desc` is sent. Lookalike re-deposits
  exist (2025 copies of "Attention Is All You Need" with the real
  authors). Field queries rank rather than require, so a journal term
  goes alone in a tree. No DataCite DOIs (arXiv); subtitles apart. Top
  matches can be derivative ("Faculty Opinions recommendation of X").
  Elsevier PIIs are `alternative-id`s, found only compact and upper case.
  `since` is `from-created-date` (`from-index-date` moves on re-index).
- **DataCite.** arXiv, Zenodo, data and software. **Relevance only with
  page-number paging** (a cursor sorts by creation date). No newest-first,
  no container search (`container.title:` matches nothing).
- **doi.org.** Content negotiation resolves any registered DOI through its
  agency (a Zenodo DOI minutes old); its 404 is authoritative. One DOI per
  request, so it sits after the batch-capable providers.
- **Europe PMC.** PMIDs, PMCIDs and NBK IDs (the only provider for them),
  biomedical abstracts, "Retracted Publication" flags. Cursor paging keeps
  relevance; full Lucene.
- **Open Library.** Books. Some ISBNs carry unrelated records, so scoring
  decides, not the identifier. A query that only excludes fails (500).
- **DBLP.** REST API and mirrors sit behind a proof-of-work bot challenge;
  we don't solve it. SPARQL has no text index (title search a 2–9 s scan
  of ~7M titles), so DBLP is no search provider; key lookups take ~0.1 s.
- **Semantic Scholar** (not built; key pending). Keyless requests mostly
  429 from an AWS gateway with no `Retry-After`; a fake key gets 403.

## Agents (MCP)

`aspero-mcp` (#58): **`Tools`** (`crates/mcp/src/tools.rs`) are
transport-free functions over the engine, JSON in (serde types whose
schemas are the input schemas) and `aspero-json`'s shapes plus a few lines
of text out; `server.rs` is the only code that sees `rmcp` (pinned
exactly). Output schemas are hand-written and open beyond the keys agents
rely on; descriptions say results are third-party data, not instructions.
A **`Session`** (`session.rs`) has a spend cap (`--budget`, $0.25): tools
that may spend are refused once it's spent, and calls in flight hold an
estimate so calls at once overshoot no more than one call can.

### Streamable HTTP and host tools (P1, P2)

For apps that embed aspero: `aspero_mcp::http` and, over it, the FFI's
`McpServer` and `McpHost` (`mcp` feature, on in the xcframework), serving
through the app's own `Aspero` handle (no second engine) the engine's
tools and the app's. Transport, auth order, scopes, notifications,
budgets and activity: `crates/mcp/src/http/mod.rs`; the FFI surface:
`crates/ffi/src/mcp.rs`. Loopback only, on the app's fixed port;
sessions required, so budgets can't be dodged; keys minted by the app,
held as SHA-256 hashes and compared in constant time, with scopes and a
daily budget kept in `mcp_spend`. Host tools cost $0 and get 30 s; a
timeout says the outcome is unknown (a retry could write twice).

**Threat model.** For agents on the same machine, acting for the user.
Loopback only: nothing off the machine connects. Every request needs a key
the app minted and can revoke; aspero holds only hashes (the app keeps
tokens in the Keychain). Browsers are the remaining local attacker: a page
can send requests to loopback, but sends an `Origin` (refused) and, with
DNS rebinding, its own host in `Host` (refused); it can't read a token.
Other local processes running as the user could read the app's data
anyway, so a key bounds what an agent may do (scopes, budgets), not who
the user is. No tool deletes anything; writes go only through the app's
`writeHistory` tools, which it can refuse. Next (P3): progress
notifications and resources.

## Apps (FFI)

`aspero-ffi` (UniFFI) gives apps one `Aspero` handle
(`crates/ffi/src/lib.rs`; `calls/` and `records/`, a file per domain).
Records mirror the engine's types, converted at the boundary so the
engine stays free of UniFFI; reasons and failures are data as well as
English, so the app can localise them. The app links its own copy, so
typed records cost no compatibility, unlike the CLI's JSON. The app
passes everything in (contact, keys, cache directory); nothing comes from
the environment. Cancellation is an explicit `CallToken`: UniFFI's Swift
bindings (0.32) don't pass a `Task`'s cancellation to Rust. `just
xcframework` builds the static `AsperoFFI.xcframework` and Swift
bindings, attached to each release. Not on crates.io. Third-party notices
ship inside it and the CLI (`crates/notices-gen`; run `just notices`
after a version bump or dependency change).

Not until needed: priority lanes in the pacer, a daemon, a SQLite
connection pool, and CLI keys anywhere but the environment (agents run
headless, where a Keychain prompt after an upgrade would hang them). The
app and the CLI keep separate caches (the sandbox puts the app's in its
container, and different schema versions would rebuild each other's).

## Future

- **S2 provider** when the key arrives, with precomputed **SPECTER2**
  embeddings (local SPECTER2 only for works S2 lacks, same base model *and
  adapter* so vectors share a space).
- **Full text** via a text-extractor seam, from legal OA PDFs.
- **`check --fix`**: suggested DOIs and corrections written to the .bib.
- **Pages over MCP**, once agents hold pages rather than addresses.
- **More sources**: DBLP keys, arXiv, Unpaywall, OpenCitations, NASA ADS,
  INSPIRE-HEP, HAL, CORE.
- **Re-normalising cached records** from raw bodies when a mapping
  changes, instead of a rebuild.
- **Configuration**: a TOML config (providers, per-provider resilience).
- **hayagriva workarounds** in `crates/bib/src/cite.rs`: drop them once a
  release includes typst/hayagriva#520.

## Open questions

- Whether to cap OpenAlex spend per CLI command, as MCP sessions are.
- The MCP budgets' defaults ($0.25 a session, $1 a key a day).
- Whether a lookup should ever hedge (ask two providers at once).

## Conventions

- Licence MIT OR Apache-2.0; edition 2024 on current stable, no MSRV yet.
- Conventional commits; small, focused changes; clippy and rustfmt clean.
- Navigable by grep and this file (`bench/nav-dogfood.md`): `just check`
  and CI fail a source file over 1,000 lines before its tests
  (`scripts/file-sizes.py`, unless `scripts/file-sizes.allow` says why), a
  function over 200 lines without a local allow and reason
  (`clippy.toml`), and a path or DESIGN section cited but not there
  (`scripts/doc-paths.py`). Split by concern before allow-listing. A rule
  that belongs to one module goes in its `//!` doc; this file points.
- Tests: unit tests beside code, property tests for identifiers, fixture
  replay for providers, paused-time tests for retries, pacing and
  concurrency, the live benchmark for scoring; fuzz targets (`fuzz/`,
  nightly, `just fuzz <target>`) for readers of untrusted input.
