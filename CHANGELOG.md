# Changelog

## Unreleased

Added:

- Pages that aren't scholarly works (#119): `claimFromPage` tells a
  scholarly work's page (an identifier, citation tags, scholarly JSON-LD,
  a publisher's site, or no markup to tell) from a web page (a Wikipedia
  article, a news story, guidance), and a web page has no claim to
  verify. `PageClaim.kind` (`PageKind`: `work`, `webPage`) and
  `PageClaim.webPage` (`WebPage`: title, site, canonical address
  normalised, people as authors, published, updated, language, and its
  key, `url:` and the address), read for every page with a title.
  `webPageWork(page, accessed)` makes it a `Work` of type `webpage` that
  `export` writes as BibTeX `@misc` (`howpublished`, `note` with the
  site and access date), RIS `ELEC` (`Y2`, `LA`) and CSL-JSON `webpage`
  (`accessed`, `language`: a Zotero Web Page), and `cite` formats in any
  style. `Work.accessed` and `Work.language`; core `WorkType::WebPage`,
  `Work::accessed`, `Work::language`, `webpage::{PageKind, WebPage,
  normalise_url}`. `aspero verify --html FILE --url URL` shows a web
  page's record (exit 0) rather than verifying it, and writes it out
  with `--format bibtex|ris|csl-json|cite`, read on `--accessed DATE`
  (today by default).

Fixed:

- `claimFromPage` titled a page by its JSON-LD `headline` before its
  `name` (#118): Wikipedia's `headline` is the article's short
  description ("American actor and novelist (1930–2025)"). `name` comes
  first now, unless it isn't a title. A page title's suffix also goes
  when it is the name the page gives its own site (`og:site_name`,
  JSON-LD's website or publisher), even one holding a separator.
- A page's values lost anything that looked like a tag once decoded, so
  a title naming one ("&lt;meta&gt;: The metadata element") lost it. Only
  formatting markup (`<i>`, `<sub>`, `<span>`, JATS's, namespaced tags)
  is taken out now, escaped or not.

## 0.28.2

Added:

- `Budget.spendableTodayUsd`: what this engine's paid calls may still
  spend today under both caps, as `aspero budget` counts it
  (`Engine::spendable_today`), and `Budget.creditCapUsd`, the credit cap
  in force ($0.25 when unset), so callers needn't repeat the policy.

## 0.28.1

Added:

- Each MCP key's spend today (#113): `HttpServer::status` gains
  `KeyStatus.spent_today_usd`, `Status.spend_day` and `spend_resets_at`
  (FFI `McpKeyStatus.spentTodayUsd`, `McpStatus.spendDay`,
  `spendResetsAt`): the ledger's figure for the UTC day plus spend not yet
  recorded, the same the key's daily budget is checked against.
- What OpenAlex charges (#113), from `/rate-limit`'s `endpoint_costs_usd`
  (so with a key): `Usage.prices` and `Budget::prices` (a `Prices` of
  `singleton`, `list`, `search`, …), `Budget::search_price`; FFI
  `Budget.searchPriceUsd` and `Budget.prices` (`[BudgetPrice { endpoint,
  usd }]`). `Engine::spendable_today`: what paid calls may still spend
  today under the caps (the allowance left to its cap, then credit to the
  credit cap). `aspero budget` adds "searches $0.001 each, about 850 left
  today" (spendable over the search price, rounded down) and JSON
  `spendable_today_usd`, `search_price_usd`, `searches_left_today`,
  `prices_usd`.

## 0.28.0

Added:

- Developer: an HTTP record/replay tape (`ASPERO_HTTP_TAPE`,
  `ASPERO_HTTP_TAPE_MODE=replay-only`), on by default for `verify_bench`
  and `scripts/redteam.py`: each distinct request is paid for once and
  replayed free after, offline if asked (DESIGN "HTTP tape").
- A daily cap on OpenAlex prepaid credit (#110), beside the allowance's:
  `openalex_daily_credit_cap_usd` (engine `StandardConfig`,
  `EngineBuilder::daily_credit_cap`; FFI `Config.openalexDailyCreditCapUsd`,
  nil for the default; CLI `--openalex-credit-cap-usd` /
  `ASPERO_OPENALEX_CREDIT_CAP_USD`). Once the free allowance is all used
  (by anything using the key), paid calls go on, spending credit, while
  the credit spent today is under it; until then the allowance cap holds
  the rest of the allowance back. 0 never spends credit (DESIGN
  "Resilience").
- Which cap stopped a call: `Error::BudgetCapped` gains `which`
  (`CapKind::Allowance` or `Credit`), FFI `BudgetCap.capKind`
  (`BudgetCapKind.allowance` / `.credit`), with `capUsd`/`spentUsd` the
  cap reached and what's spent against it; a credit stop reads "stopped at
  today's credit cap of $1.00 ($1.02 of credit spent today)".
- Credit spent today and the credit's expiry on `Budget`:
  `prepaid_spent_today` / `prepaid_expires_at` (FFI
  `prepaidSpentTodayUsd`, `prepaidExpiresAt`; `aspero budget` JSON
  `prepaid_spent_today_usd`, `prepaid_expires_at`, and `allowance_cap_usd`,
  `credit_cap_usd`). OpenAlex doesn't report the spend: it's the day's
  first balance seen, kept in the cache file so every process and restart
  agrees, less the balance now, a top-up raising the start; a day first
  seen part-way through undercounts. `Usage.prepaid_expires_at` from
  `/rate-limit`, read as JavaScript prints a date (or ISO).

Changed:

- OpenAlex prepaid credit is now capped at $0.25 a day by default
  (before, it was spent freely once the allowance was gone, unless an
  allowance cap at or under the allowance stopped every paid call). Set
  `--openalex-credit-cap-usd` (or the config field) to allow more, or 0 to
  spend none. With a key, the first paid call of a process now asks
  `/rate-limit` (free) first, as an allowance cap already did. The
  benchmark and `scripts/redteam.py` set their own credit cap, $5.
- `aspero budget` says both caps in their terms: "free allowance $0.0498
  of $1.00 used today (cap $0.50), resets 00:00 UTC; credit $0.0300 spent
  today (cap $0.25), $4.97 left, expires 2027-02-14".

Fixed:

- Credit spent today no longer runs far past the truth ("$5.33 of credit
  spent today" for $0.34 spent): OpenAlex's servers answer with balances
  up to $0.30 apart, the stale ones higher, and each rise was taken for a
  top-up. A response's balance now only ever lowers today's figure; only
  `/rate-limit` (which says the credit bought) sets it either way or tells
  a top-up; and a stop at the credit cap is checked with a free
  `/rate-limit` first (at most once a minute). `Budget.prepaid_remaining`
  is that balance, not the last response's. The first `prepaid_day`
  table is dropped. `Usage.prepaid_bought` (OpenAlex
  `prepaid_balance_usd`).
- OpenAlex is no longer stopped until the reset ("usage quota
  exhausted") by a failing call (a per-second 429, a server fault) made
  once the day's allowance is used up while prepaid credit is left:
  OpenAlex spends the credit then, so only no credit left either (or its
  `not_enough_credits`) is a spent budget.

## 0.27.0

Added:

- `docs/accuracy.md`: what `verify` promises (a fabricated or miscited
  reference shouldn't come back verified; plausible is "check it") and
  what it can't check, the verdict policy, the red-team method (held-out
  and adversarial rounds), acceptance criteria and the release gate, the
  0.26.0 baseline, and how to reproduce it. Linked from the README and
  DESIGN.
- Issue forms: "Wrong verdict" (the citation, the verdict, the JSON
  output, the right work, the version and where it ran), "Bug" and
  "Feature"; `scripts/issue-to-case.py <n>` turns a wrong-verdict issue
  into a red-team case in `bench/redteam/reports.jsonl`.
- `verify --report`: after each result, a link to a new wrong-verdict
  issue with the form filled in (JSON: `report_url`), the output
  shortened to keep the link under 7,000 characters. Nothing is sent: the
  link is only printed.

Fixed (from the red team of #102: wrong citations verified fell from 31
of 152 to none, with real ones accepted 152 of 155, from 147):

- A citation naming co-authors a work doesn't have is no longer
  verified on its first author: it's plausible, its authors to check.
  Not for consortia, bylines cut short, or books.
- A title with words added ("Deep learning for medical image analysis"
  for "Deep learning"), a subtitle the work lacks or another one, or a
  word replaced ("Hydroxychloroquine in …" for "Dexamethasone in …") or
  words inserted ("… Septic Shock in Adults and Children (Sepsis-3)") is
  plausible at most. A dropped subtitle still matches, as does a subtitle
  added to a book's title (book records often leave it out).
- A year inside a title ("Cancer statistics, 2020") no longer stands in
  for the year cited, and is part of the title: the 2020 report cited as
  of 2018 with its volume and pages is plausible, the year named, not the
  2018 report nor "Colorectal cancer statistics, 2020". "CA Cancer J
  Clin" now matches "CA: A Cancer Journal for Clinicians".
- The cited journal is read in more styles: before the year ending a
  Vancouver reference ("… JAMA. 2020."), before its DOI link, IEEE's
  "vol." and "no.", Harvard's "'…', Chest." and "pp.", BibTeX's
  fields, prose ("… reported in Gastroenterology (1984)"), after a title
  ending in a capitalised word ("… Wuhan, China. Lancet."). A wrong journal there, alone or with the year one off, is
  plausible, not verified.
- Nature's single page ("Nature 530, 484 (2016)"), Harvard's pages and
  BibTeX's volume and pages are read, so wrong ones are caught.
- Another work's title at a work's journal, volume and page is a
  different title, not a missing one: plausible at most, never verified.
- A copy without journal, volume or page (a Zenodo deposit, a report)
  no longer verifies a citation of the journal article, nor outranks it.
- A preprint cited in a journal by name, and a journal article cited as
  a preprint, are other versions: plausible.
- Real works missed before are found or accepted: conference papers
  without DOIs (the title a reference suggests is searched when the
  citation matcher has none), a book found only reprinted in a reader,
  and a dataset whose DOI's record carries a later version's year.

## 0.26.1

Fixed (found by fuzzing, `fuzz/`):

- An `author_id` like an ORCID with extra hyphens (`0000-0002-18-5-0097`)
  panicked instead of being refused, in a query tree from the CLI, the
  MCP tools or the FFI.
- A .docx file inflating to gigabytes was read whole: parts are read up
  to 64 MB, a larger body unreadable.
- Each identifier kind's own parser (`WorkId::parse_as`, as a page's
  `citation_doi` is read) now cleans look-alikes (Unicode dashes,
  zero-width spaces) as `WorkId` parsing and `scan` do.
- A soft hyphen ending a DOI's line, the next not continuing it, is no
  longer read as a `-` ending the DOI.

## 0.26.0

Added:

- MCP activity as data beside its summary (#100): `http::Activity` and
  Swift's `McpActivity` have `error` (the kind, when it failed: Swift's
  new `McpErrorKind`, from `invalidArguments`, `notFound`, `forbidden`
  and `unsupported`, the agent's own doing, to `unavailable`,
  `throttled`, `auth` and the app's `timeout`, with `other` last for
  kinds added later), `errorMessage` (the whole message, without
  "error: "), `query` (what an engine tool was asked, as given: the
  keywords or the query tree in a line, `title:sepsis AND years:2018–`,
  identifiers or citations joined with "; ", at most 500 characters) and
  `count` (how many came back: works found, looked up, verified,
  references listed). An app tool's `query` and `count` are empty. The
  summary is as it was; the new fields default to `nil`, so Swift code
  building an `McpActivity` still compiles.

## 0.25.0

Added:

- A claim's site, the page it was read from (#98): `claimFromPage` and
  `verify --url` fill it from the address's host (`jamanetwork.com`),
  `site` in a JSON claim and `--site HOST` give it otherwise, and Swift's
  `Claim` has `site`. It's a hint of the publisher, never the journal nor
  a difference reported. For a claim without identifiers from a known
  publisher's site (JAMA Network, Oxford Academic, Nature, Elsevier's
  sites, Wiley, Springer and BMC, ACS and some 30 more, by registrable
  domain), Crossref is also asked for the title among the publisher's
  works (its DOI prefixes, free), and between candidates that match about
  as well the publisher's is chosen: a title shared from JAMA's page
  ("Treatment of Acute Ischemic Stroke: A Review") is now JAMA's review,
  not an older article of that title elsewhere. It never makes a match
  verified, never outranks a clearly better one, and with no candidate
  from the publisher changes nothing. When it decided, JSON says
  `chosen_by_site: true` and Swift's `Verification.chosenBySite`.

## 0.24.0

Added:

- A claim from a publisher's article page, for addresses that don't say
  which work they show (JAMA Network, Oxford Academic): `claimFromPage`
  (Swift) and `aspero verify --html FILE --url URL [--title TEXT]` read
  the page's markup, as a browser rendered it, offline: Highwire
  `citation_*` tags, PRISM, Dublin Core and schema.org JSON-LD, each field
  from the first that gives it, then `og:title` or `<title>` without the
  site's name (" | JAMA | JAMA Network"), then the title a browser gave it
  unless that's link text ("Full Text", "PDF"), and the address's
  identifiers. Every identifier found is claimed, so `verify` checks it,
  or `lookup` takes it. Without the markup, the address and the title
  alone. Swift's `PageClaim` holds the identifiers (`Mention`s), the
  `Claim` to verify and the `PageSource` that answered; JSON echoes
  `page_source`.

## 0.23.0

Added:

- Publishers' article addresses, pasted or dragged from a browser, are
  read as the work they show, by `scan` and so by `lookup`, `verify`,
  `identify`, MCP and the app: Nature's `nature.com/articles/…` pages as
  their DOI (`nature14539` is `10.1038/nature14539`), and ScienceDirect,
  Cell and The Lancet pages by Elsevier's PII, a new kind of identifier
  (`pii:S2772940026000032`). Crossref finds the DOI a PII belongs to (one
  free call), and the work is then looked up as that DOI, so results are
  the same as for the DOI. A PII Crossref doesn't know is not found.
- Each mention `scan` finds says whether it was read from a web address
  (`from_url` in JSON, only when it was; `fromUrl` in Swift), so the app
  can say a work was found from its address.

## 0.22.1

Changed (the CLI's tables and text; JSON and the other formats are as
they were):

- Tables fit the terminal: identifiers are never broken, titles get the
  width left, low-value columns drop or move under the title when narrow,
  and rows of several lines are ruled apart. Search tables no longer show
  the provider's raw score.
- Text wraps to the terminal with hanging indents (unwrapped when piped).
  Numbers have thousands separators, work types read in words ("journal
  article"), and `analyse` numbers its works from 1 and leads each line
  with the work.
- `verify` text gives the verdict, why (unless it simply matches) and each
  field's check as ✓/~/✗, without repeating the work; its table shows the
  work matched.
- `verify` and `resolve` with no citation read one per line from stdin
  when piped, as `lookup` does; on a terminal they say how to give one.
- Batch progress overwrites itself on stderr, only on a terminal. Colour
  is kept to verdicts and secondary lines; `NO_COLOR` turns it off.
- `lookup` says why an input wasn't looked up below the table.

## 0.22.0

Breaking: names now agree between Swift and JSON, and money says its
unit. Only names change; values and shapes stay as they were.

- Usage's cost: `cost` → `costUsd` (Swift), `cost` → `cost_usd` (JSON).
- Usage's remaining budget: `remaining` → `remainingUsd`, `remaining` →
  `remaining_usd`.
- Usage's daily allowance: `limit` → `limitUsd`, `limit` → `limit_usd`.
- Usage's prepaid balance: `prepaidRemaining` → `prepaidRemainingUsd`,
  `prepaid_remaining` → `prepaid_remaining_usd`.
- `Budget`'s fields in Swift: `remaining` → `remainingUsd`, `limit` →
  `limitUsd`, `prepaidRemaining` → `prepaidRemainingUsd`.
- `aspero budget`'s JSON: `limit` → `limit_usd`, `used` → `spent_usd`,
  `remaining` → `remaining_usd`, `prepaid_remaining` →
  `prepaid_remaining_usd`.
- A reached daily cap's figures: `BudgetCap.cap` → `capUsd`,
  `BudgetCap.used` → `spentUsd` (Swift only: JSON gives the cap as an
  error message).
- An analysis' estimate: `Estimate.usd` → `costUsd` (Swift), `usd` →
  `cost_usd` (JSON; Rust: `Estimate::cost_usd`).
- The OpenAlex daily cap: `Config.openalexDailyCap` →
  `openalexDailyCapUsd` (Swift), `StandardConfig::openalex_daily_cap` →
  `openalex_daily_cap_usd` (Rust), `--openalex-cap` → `--openalex-cap-usd`
  (the old flag still works) and `ASPERO_OPENALEX_CAP` →
  `ASPERO_OPENALEX_CAP_USD` (the old variable is still read when the new
  one isn't set).
- A search's cursor for the next page: `SearchResults.next` →
  `nextCursor` (JSON already `next_cursor`).
- How a result was served: `SearchResults.freshness` and
  `Lookup.freshness` → `served` (JSON already `served`; the enum is still
  `Freshness`).
- A lookup attempt's failure: `Attempt.message` → `error` (JSON already
  `error`).
- A work's deduplicating identifier: `Work.canonical` → `canonicalId`,
  `canonical` → `canonical_id`.
- A DOI found only with its trailing punctuation kept:
  `Lookup.withPunctuation` → `foundWithTrailingPunctuation`; JSON's
  `"note"` → `"found_with_trailing_punctuation": true` (Rust:
  `Looked::found_with_trailing_punctuation`).
- The work a verdict is about: `Verification.best` → `bestMatch`, `match`
  → `best_match` (a `check` entry's `match` too; Rust:
  `Verification::best_match`).
- Providers that failed: `Verification.failures` → `errors` (JSON already
  `errors`).
- The caller's key, echoed: `Verification.key` → `claimKey`, `key` →
  `claim_key` (claims are still written with `key`).
- Why, as data: `Verification.why` → `reason`, `why` → `reason` (Rust:
  `Verification::reason`).
- Why, in an English sentence for logs and agents: `Verification.reason`
  → `reasonText`, `reason` → `reason_text` (Rust:
  `Verification::reason_text`).
- `Analysis.notYet`, always empty, is removed from Swift.

## 0.21.0

Breaking:

- An analysis's `incomplete` lists every stop it met, not only the
  first: a JSON array, always present (`[]` when complete), at the top
  level and in each section; `[Incomplete]` in Swift. A sample of a
  much-cited seed (`capped`) is no longer hidden behind a budget stop or
  a failure.
- Removed from the library: the unused `Capabilities` flags `citations`,
  `references` and `recommend` and their `Provider` methods,
  `ArxivId::is_old_style`, `ReplayClient::then_fail` and
  `Analysis::not_yet` (the app's `notYet` stays, always empty).
  `Engine::identify` returns an `IdentifyReport` with its usage.

Added:

- The app's search results say which filters were ignored
  (`ignoredFilters`); usage gives a source's daily budget and prepaid
  balance; cache stats count cached citation links; `analyse` takes how
  many related works to list (`relatedLimit`, 20); a stop says which
  related-works stage stopped (`stage`) and which source failed.

Fixed:

- A search naming no source asks Europe PMC before DataCite, in the order
  the search sources are listed.
- Search refuses what it couldn't search faithfully, rather than
  widening or narrowing it: work types that would be filter syntax
  (`book,is_oa:true`), terms with no letters or digits, a journal beside
  other terms on Crossref (which only ranks by it), and leaving out on
  OpenAlex a journal naming more sources than a filter takes. `<`, `>`
  and `=` are dropped from searched words.
- Europe PMC and DataCite bracket a query tree beside an expert query,
  and escape backslashes in DOIs. A none of a none means an any. Sorting
  newest first no longer changes what a tree can search.
- MCP budgets hold: a failed `identify` charges what it spent; calls at
  once pass a session's or a key's budget by one call at most, and a key
  runs at most four paid calls at once; spend the ledger couldn't save is
  kept and saved later.
- The MCP server: stopping refuses new requests at once, and a stop cut
  short finishes when called again; a session opening as its key is
  revoked is closed; a key may hold 16 sessions (then HTTP 429); activity
  the app is far behind on is dropped, not queued without limit; an app
  tool that times out is reported as of unknown outcome, to check before
  retrying.
- Related works read at most 75 pages of citing works however little
  each seed is cited, and the estimate agrees; works of unknown citation
  count are read, not taken as uncited; pages keep to the budget by what
  OpenAlex charges.
- A seed is `not_found` only once that's settled: a failure or the budget
  stopping a second ask says so. A daily cap stops an analysis as
  `budget`, not `provider_failed` (the CLI no longer exits
  "unavailable"). Preprints and published versions linked only through a
  third record count as one work. A stale cached record's references
  aren't cached as fresh. A lookup of 101 works counts one paid request.
- Odd one out says `no_reference_data`, not `no_links`, where a seed of
  unknown references may cite the work.
- A failed lookup behind `references` or a search by citations names its
  source and any daily cap reached; a spent quota with no stated reset
  says when it is tried again (`until`).
- `--style Foo.CSL` is read as a file. Counts given the app saturate
  rather than wrap.

## 0.20.1

Fixed:

- Standard journal abbreviations that leave out or join words ("Proc Biol
  Sci", "Philos Trans R Soc Lond B Biol Sci", "Acta Crystallogr D Biol
  Crystallogr", "Eur J Cardiothorac Surg") are no longer taken for another
  journal, so a correct citation using one verifies. A subject word left
  out still means another journal ("Am J Med" isn't the American Journal
  of Emergency Medicine).
- An author's initials or a word of the title no longer count as the
  journal ("SR" for Scientific Reports, "mice" for Medicine), nor outweigh
  the journal a reference names.
- Structured claims compare the journal as free-text references do, so a
  `container` the rules can't recognise no longer lowers a correct
  citation; only one that's clearly another journal is a `mismatch`.
- An OpenAlex work whose main copy is in a repository (PubMed Central,
  arXiv) gives the journal it appeared in, not the repository; an
  `article` in a book series is a chapter.
- A merged work's cited-by count no longer depends on the order sources
  answer in.
- Another edition of a cited book stays `plausible` when the claim gives
  the publisher as its container; a chapter cited from a book no longer
  makes another edition of the book plausible.
- More organisations ("Public Health England", "World Bank", "Cancer
  Research UK", "Sepsis Task Force") are no longer split into given and
  family names in exports.

## 0.20.0

Fixed:

- A citation naming the wrong journal no longer verifies (#80). The
  journal a reference cites is read where it can be told (the words
  before the volume and page, Vancouver or APA), and a journal article
  cited in a journal that's clearly another, or a preprint cited by
  journal, volume and page, is `plausible` at most, with
  `checks.container` a `mismatch` and a `journal` difference whose `cited`
  is the journal as cited. Abbreviations, "The", spelling variants and
  names inside names ("Proc Natl Acad Sci U S A") aren't another journal;
  books, proceedings and references without a clear journal are left
  alone.
- `read` carries the journal read from free text (`container`).
- APA references without an issue ("Journal Name, 146, 106274.") give
  their volume and page.
- One work shows the same cited-by count, open access and date whichever
  source's record won the merge (#81). Open access is the more
  informative (a free copy with a PDF over `is_oa: false`); between
  records of one version (a shared DOI or PMID), the cited-by count is
  OpenAlex's when one has it, else the larger, and `issued` the earliest
  date (online first), the more precise of dates that overlap, with the
  other year kept in `other_years`. `verify` now agrees with `lookup`,
  which asks OpenAlex first.

Changed:

- Search hits, and a candidate's `found_by` sightings, leave `score` out
  when the source gives none (OpenAlex sorted by citations), rather than
  `null` (#81). The MCP `search` output schema's `score` is a number, and
  optional, as before.

## 0.19.1

Fixed:

- MCP: open sessions see tool-list changes (#82). The server declares
  `tools.listChanged`; `setKeys` sends `notifications/tools/list_changed`
  to the open sessions of a key whose scopes changed (same id, same
  hash), and `hostToolsChanged` sends it to every open session, neither
  waiting on the agents. `tools/list` is worked out from the key's scopes
  and the app's tools at each request, so a session that lists again
  after the notification gets the new tools (a newly allowed write tool;
  no longer a removed scope's). Keys removed or re-hashed still end their
  sessions (401).

## 0.19.0

Added:

- Agents in an app (#58, P1 and P2): the FFI's `McpServer` (behind an
  `mcp` feature, on in the xcframework) serves the MCP tools over
  Streamable HTTP at `http://127.0.0.1:<port>/mcp` (23130 by default;
  never moved: a port in use throws `portInUse`), through the app's own
  `Aspero` handle. Requests need a bearer key the app minted (`McpKey`:
  id, name, scopes, the token's SHA-256, a daily budget), compared in
  constant time; browsers (an `Origin`) and other host names are
  refused. Scopes (`sources`, `readHistory`, `writeHistory`) decide what a
  key sees and calls. Budgets per session (`sessionBudgetUsd`, $0.25) and
  per key per UTC day (`dailyBudgetUsd`, $1, engine spend, kept in the
  cache file). The app serves its own tools through `McpHost` (`tools()`,
  `async call(name, arguments, caller)`, `activity()`, told of every tool
  call); `setKeys`, `hostToolsChanged`, `status` and `stop`.
- `aspero-mcp`'s `http` feature: the same server for Rust hosts
  (`aspero_mcp::http::HttpServer`, the `Host` trait). The CLI still
  serves stdio only.
- The cache keeps each MCP key's spend per UTC day in an additive table,
  `mcp_spend`, which rebuilds and `clear` leave alone.

- Swift 6: compile the generated bindings in Swift 5 language mode; UniFFI
  0.32's async foreign-trait support (`McpHost`) isn't yet Swift 6 clean
  (see the README).

## 0.18.1

The 0.18.0 release, built: 0.18.0's GitHub release (binaries, the
xcframework, Homebrew) failed to build, because its release commit held a
stray git worktree entry. The code is the same as 0.18.0's.

## 0.18.0

Added:

- Third-party notices, built in (#77). The FFI's `dependencies()` lists
  what the library is built from (`Dependency`: `name`, `version`,
  `license` as an SPDX expression, `repository`): every crate in it,
  aspero's own included, and the bundled CSL styles and locales
  (CC-BY-SA-3.0), by name. `thirdPartyNotices()` gives the full notices:
  each licence text once, after the crates under it and their copyright
  lines, then the citation style data's licence and authors.
- `aspero licences` (or `licenses`) lists the CLI's crates and data with
  their licences (a table, or JSON); `--full` prints its notices.
- CSL-JSON export: the FFI's `ExportFormat.cslJson` (a new, last case)
  and the CLI's `--format csl-json` (lookup, search, verify) write one
  CSL-JSON array, keyed as BibTeX, that Zotero imports and aspero's own
  reader reads back as written: type, title, authors, date, container,
  volume, issue, page, publisher, URL, DOI, ISBN, PMID, PMCID, and an arXiv
  preprint's number. Citation styles format from the same items.
- Style credits as data: the FFI's `CitationStyle` gains `authors` (its
  `<author>`s, then `<contributor>`s), `link` (its page in the Zotero
  Style Repository, `https://www.zotero.org/styles?q=id%3Aapa`) and
  `license` (`CC-BY-SA-3.0`, from its `<rights>`), all with defaults; and
  `styleProject()` gives the Citation Style Language project's name, link,
  licence and deed, and the bundled locales (`en-US`, `en-GB`) and their
  repository. `styleCredits()` is unchanged.
- `just notices` writes both artefacts' notices (the normal dependencies,
  for macOS, from Cargo.lock); tests fail when they're stale, and
  `just release-check` checks them.

Fixed:

- `verify --format cite` (and `csl-json`) gives the works in the input's
  order, not the order their checks finished: numbered styles (Vancouver,
  IEEE) were numbered unpredictably.

## 0.17.1

Fixes from a discovery session (IV morphine titration in the ED).

Changed:

- A bare `aspero` prints an overview: the commands by purpose (check, find,
  explore), examples, and where to read more. `-h` keeps to everyday
  options; `--help` has every option, exit codes and environment.
- Searches prefer Europe PMC to DataCite, when no source is named: a
  search OpenAlex can't say goes to the first that can (`searchCapabilities`
  lists them in that order: OpenAlex, Crossref, Europe PMC, DataCite, Open
  Library).
- A search without a source goes to the first source, in order of
  preference, that can search the whole query (tree, `since`, fields,
  sort), not to the first that searches at all; if none can, the first's
  reason fails, ", and no other source can" (`UnsupportedQuery`, still
  kind `unsupported`). `Engine::search_supports` (FFI `searchSupports`,
  MCP `search_supports`) without a source now says whether any source
  can, not whether the default could; with a source, nothing changes.
  The app asks with an explicit source for saved searches, so they are
  unaffected.
- Related works weigh each citing work's co-citation by 1/√ of its
  reference count, so works listing hundreds (a handbook's 26 chapter
  records, each with ~970 references) no longer make everything they
  cite "related"; `co_cited_with` still counts citing works.
- `Incomplete` (engine) is no longer `Copy`; it gains `stage` (`Stage`:
  citing, coupling, lookup) and `provider`, and JSON `incomplete` adds
  `stage` and `provider` when known. Related works' `done` and `total`
  count the stage that stopped (citing works, coupling pages, works
  looked up), never every citing work read for a failed lookup. The FFI's
  `Incomplete` is unchanged.
- The CLI shows warnings without `-v` (a failed analysis step and why),
  but not each failed request.
- `analyse --format jsonl` prints a line per seed and finding, tagged
  with its `section`, then a `summary` line, instead of one line.
- Search tables show a Cited column when works carry `cited_by_count`,
  and Score only when some hit has one.
- `analyse --kinds` is documented as comma-separated
  (`--kinds related,probably-missing`); an input naming an analysis is a
  usage error saying so.
- `aspero budget` without a key looks one work up (free, past the cache)
  and shows the budget from its headers, "(no key)"; JSON adds
  `"key": false`. The FFI's `refreshBudgets` still asks nothing without
  a key.

Fixed:

- Without a key, OpenAlex refuses a cursor past the first page (HTTP 401
  "needs a free API key"). That counted as a rejected key, which stops
  the engine asking OpenAlex anything, so related works failed their
  coupling page and their lookup (`provider_failed`, done = total, works
  with ids only). Keyless 401/403s are `UnsupportedQuery` now, and
  related works read only first pages without a key (`Capabilities`
  gains `max_pages`).
- Authors: OpenAlex organisation names cut short ("Société française
  d'anesthésie et") give way to the name as printed; "et al." entries
  are dropped; a one-letter family name (OpenAlex's "S. C. S") isn't
  split off; the CLI's first author is a family name or an
  organisation's whole name, not its last word ("et et al.").

Swift bindings: doc comments of `searchSupports` and
`RelatedWork.score` only (`searchSupports`'s checksum changes with its
comment); no shapes changed.

## 0.17.0

Added:

- Budgets say what they're out of (#71): `Usage` and the engine's
  `Budget` gain `limit` (OpenAlex's daily allowance,
  `x-ratelimit-limit-usd`) and `prepaid_remaining`
  (`x-ratelimit-prepaid-remaining-usd`), read from every response, free
  lookups included; `Budget::used` is `limit − remaining`.
  `Engine::refresh_budgets` (FFI `refreshBudgets`) asks OpenAlex's
  `/rate-limit`, which costs nothing, with a key, and sends nothing
  without one. FFI `Budget.limit` and `prepaidRemaining`. `aspero budget`:
  "openalex: $0.0498 of $1.00 used today, resets 00:00 UTC". JSON usage
  gains `limit` and `prepaid_remaining`.
- A daily cap on OpenAlex spend (#72): `openalex_daily_cap` (engine
  `EngineBuilder::daily_cap`, `StandardConfig`, FFI `Config`; CLI
  `--openalex-cap USD` / `ASPERO_OPENALEX_CAP`). Once today's use as
  OpenAlex reports it for the key (`limit − remaining`, shared by
  everything using it) reaches the cap, paid calls stop until the reset:
  searches fail with the new `Error::BudgetCapped { cap, used, resets_at }`
  (kind `budget_capped`, exit 5; FFI `ErrorKind.budgetCapped`, with
  `Failure.budgetCap` carrying `cap`, `used` and `resetsAt`), lookups go
  one free request at a time, and checks fall back to the free sources.
  With a key, a free `/rate-limit` refresh decides the day's first paid
  call; without one, unknown use counts as under the cap. Core
  `ErrorKind::ALL` lists every kind; `Capabilities` gains `budget_status`
  and `max_free_batch`.
- Choosing sources (#73): `StandardConfig::disable` (FFI
  `Config.disabledSources`; CLI `--without europepmc,openlibrary` /
  `ASPERO_DISABLED_SOURCES`) turns sources off by the names results give
  (`SOURCES`, with `source_name` for showing them). They are never built
  or asked; `searchCapabilities` leaves them out, and a search naming one
  fails as `unsupported`, "Europe PMC is turned off" (new
  `EngineError::Disabled`), as `searchSupports` says. An unknown name, or
  turning every source off, is a setup error.
- The cache's trust and size (#74): how long found records and "not
  found" answers are trusted (`StandardConfig::cache_policy`; FFI
  `Config.cacheFoundDays` 7 and `cacheNotFoundDays` 1; CLI
  `--cache-found-days`, `--cache-not-found-days`), and a size limit
  (`SqliteCache::set_max_bytes`; FFI `Config.cacheMaxMb`; CLI
  `--cache-max-mb`): past it, the least recently used records, "not
  found" answers, searches and links go (never the pacing slots), and the
  file shrinks. Cache stats gain `bytes` (FFI `CacheStats.bytes`; `aspero
  cache stats` shows it, in MB of 1,000,000 bytes, beside any limit). Use
  is tracked in a `used_at` column added to existing caches, which are
  kept: no rebuild. New cache files use incremental vacuuming; older ones
  convert the first time they're evicted from.

## 0.16.1

Fixed:

- An organisation author OpenAlex lists as a name ("Atls Subcommittee")
  was split as a person, so citation styles wrote "Subcommittee, A."
  (#69). Subcommittees, taskforces, workgroups, secretariats, trialists
  and collaborators now read as organisations.

## 0.16.0

Added:

- Checking again past the cache (#67), for before submitting: any
  retraction or correction since shows, and a work "not found" before
  but registered since is found. `CallOptions::fresh` (the engine's
  `fresh` option, independent of deadlines and retries), the CLI's global
  `--fresh` and the FFI's `Patience.fresh` (waiting like `batch`) skip
  every cached answer (lookups, "not found", searches, the citation links
  `references` and `analyse` read) and cache the providers' answers in
  their place, so later ordinary calls see them. Where a provider fails,
  the cached answer is served marked `stale`, even one within its
  lifetime. `--fresh` with `--no-cache` is a usage error.
- DESIGN's cache section gives every lifetime: records (and their
  notices) 7 days, "not found" 1 day, searches 1 day, references 30 days,
  citing works 1 day; provider failures and verdicts are never cached.

## 0.15.0

Added:

- Query trees find works by identifier (#65), to combine with anything
  else a tree says: `{"all": [{"id": "10.1038/nature14539"},
  {"not_retracted": true}]}` is that work if it isn't retracted. The
  value is any identifier `lookup` takes; one that isn't fails as a bad
  query, and `phrase` means nothing. Each source takes some kinds:
  OpenAlex DOIs (arXiv ids by their DataCite DOI), PMIDs, PMCIDs and
  OpenAlex ids, any of one kind ORed, none of any (`doi:!…`), but no ISBNs;
  Crossref DOIs only, any of them but neither all of two nor none, and no
  arXiv ids (their DOIs are DataCite's); DataCite DOIs and arXiv ids;
  Europe PMC DOIs, PMIDs, PMCIDs and NBK ids, with any and none across
  kinds; Open Library ISBNs. A kind a source can't take is refused with
  the reason ("Crossref finds works by DOI only", "OpenAlex has no ISBN
  filter"), as `search_supports` says beforehand.
- `TermField::Identifier`, `Filter::Identifier`, `Term::work_id` and
  `IdKind::name` in core; `{"id": …}` (or `"identifier"`) in the JSON tree
  `search --query` and MCP's `search` take; on the FFI,
  `SearchField.identifier`, listed in `searchCapabilities` by every
  searching source.

## 0.14.0

Added:

- Related works (#60, phase G2): `aspero analyse --kinds related`
  (`--limit`, default 20) lists works around a set, by co-citation (citing
  works read that cite the work and a seed) and bibliographic coupling
  (the set's references it cites too), scored 0.8 and 0.2 of each as a
  share of the highest, each with `co_cited_with`, `shared_references` and
  the `seeds` linking it. They come from OpenAlex's citing works, 200 a
  page ($0.0001): seeds cited at most 200 times share a page, a seed cited
  more than 1,000 times is sampled (its 1,000 most cited citing works:
  `incomplete: {kind: capped, done, total}` in citing works), at most 75
  pages a set, plus five pages of works citing the set's most shared
  references. Works of the set (by any identifier), retracted works and
  versions drop or merge. Eleven sepsis trials cost $0.006 and 16 s; a
  rerun within a day is free (citing works are cached in `edges`).
  `--estimate` counts the citing pages exactly and the rest as a ceiling
  ("at most").
- `AnalysisKind::Related { limit }` and `Analysis::related` in the
  engine; `related` in `aspero_json::graph`; on the FFI,
  `Analysis.related` (`RelatedWorks`, `RelatedWork`; defaulted to `nil`)
  for `AnalysisKind.related`, the best 20.
- OpenAlex lists take `select`, and read up to 200 works a page.
- Searches for only works added since a day, for watching a search
  (#62): `aspero search --since 2026-11-01`, `SearchQuery::since`, and on
  the FFI `SearchRequest.since` (`YYYY-MM-DD`; any other text fails as
  `badRequest`). "Added" is when the source took the work, whenever it
  was published: Crossref's first deposit (`from-created-date`), DataCite's
  registration (`created`), Europe PMC's first indexing (`FIRST_IDATE`).
  OpenAlex (it needs a paid plan) and Open Library can't, and fail
  (`unsupported`) saying why, rather than ignoring it; without a source,
  the first that can answers (Crossref). `Filter::Since` and
  `SearchFilter.since` say which sources can, and `searchSupports` takes
  an optional `since` to ask beforehand (`Engine::search_supports_since`).
  A search's `total` already counts what's new.

Changed:

- `analyse` no longer reports `related` under `not_yet`.

## 0.13.0

Added:

- Citation links, from OpenAlex's `referenced_works` (#60, phase G1).
  `aspero references <id>` pages through the works a work cites, each
  looked up (batches of 100, $0.0001 each), with `coverage`: `known`, or
  `unknown` when OpenAlex has no reference data for the work. OpenAlex
  lists none for those and for works citing nothing alike, so an empty
  list reads as unknown.
- `aspero analyse <ids…|file>` (`--kinds retracted,odd-one-out,probably-missing`,
  `--limit`, `--budget`, `--estimate`): analyses of a set of works, given
  as identifiers or a .bib, .ris or CSL-JSON file. Seeds resolve cheaply
  (cached, else batched, asked once) or say why not (`invalid_id`,
  `unsupported_id`, `not_found`, `provider_failed`, `over_budget`);
  preprint and published versions count as one work. Retracted
  references come from OpenAlex (100 per $0.0001 request) and notices
  already cached; odd ones out are `no_links` or `no_reference_data`;
  probably missing works are those two or more works of the set cite,
  with which. Stopped steps say how far they got (`incomplete: {kind,
  done, total}`); `--estimate` costs nothing. `related` is not yet.
- On the FFI, `references(id:cursor:limit:patience:)` and
  `analyse(seeds:kinds:budgetUsd:estimateOnly:patience:)`, with
  `ReferencesPage`, `Analysis` and their records; the JSON contract's
  shapes are in `aspero_json::graph`.
- `SourceRecord::references`; the cache keeps citation links in a new
  `edges` table (references 30 days, citing pages 1 day).

Fixed:

- OpenAlex batch lookups missed works found by a DOI their record no
  longer carries (re-registered after a change of publisher, such as
  Academic Emergency Medicine's `10.1197/…`), though a lookup of the DOI
  alone found them; the record now answers that DOI. Seeds OpenAlex still
  misses are looked up as `lookup` does and asked about again by the
  OpenAlex id, DOI or PMID found, and works sharing any identifier with a
  seed are never probably missing.
- Estimates count the seeds' batches when there's no cache for the run
  to find them in.

Changed:

- The cache is rebuilt once (schema 10): cached records now carry the
  works they cite.

## 0.12.0

Added:

- `aspero mcp`: aspero's tools for AI agents over the Model Context
  Protocol, on stdio (#58): `claude mcp add aspero -- aspero mcp`. Tools:
  `search` (a query tree, as `search --query` takes it, or keywords),
  `search_supports`, `lookup`, `verify` (one citation or a batch),
  `identify`, `citing_works` and `references`. Results are the JSON the commands
  print, as structured content with output schemas, plus a short text
  summary; `full` for every field. A new crate, `aspero-mcp`, holds them,
  on the official Rust SDK (`rmcp`).
- Each MCP session has a spend cap, `aspero mcp --budget <usd>` (default
  $0.25); once spent, tools that may spend are refused with a
  budget error. Every result reports the call's usage and the session's
  spend.
- `aspero-json`: `QueryTree`, the query tree's JSON form as a serde type,
  and with the `schemars` feature, JSON Schemas for it and `ClaimInput`.

## 0.11.1

Fixed:

- The OpenAlex request that crosses the budget could come back as a
  server error; any failing response that says the budget is spent
  (`not_enough_credits`, or none remaining) is now `QuotaExhausted`.

## 0.11.0

Added:

- Search by a query tree: all, any and none of terms, filters and
  relations, nested (#54). `aspero search --query '<json>'`; on the
  FFI, `SearchRequest.query` (a `SearchNode`), instead of the keywords
  and fields. Each source says what it can do in `searchCapabilities()`
  (`any`, `none`, `nesting`, `relations`), and `searchSupports(source,
  query)` says, offline, whether a tree can be searched and if not why
  ("OpenAlex can't combine title and author with any"); searching anyway
  fails (`unsupported`) with the same reason.
- Fields: keywords, abstract, and author id (an ORCID, or an OpenAlex
  author id). Filters: not retracted, language (ISO 639-1, given to each
  source in its own form) and has an abstract. Relations: works citing,
  or cited by, a work named by any identifier aspero looks up (resolved,
  through the cache, to its OpenAlex work).

  | Source | Any | None | Fields | Filters | Relations |
  | --- | --- | --- | --- | --- | --- |
  | OpenAlex | within one field or filter | yes, not years | all; author id by ORCID or OpenAlex id | all | cites, cited by |
  | Crossref | no | no | keywords, title, author, container, ORCID | years, type, abstract | no |
  | Europe PMC | yes | yes | all; author id by ORCID | all but type | no |
  | DataCite | yes | yes | all but container; author id by ORCID | all but open access and retraction | no |
  | Open Library | yes | beside something to find | keywords, title, author | years, language | no |

- Newest first, a tree is searched up to this year unless a years
  filter it always applies (the tree, or one of its top-level all)
  gives an upper year; a first year alone gains this year as its last.

## 0.10.1

Fixed:

- A record whose title only partly agrees, with nothing else agreeing
  ("Journal of the American College of Surgeons" for a book by the
  College), scored plausible; it's now no evidence (#52).
- Another edition of a cited book (same title and authors, another
  year) was a mismatch; it's plausible, the year a difference. A book
  cited by its 9th edition no longer turns on which searches answered
  in time (#52).

## 0.10.0

Added:

- Search by author, title and container (the journal, or the book for a
  chapter), ANDed with the keywords, which may then be left out; and a
  sort: relevance, newest or most cited. `aspero search --author --title
  --container --sort`; on the FFI's `SearchRequest`, with `extra` for
  provider-specific parameters. A provider that can't search a field or
  give the order asked for fails (`unsupported`) rather than widening
  the search.

  | Source | Author | Title | Container | Newest | Most cited |
  | --- | --- | --- | --- | --- | --- |
  | OpenAlex | yes | yes | yes ($0.001 more) | yes | yes |
  | Crossref | yes | yes | yes | yes | yes |
  | Europe PMC | yes | yes | yes | yes | yes |
  | DataCite | yes | yes | no | no | yes |
  | Open Library | yes | yes | no | yes | no |

- Newest first, without an upper year, searches up to this year, so
  records dated ahead (deposits, early issues, typos like 2115) don't
  lead. Crossref pages date sorts by offset (it refuses them with a
  cursor), carried as `offset:N`; its order within one date isn't
  stable, so such pages may repeat or skip works of that date.
- A search with nothing to search for (no keywords, fields or provider
  parameters) is refused as a usage error. Provider parameters alone
  (an OpenAlex `filter`) are a search.
- DataCite's citation count of 0 is none: it counts only citations it
  tracks.
- FFI: `searchCapabilities()`, each searching source's fields, filters
  and sorts, in the order `search` picks from; `Work.citedByCount`.
- JSON: lean works carry `cited_by_count`.
- Library: `SearchQuery` `author`, `title`, `container` and `sort`;
  `Filter::Author`, `Title`, `Container`; `Sort`; `Capabilities.sorts`;
  `Engine::search_providers`; `ALL` and `as_str` on `Filter` and `Sort`.

## 0.9.0

Changed (library):

- Batch verification has one engine path, `Engine::verify_batch`, used
  by the CLI, the FFI and `check`. `Engine::check` returns a
  `CheckReport` (findings and usage).
- `check`'s findings and their severities (bib notes, skipped entries)
  are the engine's (`check_entries`); its JSON shape is
  `aspero_json::check`. The OpenAlex key check is the engine's
  (`check_openalex_key`). The engine and json crates depend on
  aspero-bib.
- `CslItem.isbn` is `Option<OneOrMany>` (Crossref sends arrays); CSL
  names read `name` as well as `literal`.
- New helpers: `ArxivId::doi`, `Doi::arxiv`, `orcid`, `Person::inverted`,
  `Candidate::merged_record`, `LookupResult::served`, `add_usage`,
  `StandardConfig::all`, `SEARCH_LIMIT`, and `ALL` on the engine's enums.

Fixed:

- `aspero check` left the batched OpenAlex lookup out of its usage.
- A record carrying two candidates' identifiers merged into the first
  only, so one work could rank twice; candidates now merge transitively.
- Crossref and doi.org mapped work types differently (`reference-entry`,
  `book-set`, `report-component`, `database`); an abstract of tags alone
  is none. ORCIDs from OpenAlex and DataCite given as `http://` URLs, or
  empty, are normalised as elsewhere. The cache schema is rebuilt (9).
- `aspero check -` on a terminal waited for input; it now says to pass a
  file or pipe one.
- FFI: a candidate's `Work.json` carries its merged identifiers and
  notices, as JSON candidates do.

## 0.8.6

Fixed:

- A claim of an identifier with a wrong year, journal, volume or page
  verified (at confidence 0). It's now plausible, the field a difference.
- Claimed authors in Vancouver form ("Daum RS") or with particles
  ("A. van der Berg") scored no overlap with their records.
- `identify` took a body mention over the header's identifier when both
  records carried the title.
- A claim using "response to" or "correction" as ordinary words lifted
  the derivative penalty: claims now read markers as records do.
- A claim giving only one of volume and page was searched by one place
  and scored by another.
- The CLI panicked on a closed stdout (`aspero scan … | head`); it now
  exits quietly.
- A huge `Retry-After`, deadline or breaker cooldown, or an extreme
  year, could overflow and panic. Engine locks survive poisoning.
- One record that doesn't decode, or a `null` field, failed a provider's
  whole page; the record is skipped, the `null` read as absent.
- The cache recovers from an unreadable file, and rebuilds an outdated
  schema under the write lock.
- Europe PMC batch lookups could miss a record pushed onto a second page.

Faster:

- Splitting a long unnumbered paragraph of references is linear: 256 KB
  in 2.6 ms, from 408 ms.
- FFI UTF-16 spans no longer re-encode the text for each mention.

## 0.8.5

Added:

- `Verification.read`: the claim as aspero read it, for a "cited vs
  found" view. Identifiers as written (a DOI cut short at a line break
  stays visible, with the readings tried as `alternatives`); from free
  text, the year (when it gives one), volume and first page
  (`Claim::as_read`). Title, authors and journal aren't parsed out of
  free text: they're scored by finding the work's own in it.
- `Difference.cited`: the citation's value for the field, as compared
  (the year nearest the work's, the volume or first page read, or the
  title, authors or journal given).
- JSON: `"read"` on each verification, `"cited"` in `why.differs`. FFI:
  `Verification.read: Claim?`, `Difference.cited: String?` (#46).

## 0.8.4

Fixed:

- An unnumbered reference whose authors fill its first line (ending
  "et al.") was split from its title line when that started like an
  author ("National, regional, and global trends…"). A surname's comma
  now needs a capital after it, and a reference so far without a year or
  identifier doesn't end there (#44).

## 0.8.3

Added:

- `Person::parts` (family and given names, split from a name written
  whole with the rules BibTeX export used, `None` for an organisation),
  `Person::initials` ("DV") and `Person::short`, the Vancouver form
  ("Laan DV"). FFI: `Author.family`, `Author.given` and `Author.short`;
  an `Author` passed back to `export` or `cite` with a family name keeps
  its parts (#41).

Fixed:

- A reference list stayed one reference when each DOI's end was wrapped
  onto a line of its own (`…/CIR.000000000` then `0000659`). A line that
  is one word of DOI-suffix characters with a digit, not a plain number
  or an identifier of its own, continues the link that ended the line
  before, so the reference ends in the whole DOI and `scan` reads it
  whole (#40).

## 0.8.2

Fixed:

- A DOI a PDF wrapped part-way through its suffix (`…/HJH.0b013e` then
  `3282f35c67`) was looked up as printed and reported as not existing,
  a red mismatch against the very work found. A DOI ending its line now
  offers the next line's first word joined to it as an alternative
  reading, `Claim::from_text` carries mentions' alternatives, and a cited
  DOI that doesn't exist but starts the found work's DOI is that DOI cut
  short (#39).
- An unnumbered reference list whose lines break with a lone `\r` or
  U+2028/U+2029 (as some PDF copies do) stayed one reference: those
  breaks are now newlines. Laid-out lines (`split_lines`) no longer start
  a reference at a line in lower case or closing a bracket, or after a
  word broken at a hyphen (#40).

## 0.8.1

Added:

- `WorkKind` (reply, correction, comment, retraction, expression of
  concern, recommendation): a `CitedField::Kind` difference's `kind`, the
  value `actual` words, so front ends can match on it exhaustively.
  `matching::derivative_work_kind` returns it. FFI: `Difference.kind:
  WorkKind?`.

## 0.8.0

Added:

- `identify`: which work a document is, from its first page's text, not
  the works it cites or links to. Each identifier on the page (header and
  footer first, at most six) is looked up and must be confirmed by the
  title given (the largest type on the page, or title metadata unless
  junk such as "Microsoft Word - draft3.docx") or by its own title and
  first author appearing on the page; with none confirmed, the top of the
  page is verified as a citation. It answers with the verification and
  `decided_by`: `identifier_and_title`, `identifier_and_page`,
  `only_identifier` or `title_search` (#24).
  - FFI: `identify(firstPage:, title:, titleHint:, patience:)` →
    `Identification? { decidedBy: IdentifiedBy, verification }`.
  - CLI: `aspero identify page.txt --title … --title-hint …` (exits 3
    when nothing is sure enough).

Fixed:

- `aspero scan --help` no longer says a DOI broken across lines isn't
  found.

## 0.7.0

Added:

- Citations without a title are found by where they appeared: running
  headers (`Injury, Int. J. Care Injured 47 (2016) 797–804`, `Nature 521,
  436–444 (2015)`) and short forms (`Injury. 2016;47(4):797-804.`).
  Journal, volume and first page agreeing verify; another page or volume
  is not found, not a guess. `verify` searches OpenAlex by volume, first
  page and year (±1) for them (`SearchQuery::place`, `Filter::Place`;
  found-by `place_search`) (#23).
- `Reason::Unconfirmed` (`"unconfirmed"`): the citation agrees with the
  work as far as it goes, but gives too little to confirm it. Before, this
  was `partly_matches` with no differences.
- `CitedField::Kind` (`"kind"`): the closest work is a reply, correction or
  commentary on the one cited; `actual` says which ("reply",
  "correction", "comment", …). `matching::derivative_kind` names it.

Changed:

- `partly_matches`, `closest` and `id_partly_matches` always list at least
  one difference. A closest work differing in nothing nameable is
  `not_found`. **FFI**: `Reason.unconfirmed` and `CitedField.kind` are new
  cases.
- A reference without a title (authors, journal, year and an identifier,
  say) no longer counts as citing a different title, and one naming no
  authors no longer counts as naming others ("authors mismatch" on a
  running header).

## 0.6.7

Fixed:

- `scan` found nothing in a DOI link that macOS Preview copies with
  spaces after its punctuation (`https:// doi. org/ 10. 1183/ 23120 541.
  00489- 2021`); such a link is now mended, and `verify` of it alone
  checks the DOI (#33).
- `verify` of that link was a mismatch against a placeholder record
  titled with a URL. A URL is no title, and a link's words (`https`,
  `doi`, `org`, …) no evidence of one; a year or journal agreeing alone
  no longer makes a work plausible either (#33).
- `split_references` gave up splitting references numbered inline when
  one of them had no year ("Defense Health Board … Falls Church, VA,
  n.d."). `n.d.`, "Accessed …" and "[cited …]" now count as a date, and
  one undated piece among four or more no longer stops the split (#35).
- Words inside a title no longer mark it a reply or correction
  ("differential response to corticosteroids", "Correction of
  hyponatraemia"): "response to", "comment on" and "correction" count
  only where a title starts with them.

## 0.6.6

Fixed:

- `split_references` read a list copied from a PDF onto one line ("…
  01159.x. [21] Finucane … [22] …", as Preview copies) as one reference.
  A line is now split before inline `[n]` markers in sequence (three or
  more, or two from the line's start), each after a full stop and before
  a capital, when every piece has a year or identifier; text before the
  first number is kept as a reference when dated (#31).
- A title of one or two words found inside free text no longer makes a
  work plausible by itself or with only a year: `verify "aspero 0.6.5 is
  in"` matched an encyclopedia entry titled "Aspero". It takes the
  authors, the journal, the volume and page, or an identifier (#29).

## 0.6.5

Fixed:

- Titles and journal names from Crossref (and OpenAlex, DataCite,
  Europe PMC) kept their HTML entities and JATS tags (`Anesthesia &amp;
  Analgesia`, `<i>E. coli</i>`), into citations, BibTeX and RIS. They
  are plain text now. Abstracts no longer gain a space where an inline
  tag was (`H<sub>2</sub>O` is `H2O`), and a `<` that opens no tag
  (`age < 65`) is kept (#27).

## 0.6.4

Fixed:

- `scan` found nothing in a DOI link broken across lines right after
  the DOI's prefix (`http://dx.doi.org/10.1017/` then
  `S1049023X15004653.`), as PDFs break them. A link ending in `/` at the
  end of a line now continues onto the next (#25).

## 0.6.3

Changed:

- `scan` is about 100× faster: ~1 GB/s on prose, ~200 MB/s on dense
  reference lists (was ~5.5 MB/s), with the same results, and no longer
  needs 16 bytes of memory per byte scanned. `split_references`, and
  parsing identifiers, are faster too.

Fixed:

- `scan` joined a link ending a line to the start of the next line
  when that was another link (`https://…`, `www.…`, `arXiv:…`, a DOI),
  the next reference's number (`18.`), a hyphenated word or a number.
  A link continues onto the next line only if that keeps it the same
  kind of identifier and changes it.

## 0.6.2

Added:

- Word documents: the citations Zotero and Mendeley (Desktop) keep in a
  .docx file's field codes, read as claims with their identifiers, each
  work once, from the body, footnotes and endnotes. `aspero check
  paper.docx`; FFI `readDocx(data)` → `DocxReferences { entries,
  manager, citations, text }`, `text` being the document's paragraphs
  for splitting its reference list when it has no fields. Mendeley
  Cite's content controls and EndNote's fields aren't read yet.
  (`aspero-bib`'s `docx` feature.)

## 0.6.1

Added:

- Citation styles, offline, by the hayagriva CSL processor, with eight
  styles from the official CSL repository bundled: APA 7th, Vancouver
  (NLM), Harvard (Cite Them Right), Chicago author-date, IEEE, Nature,
  AMA and MLA. Any independent `.csl` style can be used too. Styles and
  locales are CC BY-SA 3.0 (`crates/bib/styles/NOTICE.md`).
  - CLI: `--style <id|file.csl>` (implies `--format cite`) and
    `--locale en-GB` on `lookup`, `search` and `verify`; `aspero styles`
    lists the bundled ones, `--credits` prints their license and authors.
  - FFI: `citationStyles()`, `cite(works, style: StyleSource, format:
    CiteFormat, locale:)` → `[CitedEntry { index, label, text, inText }]`
    in the style's order (`.text` or `.html`), `styleCredits()`; throws
    `StyleError` (`unknownStyle`, `dependent(parent)`, `invalid`).
  - `aspero-bib`'s `styles` feature: `cite::{Style, cite, credits,
    BUNDLED}`.

## 0.6.0

Added:

- `check` reads RIS and CSL-JSON as well as BibTeX, telling them apart
  from the contents, and compares each entry's journal, volume and
  pages too (BibTeX's `journal`/`journaltitle`, `volume`, `pages`).
  PMIDs are read from BibTeX `pmid` or `eprinttype = {pubmed}`.
- `--format bibtex|ris` on `lookup`, `search` and `verify`: the works
  found, for a reference manager. `verify` writes the work each citation
  refers to (verified, or found under other identifiers than those
  cited) and names the rest on stderr. Whole names ("Yann LeCun") are
  split into family and given names; organisations stay whole.
- `check`'s findings about an entry's own fields carry `field`, `value`
  and `problem` (`not_a_doi`, `not_an_arxiv_id`, `not_an_isbn`,
  `not_a_pmid`).
- FFI: `readReferences(text)` reads a BibTeX, RIS or CSL-JSON file into
  `Bibliography { format, entries }`, each entry's `claim` ready to
  verify with `key` set to the entry key, and its `notes` as `BibNote {
  field, value, problem, message }`; it throws `ReadError.unrecognised`
  or `.invalid(format, line, message)`. `export(works, format)` writes
  works as BibTeX or RIS.

Changed:

- A citation whose identifier is the right work, with the same title and
  matching authors but a wrong year, is `plausible` /
  `id_partly_matches` naming the year, not `mismatch` / `id_disagrees`.
  `check` still reports a year off by more than one as an error.

Changed (breaking):

- `aspero-bib`: `BibError` is now `ReadError` (`Unrecognised`, or
  `Invalid { format, line, message }`); new `read`, `detect`,
  `parse_ris`, `parse_csl_json`, `bibtex`, `ris`, `cite_key`, `Keys`.
  `BibEntry::notes` are `Note { field, value, problem }`.

## 0.5.2

Faster:

- `verify` stops waiting for the remaining title and keyword searches as
  soon as a candidate verifies, instead of waiting for the slowest
  provider (Europe PMC's search can take 3–11 s). A citation whose DOI
  names another work now settles once two providers agree on the work
  it cites: 11–17 s → 3–5 s live; a citation without an identifier, 4–17 s
  → 2.5–4.5 s.

## 0.5.1

Added:

- `scan`, `split_references` and identifier parsing clean text from PDFs
  and word processors first: ligatures (`ﬁ`) expanded, soft hyphens and
  zero-width characters dropped (a soft hyphen at a line end marks a
  broken word), Unicode hyphens, dashes, quotes and spaces folded to
  ASCII. Spans still point into the text as given.
- A DOI broken across lines at a hyphen is kept as printed, and its
  reading without the hyphen is tried if the printed one doesn't exist:
  `Claim::alternatives`, `Reference::alternatives`,
  `Mention::alternative` (JSON `scan`: `alternative`; FFI:
  `alternativeIds` on `Claim` and `Reference`, `alternativeId` on
  `Mention`). `verify --split` passes them on.
- `split_lines` (FFI `splitLines`): splits a reference list's lines as
  laid out, using their indents (hanging or first-line) where the text
  alone can't tell; each reference gives its range of line indices.

Faster:

- `verify` no longer waits for citation matchers (Crossref's takes
  seconds) once the claim's identifiers alone verify it: a cold verify
  of a correct citation with its DOI takes about 0.4 s instead of 2–4 s.
- Lookups stop at a final "not found" (doi.org for DOIs): a DOI that
  doesn't exist is no longer searched for at Europe PMC, which took up
  to 5 s.
- Crossref gets one request in flight at a time without a contact
  address (its public pool refuses a second with 429), a few with one.
- A DOI's alternative reading is looked up alongside the printed one in
  `verify`'s first stage instead of before it.

Fixed:

- A lone numbered reference (`1. LeCun Y. …`) loses its number.
- `scan` rejoins a DOI or URL broken across lines (as in text from PDFs)
  by the rules `split_references` uses; the mention spans both lines.
  A link split mid-word is rejoined only before more of a link, never
  before a word or a year.

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
