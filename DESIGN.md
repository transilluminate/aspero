# Design

Working notes for **aspero**, a cross-platform Rust library + CLI for finding,
resolving and traversing scholarly literature across multiple providers
(OpenAlex, Crossref, Semantic Scholar, doi.org, arXiv, …). Primary users are
**local agents driving the CLI**, then humans (later a TUI).

Motivation: agents asked to do literature research either recall DOIs from
memory (often wrong or invented) or try web search, which is usually blocked
for bots. Scholarly APIs are open to programmatic use; a polite, cached,
agent-ergonomic CLI over them closes that gap. The first job is therefore
**getting correct identifiers and checking cited works exist**.

Name: *aspero* (cf. asperomagnetism; Latin *asper*, rough). Free on crates.io
as of 2026-11-10; crates will be `aspero` (CLI binary) and `aspero-*`.

Status: pre-code. Decisions below are agreed unless listed under
[Open questions](#open-questions).

---

## 1. What it's for

The scope is deliberately wide in *possibility* and narrow in *build order*.
The scaffold must not rule any of these out; the POC builds almost none of them.

**Resolution & hygiene**
- **Resolve** messy input (citation string, URL, BibTeX entry, "Vaswani 2017
  attention") → canonical record(s) with confidence.
- **Lint a bibliography** (`check refs.bib`): existence, wrong/missing DOIs,
  duplicates, preprints since published, retractions (Crossref now carries the
  Retraction Watch data), missing fields. CI-friendly.
- **Verify agent output**: do these cited works exist and match the stated
  title/authors/year? Later: does the cited text support the claim (S2 snippet
  search).

**Discovery**
- Agentic search without per-API knowledge.
- **Snowballing**: forward/backward citation chasing from a seed set, deduped,
  ranked by co-citation / bibliographic coupling ("cites ≥3 of my seeds").
- **Watches**: new works citing X / matching Y since last run → digest.
- Authors & venues: who works on this, recent output.

**Access**
- Locate and fetch the legal open-access PDF (Unpaywall, arXiv, Europe PMC).

**Integration** (the lightweight alternative to a bloated reference manager)
- DOI-as-citekey: `[@doi:10.1145/…]` in Pandoc/Typst/LaTeX → `.bib` generated
  at build time.
- Format conversion & citation styles: BibTeX, CSL-JSON, RIS; CSL styles via
  `hayagriva`.
- Zotero interop (complement, don't compete), note templates (Obsidian etc.),
  graph export for a web visualiser, MCP server.

### Design consequences

0. **Interface priority: CLI first.** Local agents call it via a shell, so
   `--help` text, JSON output and exit codes *are* the agent interface. MCP
   stays possible (thin adapter over the engine) but is not planned.
1. **`resolve` is a first-class verb** alongside `lookup`: fuzzy in, ranked
   candidates with confidence out.
2. **Sets in, sets out.** Commands accept and emit collections (`.bib`, a
   session, a list, stdin) as well as single items, so they compose in pipes.
3. **Time is recorded from day one** (`fetched_at`, history), so "since last
   run" / watches are cheap later.
4. **CSL-JSON-shaped core**, so export and formatting are cheap.

---

## 2. Principles

- **Library first.** The CLI, TUI and MCP server are thin adapters over the
  same engine. Anything useful lives in a library crate.
- **Agent-ergonomic.** JSON when stdout isn't a TTY, lean default fields,
  structured errors, distinct exit codes, never silently drop a request
  parameter.
- **Polite by construction.** Rate limits, backoff and circuit breaking are
  centralised and shared across processes — not left to each call site.
- **Never lose source data.** Store raw provider responses; merged views are
  derived and recomputable.
- **Don't over-unify.** Shared core is minimal; source-specific data lives in
  per-source extensions.
- **Keep WASM possible** for `core` and provider crates (no direct `tokio`
  dependency; timers behind an abstraction; no disk I/O). Not a goal yet.

---

## 3. Architecture

```
crates/
  core/        types, Provider trait, identifier parsing/normalisation, errors,
               resilience policy types. No I/O.
  providers/
    openalex/  feature "openalex"
    crossref/  feature "crossref" (bibliographic query for resolve; retractions)
    doi/       feature "doi"      (doi.org content negotiation → CSL-JSON/BibTeX)
    s2/        feature "s2"       (after API key arrives)
    arxiv/ …   later
  store/       SQLite: raw records, identifier graph, history, rate-limit
               tickets, circuit-breaker state. Behind a `Store` trait.
  engine/      routing, caching, merging, in-flight dedup, resilience.
  cli/         clap; `tui` feature later (ratatui).
  mcp/         later (rmcp).
```

### Pluggability

- **Compile time:** each provider is a crate behind a cargo feature. No
  `.so`/`.dylib` plugins (no stable Rust ABI).
- **Runtime:** config enables/disables/orders compiled-in providers. A provider
  named in config but not compiled in → warning, not error.
- **Library users** implement `Provider` and register it with the engine.
- **Later, maybe:** out-of-process plugins speaking JSON over stdio (cf. git
  credential helpers). WASM component plugins are a possible but overkill path.

### Provider trait (sketch)

One trait with defaulted `Unsupported` methods plus declared capabilities —
friendlier to `dyn` than capability sub-traits with downcasting.

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn capabilities(&self) -> &Capabilities; // id kinds, search filters, graph, recs

    async fn lookup(&self, ids: &[WorkId], fields: &FieldSet) -> Result<Vec<SourceRecord>> { Err(Unsupported) }
    async fn search(&self, q: &SearchQuery) -> Result<SearchPage> { Err(Unsupported) }
    async fn citations(&self, id: &WorkId, page: PageReq) -> Result<GraphPage> { Err(Unsupported) }
    async fn references(&self, id: &WorkId, page: PageReq) -> Result<GraphPage> { Err(Unsupported) }
    async fn recommend(&self, pos: &[WorkId], neg: &[WorkId]) -> Result<Vec<SourceRecord>> { Err(Unsupported) }
}
```

- Providers return `SourceRecord { source, ids, core, raw, fetched_at }` and
  **never merge**; the engine does.
- Each provider owns: its HTTP mapping, its error mapping into the shared error
  enum, its default `ResiliencePolicy`, and its credentials.
- **Transport seam**: providers never own an HTTP client. They build
  `HttpRequest`s and map `HttpResponse`s; the engine injects an `HttpClient`
  (where per-request pacing, timeouts and user agent live). Responses carry
  `received_at`, so providers stamp `fetched_at` without a clock. Tests inject
  `ReplayClient` (core `testing` feature) with curl-recorded fixtures — no
  network, no async runtime, no wiremock.
- `SearchQuery` has common filters (text, year range, type, open access,
  limit) plus an `extra` map. Filters a provider can't honour are reported in
  the response's `ignored_filters` — never silently dropped.
- `SearchQuery.mode`: `Keywords` or `Citation` (a whole reference string to
  match). Citation goes only to providers declaring `citation_search`
  (Crossref's `query.bibliographic`). Hits carry the provider's relevance
  score, comparable only within one provider's results.
- Note for WASM later: futures there are `!Send`; may need
  `#[async_trait(?Send)]` under `cfg(target_arch = "wasm32")`.

### Routing

| Operation | Interchangeable? | Strategy |
|---|---|---|
| Lookup by ID | Yes | Fallback chain by priority **and current rate-limit headroom / breaker state** |
| Search | No (rankings differ) | Explicit `--source`; or `--source all` → fan out, dedup via identifier graph, merge by reciprocal rank fusion, label provenance |
| Citations / references | Partially (coverage differs) | Union with per-edge provenance |
| Recommendations | No | Capable providers only |

---

## 4. Data model

- **Core `Work`** (CSL-JSON-shaped, minimal): identifiers, title, authors,
  issued date, container/venue, abstract, type, URLs.
- **Per-source extensions** for everything else (S2 TL;DR and influential
  citations, OpenAlex concepts/topics, arXiv versions/categories, …).
- **All fields `Option`**, plus `#[serde(flatten)] extra` on wire types so new
  upstream fields don't break deserialisation.
- **Field-level provenance** on merged views, with per-field source priority
  (e.g. Crossref/doi.org for bibliographic fields; S2/OpenAlex for citation
  counts; arXiv for latest preprint version).

### Identity

- Identifier kinds: DOI, PMID, PMCID, arXiv, S2 paperId, S2 CorpusId, OpenAlex
  `W…`, MAG, DBLP, ACL, URL.
- **DOI trailing punctuation**: `.,;:` are stripped on parse (DOIs pasted
  from prose). The spec allows a DOI ending in one, so if a stripped DOI is
  not found and the input had trailing punctuation, the engine retries once
  with it kept — no config switch needed.
- **Normalisation** is core logic and heavily tested (DOI: lowercase, strip
  `https://doi.org/`, `doi:` prefixes, URL-decoding; arXiv: old/new styles,
  version suffix handling; etc.).
- **Identifier graph**: every response that reveals co-identifiers adds edges;
  a work is a connected component.
- **Version identity** (preprint ↔ published, conference ↔ journal): merge
  only on **asserted** links from a provider. Fuzzy title/author matching
  produces *suggestions* ("possibly same as …"), never auto-merges — a wrong
  merge corrupts the graph; a duplicate is merely annoying.

---

## 5. Storage

**SQLite from day one** (`rusqlite`, `bundled`), WAL mode, `busy_timeout`.

**Two stores, by disposability** (decided 2026-11-10):
- `cache.sqlite` in the platform **cache** dir (`SqliteCache`, built):
  records, "not found" answers, search pages — all refetchable. No
  migrations: a schema-version mismatch drops and rebuilds it. Batch-first
  API (many IDs per read; many answers per write transaction).
- `aspero.sqlite` in the platform **data** dir (when first needed): history
  and derived data (embeddings, extracted text) — costly to recompute, so
  forward-only numbered migrations recorded in `user_version`.
Chosen over many JSON files because of multi-process agent concurrency and the
need for transactions across cache, identifier graph, history and rate-limit
state.

Tables (indicative):
- `source_record(source, source_id, fields, raw_json, fetched_at)`
- `identifier(kind, value, work_key)` + edges with provenance
- `history(id, ts, session, command, args_json, result_ids)`
- `provider_state(provider, next_slot_at, backoff_until, breaker_state,
  consecutive_failures, opened_at)`

Cache policy:
- Entity-level: a request is a hit if requested fields ⊆ cached fields; misses
  fetch only missing IDs/fields, batched where the provider supports it.
- TTL per resource kind (indicative: metadata ~7d, citation lists ~1d,
  searches ~1d), configurable.
- **Stale-on-error**: if providers fail, serve stale data flagged
  `"stale": true` rather than erroring.
- Location: platform cache/data dirs via `directories`.
- **Negative cache**: "not found" answers are cached briefly (~1d), so
  re-verifying a bibliography with invented DOIs doesn't re-ask every time.
- **Derived data** (abstract-derived embeddings, extracted text, …) lives in
  one generic table keyed by `(work id, kind, producer, version)` with a
  media type and a blob, found via any of the work's identifiers. Nothing in
  aspero depends on a particular producer.

### Abstracts, text and embeddings (later; seams only)

aspero stays a standalone, MIT/Apache, generally useful tool. These are
optional extensions behind generic seams, never dependencies.

- **Abstracts first**: they are the cheap, high-value step for agents. The
  agent is the reasoner — given the abstract it can judge whether a cited
  work supports a claim (the step after `verify`). Sources: OpenAlex
  (inverted index; not always present), Crossref (JATS, sometimes), S2.
  Cached whenever a provider returns them, with provenance.
  *Built:* `--abstract` on lookup/verify/search/resolve, via
  `Engine::abstracts` (fills gaps from other providers, keeps provenance,
  `null` when none exists — e.g. Nature's "Deep learning" has no abstract
  at OpenAlex or Crossref). Next source for coverage: S2, and arXiv's API
  for preprints.
- **Re-normalising cached records**: the cache stores mapped records *and*
  raw bodies. When a provider's mapping changes (e.g. stripping an
  "Abstract" heading), cached records stay old until their TTL. Fix when
  it matters: providers declare a mapping version; the engine re-maps
  cached raw bodies whose version is older.
- **Full text** (Phase 4): fetching legal OA PDFs is easy (Unpaywall, arXiv,
  Europe PMC); extraction is the hard part. A text-extractor seam lets any
  external library or tool fill `derived` with text — e.g. a layout-aware
  ML pipeline such as rs-cogniste, used as an optional library, not a
  dependency.
- **Embeddings** (Phase 3/4): **S2-first**. The S2 Graph API offers
  precomputed SPECTER2 vectors as a paper field (to confirm when the S2
  provider is built). Local inference (candle/ONNX, optional crate behind a
  feature) only for works S2 lacks, and only with the **same base model and
  adapter** S2 uses, so vectors share one space and compare directly.
  SPECTER2 has task adapters (proximity, ad-hoc query, …): confirm which S2
  serves before mixing. Uses: ranking snowballed sets against seeds without
  LLM cost, `related`, watches, and preprint↔published *suggestions* (§4).
  Vectors are BLOBs in `derived`; brute-force cosine is fine at personal-
  library scale, `sqlite-vec` only if ever needed.

---

## 6. Rate limiting & resilience

Centralised `ResiliencePolicy` per provider, defaults matching each provider's
terms (S2's key terms require exponential backoff), overridable in config.

**Rate limiting**
- In-process: single dispatcher with a priority queue (interactive > background
  prefetch) and **in-flight dedup** (identical concurrent requests share one
  call).
- Cross-process: ticket scheme in `provider_state` — `BEGIN IMMEDIATE`, claim
  `next_slot_at`, advance by interval, commit, sleep until slot.
- On 429: write `backoff_until` so **all processes** back off together.
- Later: a daemon / the MCP server can own the queue behind the same trait.

**Backoff**
- Exponential, full jitter, **per-sleep cap**, **max attempts**, and an
  **overall per-request deadline**.
- Don't rely on `Retry-After` (S2 doesn't send it on 429s — see §9); honour it
  when present.

**Circuit breaker** (per provider, state shared via SQLite)
- Closed → Open after N consecutive countable failures → Half-open after
  cooldown (single probe) → Closed on success.
- Countable: timeouts, connection errors, 5xx. **Not** countable: 429 (backoff
  instead), other 4xx.
- Auth rejected (401/403): distinct `AuthRejected` state — disabled until
  credentials change.
- Open breaker ⇒ router falls through to the next provider. All open ⇒
  cache-only mode with stale flags.

**Errors** (`thiserror` in libraries; categorised by what the caller does):
`RateLimited { retry_after }`, `QuotaExhausted { resets_at }`, `Network`, `Server(status)`, `Timeout`,
`CircuitOpen`, `AuthRejected`, `NotFound`, `BadRequest(msg)`, `Unsupported`,
`Decode`, with `is_retryable()`. Error parsing must handle non-provider bodies
(e.g. AWS API Gateway JSON in front of S2).

---

## 7. Configuration & credentials

Config file: **TOML** (not YAML — `serde_yaml` is archived), platform config
dir, every field with a serde default.

```toml
[providers.openalex]
enabled = true
priority = 20            # key via OPENALEX_API_KEY / keyring

[providers.crossref]
enabled = true
priority = 30
mailto = "me@example.com"

[providers.s2]
enabled = true
priority = 10

[providers.s2.resilience]
max_retries = 5
backoff_cap = "60s"
deadline = "120s"
breaker_threshold = 5
breaker_cooldown = "5m"
```

Credentials needed now: **OpenAlex API key** (`OPENALEX_API_KEY`; free tier
is a daily credit allowance, so cost per call matters — see §9). Crossref and
doi.org need none, only a `mailto` for the polite pool. S2 key pending.

Credential resolution order (shared by CLI, TUI, MCP), per provider:
1. `--api-key` flag (supported, discouraged — visible in `ps`/history)
2. Env var (`S2_API_KEY`, …) — primary path for agents
3. OS keyring (`keyring` crate)
4. Config file, written `0600` — fallback for headless systems

- `auth login [--provider X]`: hidden prompt → keyring (fallback file). The
  TUI entry box calls the same function.
- `auth status`: shows key source per provider and validates with one cheap
  request, distinguishing *rejected* from *rate-limited*.
- `.env.local` not a primary mechanism (CWD-dependent); optional `--env-file`.
- Secrets held as `secrecy::SecretString`; never logged or `Debug`-printed.

---

## 8. CLI surface (indicative)

```
aspero resolve <text|id|->           # fuzzy → ranked candidates
aspero verify <citation|->           # does it exist and match? (basic in Phase 0)
aspero lookup <id>...                # DOI:…, arXiv:…, PMID:…, W…, …
aspero search <query> [--source X|all] [filters]
aspero citations|references <id>
aspero related <id>                  # recs + top citations + key references
aspero check <refs.bib>              # later; `verify` over a whole bibliography
aspero history / session …
aspero auth login|status
aspero cache stats|clear
```

- Output: `--format json|jsonl|table|md|bibtex|csl`; JSON default when not a
  TTY. Lean default field set; abstracts opt-in.
- Stdin accepted wherever a list is.
- `S2_SESSION`-style env var / `--session` groups an agent's research thread.
- Errors: structured JSON on stderr in JSON mode; distinct exit codes per
  error category.
- Help text is a product surface for agents: every command has examples.

---

## 9. Observed API behaviour

Recorded 2026-11-10 from a keyless probe of S2 Graph API
(`/graph/v1/paper/search`):

| Request | Result |
|---|---|
| No key, single | `429` (shared pool saturated) |
| No key, burst of 10 | `200` then nine `429` |
| No key, 7 tries @3s | all `429` |
| Fake `x-api-key` | `403 {"message":"Forbidden"}` |

- 429 comes from AWS API Gateway (`x-amzn-errortype: TooManyRequestsException`),
  not S2 application JSON. **No `Retry-After` header.**
- No `Access-Control-*` headers on 429s; CORS on 200s unverified (affects the
  in-browser WASM idea).
- Keyless S2 is best-effort only. These responses become replay fixtures.

### OpenAlex (help.openalex.org, pages dated 2026-10-23 – 2026-11-03)

- **Key**: `?api_key=…` or `Authorization: Bearer …` (prefer the header — keeps
  it out of logged URLs). Unactivated key → `403 key_not_activated`. No
  `mailto` / polite pool mentioned any more.
- **Budget**: free key $1/day (resets 00:00 UTC); no key $0.10/day. New
  accounts are throttled to ≤$0.10 per 5 min for their first $1. Errors are
  free.
- **Cost per call**: get-by-ID/DOI **free**; list+filter $0.0001; keyword
  search $0.001; semantic search $0.001 (1 req/s); `group_by` like its base;
  rerank +$0.001; cached PDF download $0.01. So $1/day ≈ 10k filter calls or
  1k searches — lookups cost nothing.
- **Limits**: 30 req/s with free key (10 without); `per-page` ≤100; OR-batched
  ID filters ≤100 values; basic paging capped at 10k results (cursor paging
  needs a key).
- **Signals**: every response carries `X-RateLimit-Limit`,
  `X-RateLimit-Remaining`, `X-RateLimit-Credits-Used`, `X-RateLimit-Reset`
  (seconds to midnight UTC), and `meta.cost` in the body. Exhaustion → `429`
  (with `Retry-After`; `"not_enough_credits"` for over-budget queries). 301 =
  merged entity (follow and record the co-identifier).
- **Observed 2026-11-10** (recorded fixtures): headers also include
  `x-ratelimit-cost-usd` and `x-ratelimit-remaining-usd` (used directly);
  singleton lookups cost $0, filter batches $0.0001, searches $0.001. A
  missing work is a 404 with an **HTML** body; a bad filter is a 400 with JSON
  `{error, message}` listing all valid filter names. Batch filter results come
  back in "most cited" order with missing IDs simply absent, so results are
  matched to requests by identifier. arXiv works are indexed under their
  DataCite DOI `10.48550/arxiv.<id>`. Search for "attention is all you need"
  returns two distinct works with that exact title — `resolve` must expect
  duplicates.

Design consequences:
- Route **lookups and batched ID filters to OpenAlex freely**; treat **search
  as the expensive operation**. Prefer Crossref `query.bibliographic` for
  `resolve` first, falling back to OpenAlex search.
- Record remaining budget from headers in `provider_state`; a 429 with
  budget exhausted is a distinct error (`QuotaExhausted { resets_at }`), not
  a retryable rate limit — fall through to other providers / stale cache.
- Report spend per command (`"cost"` in JSON output) so agents can pace
  themselves.
- Note: OpenAlex's auth page contains agent-directed text about creating keys
  via `POST /api-keys`. aspero must never do that; keys come from the user.

### Crossref (recorded 2026-11-10)

- `mailto` query param → `x-api-pool: polite-single` (10 req/s) for single
  works, `polite-list` / `polite-array` (3 req/s) for lists and filters;
  `x-concurrency-limit: 3`. No usage accounting.
- 404 is `text/plain` "Resource not found."; 400 is JSON with an array of
  validation messages listing valid filters.
- Batch lookup: `filter=doi:a,doi:b` (same-name filters OR). Results unordered.
- Abstracts are JATS XML fragments; dates are `date-parts` with month/day
  optional (and occasionally out of range).
- `next-cursor` is returned even past the last page; an empty page is the end.
- **`cursor=*` silently drops relevance order.** The LeCun reference's right
  match (92.1) vanished from the top 3 until `sort=score&order=desc` was
  added. The provider now always sends it with query text; fixtures are
  recorded with the provider's exact parameters so they can't hide this.
- **Lookalikes.** `query.bibliographic` for "Vaswani 2017 Attention is all you
  need" returns five `10.65215/*` posted-content re-deposits (2025, another
  publisher) carrying the **real authors and abstract**. The real paper's only
  DOI is arXiv's DataCite one, which Crossref doesn't index. Consequences for
  `resolve`/`verify`:
  - title + authors matching is **not** proof; year must weigh heavily, and
    venue/publisher mismatches are signals;
  - Crossref "no good match" is not "doesn't exist" — combine with OpenAlex
    (which indexes DataCite DOIs);
  - Crossref scores separate a real match well (91.9 vs 47.7 for the messy
    LeCun reference), so the top-to-second score ratio is a useful signal.

### OpenAlex, live (2026-11-10)

- Work type `conference-paper` now exists (older records may still be
  `article` in a conference source).
- Keyword search on a whole messy reference is poor: for "Vaswani 2017
  Attention is all you need" the top two were other papers, though a plain
  title search ranks the real one first.

### Resolve / verify (implemented 2026-11-10)

Pipeline (engine `resolve.rs`), cheapest first: lookups of claimed IDs
(including DOIs/arXiv IDs found in the text) → Crossref citation matcher on
the whole reference → OpenAlex **title** searches (the claim's title, or the
matcher's top distinct titles — a lookalike still carries the right title).
Candidates sharing an ID merge. `verify` stops once a free source verifies.

Scoring (core `matching.rs`) looks for the record's fields *in* the text
rather than parsing citation styles: title as a phrase, authors' family
names (first author alone is a match — "et al." styles), years. Year score
halves per year off; ≥2 off is a mismatch with a 0.5 penalty. One comparable
field caps confidence below "verified".

Verdicts: verified / plausible / mismatch (claimed ID missing or another
work's; or title+authors match but the year differs) / not_found /
unverifiable. Live batch (7 claims, $0.006): both Vaswani forms → real arXiv
paper; LeCun Vancouver → Nature 2015 via Crossref alone; nature14540 →
mismatch naming nature14539; invented paper → not_found; NeurIPS 2019 →
mismatch "year (2017) differs".

**Benchmark** (`bench/citations.jsonl`, `cargo run -p aspero-engine
--example verify_bench`): 20 citations with known verdicts, run live
through `target/bench-cache` (first run ≈ $0.015, re-runs free). It caught
two bugs on day one:
- Crossref's top matches for an AlphaFold reference were *Faculty Opinions*
  commentaries ("… recommendation of <title>"); their titles steered the
  OpenAlex search away from the real paper. Fix: query the part of a hit's
  title the reference shares (`shared_title`).
- An unmentioned sole author scored as a partial author match, making a
  1992 "Protein Folding" a near miss for an invented paper.

Add a case for every new failure mode found in the wild.

Still to do: venue as a signal; merging records that share no ID
(preprint ↔ published, as *suggestions* per §4); down-weighting
derivative records (commentaries, peer reviews) by type.

To verify at implementation time: doi.org content-negotiation limits,
arXiv (~1 req / 3 s).

---

## 10. Roadmap

**Phase 0 — POC (now)** — deliberately thin; prove usefulness to agents.
- Cargo workspace with the Phase 0 crates (later crates added when needed).
- `core`: types, identifier parsing/normalisation, `Provider` trait, error
  enum, `ResiliencePolicy`.
- Providers: **OpenAlex** (lookup, search, citations/references) and
  **Crossref** (lookup, bibliographic query for `resolve`). Two providers from
  the start keep the trait honest. **doi.org** as a small fallback / BibTeX
  source.
- `store`: SQLite cache + identifier graph + history. `provider_state` table
  created now (schema stable) but only used in-process.
- `engine`: routing with fallback, cache, per-process rate limit and backoff
  (honouring the §6 caps and deadlines).
- `cli`: `lookup`, `search`, `resolve`, `verify` (basic: exists + title/
  author/year match), `history`, `auth status`.
- **Tests on core logic**: identifier normalisation (property tests), cache
  field-subset logic, identifier-graph merging, backoff schedule & ceilings,
  routing fallback, error mapping from recorded fixtures (replayed through the
  transport seam; `scripts/record-*.sh` re-records them).
- Build a small **fixture set of bad citations** (invented DOIs, wrong years,
  title drift, preprint vs published) — no existing test data; this doubles as
  the `resolve`/`verify` benchmark.

**Deferred until real use shows the need:** cross-process ticket limiter,
shared circuit breaker (state machine + multi-process tests).

**Phase 1** — S2 provider (once key arrives): search, batch, recommendations,
snippets; `related`. Breaker + cross-process limiter if needed.
**Phase 2** — `check` (bib lint over `verify`), retractions, format export
(`hayagriva`).
**Phase 3** — snowballing; watches; MCP adapter if ever wanted.
**Phase 4** — TUI; graph export + local web visualiser; OA PDF fetch.

---

## Project conventions

- Licence: MIT OR Apache-2.0.
- Rust edition 2024, tracking current stable; no MSRV promise yet
  (`rust-version` set only once there are users who need one).
- Async: engine/CLI on `tokio`; `core` and providers runtime-agnostic.

## Open questions

- Whether to cap OpenAlex spend per command / per session (e.g. refuse
  `--source all` fan-out searches once remaining budget < threshold).
- Exact default TTLs and resilience numbers per provider.
- Whether the cross-process limiter needs a daemon earlier than Phase 3.
- Resolve confidence scoring: which signals (title similarity, author overlap,
  year delta, venue, provider score margin) and how weighted. See the Crossref
  lookalike notes in §9: year must weigh heavily.
