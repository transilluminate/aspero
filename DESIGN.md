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
  generic seams; they are never dependencies. Those seams (full text, a
  durable store of derived data) are designed with their first producer
  in hand, not ahead of it.
- **Work identifiers are a contract.** Callers store canonical `kind:value`
  strings durably and join on them, so a change to an identifier's
  canonical form (its normalisation or display) or to the ranking in
  `id::canonical` is a breaking change: it needs a migration story (old
  form to new, for stored keys) and a minor version, not only a changelog
  line. Property tests pin that display re-parses to itself.

## Architecture

```
crates/
  core/        identifiers, Work model, Claim scoring, Provider trait, errors,
               CSL-JSON reader, HTTP seam. No I/O.
  providers/   one crate per source, each behind an engine feature:
               openalex, crossref, datacite, doi (doi.org), europepmc,
               openlibrary
  store/       disposable SQLite answer cache; the benchmarks' HTTP tape
  bib/         BibTeX/BibLaTeX (via the `biblatex` crate), RIS, CSL-JSON →
               claims, and .docx citation fields (`docx` feature: Zotero,
               Mendeley); works → BibTeX, RIS, CSL-JSON (one writer, also
               feeding the styles), and citation styles (`styles`
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
- **Search** goes to one provider: the one named, or else the first, in
  order of preference, that can search the whole query (its mode, tree,
  `since`, fields and sort: the same checks as a named search, no calls);
  if none can, the first's reason fails, ", and no other source can"
  (`UnsupportedQuery`). `search_supports` without a source likewise asks
  whether any can (before 0.18, whether the first could). Filters a
  provider can't apply are stripped and reported as ignored. Fields
  (author, title, container) and sorts (newest, most cited) it can't
  apply fail (`Unsupported`): dropping one would widen or reorder the
  search silently. Relevance never fails; with only fields to rank by, it
  is the source's own order.
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
  groups flattened, an all or any of one node unwrapped, a none of one
  none read as any of what it held), checks its
  values (author ids, language codes, year ranges, identifiers, and work
  types beyond aspero's own: letters, digits and hyphens, as the request's
  type is too, never a source's filter syntax), folds the
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
  all) has an upper year; years under any or none don't count. The cap
  never changes what a source can search: the tree as asked decides, as
  `search_supports` (which takes no sort) says, so Open Library's none
  alone stays refused newest first too.
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
  CorpusId, ISBN, NCBI Bookshelf (NBK), Elsevier PII. Each parses bare,
  `kind:`-prefixed and URL forms into one canonical value; display
  re-parses to itself (property-tested). Bare digits are ambiguous and
  refused, except checksum-valid ISBN-13s.
- **Publishers' article URLs** (#95), as pasted or dragged from a
  browser, read as the identifier they show (query, fragment, trailing
  `/` and `.pdf` aside):
  - a DOI in the path (Wiley, NEJM, Frontiers, Springer and BMC's
    `/article/10.…`), page suffixes (`/full`, `/pdf`, …) dropped;
  - Nature's `nature.com/articles/<suffix>`: `10.1038/<suffix>`, for
    every shape seen (`nature14539`, `s41586-020-2649-2`, `ncomms1234`,
    `srep01234`, `nbt.3122`, `35057062`, `171737a0`, `d41586-…`), but
    `10.1057` for the Palgrave journals hosted there (`s41599-…`;
    `palcomms20162` is `palcomms.2016.2`). Older pages named by only part
    of their DOI aren't read rather than read wrong (`/articles/6605004`
    is `sj.bjc.6605004`, `/articles/ncb0200_57` is `35000001`);
  - Elsevier's PII (`pii:`), from ScienceDirect's
    `/science/article/pii/<PII>` (and `/abs/pii/`, `/am/pii/`),
    `linkinghub.elsevier.com/retrieve/pii/`, Cell's `…/fulltext/S…`
    (`/abstract/`, `/pdf/`) and the Lancet's `…/article/PIIS…/fulltext`.
    Canonically compact, `S` and 16 characters (an X only where the
    ISSN's or the PII's check character is), as Crossref holds it:
    `S0140-6736(20)30183-5` is `S0140673620301835`. Journal PIIs only:
    a book chapter's `B…` isn't read. Only Crossref looks
    PIIs up (its `alternative-id` filter, one free list call for many:
    the compact form, upper case, finds them; the punctuated form and
    lower case find nothing, checked 2026-11-15 on ScienceDirect, Lancet
    and Cell PIIs). The engine then looks the PII's DOI up through the
    whole chain, so a PII's lookup is that DOI's (OpenAlex's record,
    usually), the PII added to its identifiers, and verdicts and merging
    are a DOI's; where the DOI isn't found again, Crossref's record
    stands. A PII Crossref doesn't know is not found;
  - arXiv's `/abs/`, `/pdf/` and `/html/`, a version and `.pdf` kept or
    dropped as written.

  Not read: JAMA Network's `/fullarticle/<number>` (Crossref holds the
  number only in the record's resource URL, which can't be filtered on,
  and no `alternative-id`: checked 2026-11-15; nothing else free maps
  it); Oxford Academic's `/article/<vol>/<issue>/<page>/<id>` (only the
  page's `citation_doi` says, and aspero doesn't fetch pages: a caller
  that has the page hands over its markup, see Pages); OpenReview
  (its ids aren't in Crossref, nor in any other source aspero asks).
- **Pages** (`webpage::claim_from_page`, FFI `claimFromPage`, `verify
  --html FILE --url URL [--title TEXT]`): a claim from a publisher's
  article page as a browser rendered it, for the addresses above that
  don't say. The caller has the page's markup (its `<head>` and JSON-LD
  scripts will do), or often only its address and the title a browser
  gave it (a window's, a shared page's or a dropped link's); aspero never
  fetches a page, and the function is pure (no network, no clock). Read,
  each claim field from the first that gives it: Highwire `citation_*`
  tags (DOI, PMID, PMCID, arXiv,
  PII, ISBN; title; each author; journal, else its abbreviation, the
  conference or the book; volume; first and last page; the year of the
  publication date, else `citation_date`, else the online date), PRISM
  (`prism.doi`, publication name, volume, pages, date), Dublin Core (`dc.`
  or `dcterms.`, any case: identifier, title, creator, date, and source as
  the journal only when it reads as a name), then schema.org JSON-LD (the
  first `ScholarlyArticle`, `MedicalScholarlyArticle` or `Article` at a
  block's top, in its `@graph` or a page's `mainEntity`, never one nested
  deeper such as a citation: identifier, `sameAs`, `@id`, `name`, else
  `headline` (it names the thing; pages fill `headline` with a
  description, Wikipedia's with the article's short description, #118;
  a `name` that isn't a title, an address or "Article", passed over),
  authors, `datePublished`, pagination, and up `isPartOf` only a
  `PublicationVolume`'s number and a `Periodical`'s name, not a
  `WebSite`'s). For a title only: then `og:title`, else `<title>`, then
  the title given. Every identifier any of them gives (each value parsed
  as the identifiers are, so normalisation is shared, or scanned), then
  the address's as `scan` reads it, is claimed, each once; only the
  address's are `from_url`. `source` is the first vocabulary in that
  order that gave an identifier or the title (`url` when none did). With
  no identifier and no title there's no claim, rather than a junk one.
  The journal is only ever the page's metadata, never guessed from the
  host: a wrong one makes `verify` report a journal mismatch and cap at
  plausible. The host is the claim's `site` instead (lower case, `www.`
  dropped; always, identifiers or not), a hint of the publisher (see
  Resolve, verify and check: Sites). Authors keep page order and their
  written form ("Last, First" or "First Last", as claims take them),
  blanks and "et al." dropped, at most 50.
  - The markup is read by a small hand-written scanner, not a parser
    (and no new dependency): tags and attributes in any quoting and case,
    values entity-decoded (`&amp;`, `&#39;`, `&#x2019;`, common named
    ones; then only formatting markup taken out of them, HTML's and
    JATS's inline tags such as `<i>`, `<sub>`, `<span>`, `<italic>` and
    any namespaced one, which publishers write escaped as often as not,
    so a tag's name written as text stays: MDN's "&lt;meta&gt; HTML
    metadata element" is "<meta> HTML metadata element"), comments,
    declarations and closing tags skipped, and the
    contents of `<script>` (except `application/ld+json`, parsed with
    serde_json, CDATA wrapping aside), `<style>`, `<textarea>` and
    `<template>` never read as markup, so a `citation_doi` inside a
    script's string is not the page's. Anything scans: a whole page, a
    `<head>`, a fragment, junk. Caps: the first 2 MiB, 20,000 tags, 32
    JSON-LD blocks; values over 1,000 characters are ignored. JAMA's
    head with its inline scripts is ~145 KB.
  - Titles (`<title>`, `og:title`, the title given) lose the site's
    furniture, segment by segment from the end, after ` | `, ` - `, ` – `,
    ` — `, ` · `: a known site or browser name (`JAMA Network`,
    `ScienceDirect`, `Oxford Academic`, `Wiley Online Library`, `Google
    Chrome`, `Mozilla Firefox`, …), the host's name (`The Lancet` on
    `thelancet.com`), a name the page gives its own site (`og:site_name`,
    JSON-LD's website or publisher), exactly but for case and
    punctuation, and whole even when it holds a separator (`- Mozilla
    Hacks – the Web developer blog`), a year or names ending in one
    (Wiley's and Sage's
    bylines), `Proceedings of …`, or a short title-cased segment (five
    words, 40 characters) after a title of three words or more (`| Brain
    |`). Prefixes go when they're a known site or the host before ` | `
    (`Frontiers | …`), one of a few sites before `: ` (`ScienceDirect:
    …`), `(PDF) `, `Full article: `, MDPI's `… | Free Full-Text | ` or
    arXiv's `[1706.03762] `; Cell Press's `: Cell` suffixes too. A colon
    in a title otherwise stays the title's. What's left must be a title:
    not link text or furniture ("Full Text", "PDF (2 MB)", "Abstract",
    "Download PDF", "View Article", "here", "doi"), a challenge or error
    page ("Just a moment...", "Page not found"), a site's name alone, an
    address or identifier; and a given title (a dragged link's name) at
    least three words and 15 characters, a page's own two words. The
    title is as the page gives it, minus its site's own name: only
    evidence takes a segment off (no site-by-site rules are added for
    web pages), and when unsure it stays, since nothing downstream edits
    it (a web page's goes to Zotero as read).
  - *Scholarly work or web page* (#119). A page is a scholarly work's
    (`PageKind::Work`, FFI `kind` `work`) when it gives an identifier
    (in its tags, JSON-LD or address; an ISBN too, so a bookshop's page
    tagged with one cites the book), citation tags (Highwire's with a
    title, journal or identifier; PRISM's or Dublin Core's with a
    journal or identifier: Dublin Core's title and creator alone are any
    page's), or a `ScholarlyArticle` or `MedicalScholarlyArticle` in
    JSON-LD; when it's on a publisher's site (Sites' table: an article
    page giving only its title); or when no markup was given, so nothing
    tells. Any other page with a title is a web page (`WebPage`): it has
    no claim, so an app doesn't verify it as a work and report "not
    found"; it is cited as itself. A work's page with a title gets the
    record too, so the page can be cited when its work isn't found.
  - *The web page record* (`webpage::WebPage`, FFI `WebPage`), each field
    from the first that gives it. Title: a JSON-LD article's (any
    article, news article, post or report) `name`, else `headline`;
    `og:title`; `<title>`; a JSON-LD page's `name` (less sure: WHO names
    its pages "Detail", the template's); the title given; each as above.
    Site: `og:site_name`, JSON-LD's `WebSite` (the article's `isPartOf`,
    else any), the site's name as the title writes it when that names
    the host ("Wikipedia" of "Gene Hackman - Wikipedia" on
    `en.wikipedia.org`), JSON-LD's publisher, else the host. Address:
    `<link rel="canonical">`, `og:url`, JSON-LD's `url`, else the one
    given, resolved against it (relative, `//host/…`) and normalised:
    scheme and host lower case, default port, fragment and tracking
    parameters (`utm_*`, `fbclid`, `gclid`, `msclkid`, …) dropped, the
    rest of the query kept in order. Authors: people only, in page
    order: JSON-LD's `Person` authors, else `author` meta tags, else
    `article:author` ones that aren't addresses; never an organisation
    (JSON-LD's, the publisher, the site), a byline holding one's name
    ("Guardian staff reporter", then the page's `article:author`), or
    "Contributors to …". Published: `datePublished`,
    `article:published_time`, `citation_publication_date`,
    `citation_date`; updated: `dateModified`, `article:modified_time`,
    `og:updated_time`; as written (`2001-09-13T05:19:30Z`, `23 Oct 2017,
    5:31 p.m.`), else the year. Language: `<html lang>`, JSON-LD's
    `inLanguage`, `Content-Language`, `og:locale` (an app sending the
    `<head>` alone has no `<html>` tag), tidied (`en_us` is `en-US`).
    Key: `url:` and the normalised address, so a page shared twice is one
    record (a string, not an identifier kind: addresses are still
    scanned for identifiers first, and a page with one is a work).
    Checked on modelled pages and 19 real pages' heads (news in three
    languages, guidance, docs, blogs, a code repository: fixtures in
    `tests/fixtures/pages/web`). One falls short: a title whose site
    nothing names stays whole ("Announcing Rust 1.0 | Rust Blog").
  - *Exported and cited* as a `Work` of type `webpage`
    (`WorkType::WebPage`; `WebPage::to_work(accessed)`, FFI
    `webPageWork(page, accessed)`): the site as container, published as
    issued, the caller's access date (`Work::accessed`: when the page was
    read; core has no clock) and language. BibTeX `@misc` (as plain and
    natbib print it) with `howpublished = {\url{…}}` (`%` and `#`
    escaped) and `note = {Site. Accessed 2026-11-16}`; RIS `TY - ELEC`
    with `T2` the site, `UR`, `Y2` accessed, `LA`; CSL-JSON `webpage`
    with `container-title`, `URL`, `issued`, `accessed`, `language`,
    which Zotero imports as a Web Page (its Website Title, Accessed). The
    styles get `accessed` too: APA cites "Shah, P. (2026, November 13).
    Title. Site. URL", Harvard adds "(Accessed: …)". `verify --html FILE
    --url URL` shows a web page's record instead of verifying it (exit
    0), and with `--format bibtex|ris|csl-json|cite` writes it out, read
    on `--accessed DATE` or today.
- **Scanning** (`id::scan`, `aspero scan`) finds every identifier
  mentioned in a document, with its position and text as written; which
  mentions matter is the caller's call. Shapes that prose imitates (bare
  digits, `W1`, `1706.03762`, a PII) count only when labelled, prefixed
  or URLs. Each mention says whether it was written as a web address
  (`from_url`: a link with its scheme, a host known without one such as
  `doi.org/…` or `nature.com/…`, or a DOI in a host's path). Identifier
  links (doi.org, PubMed, arXiv) count as much as publishers' pages: the
  question is how it was written, not which kind it is.
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
  container, publication details, abstract, URL, cited-by count, access
  date and language (a web page's, see Pages), plus:
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
  detailed ones replacing bare flags. Values sources give differently are
  made the same on every merged record, so a work doesn't change with the
  source that won (#81): open access is the more informative (free with a
  PDF, then a page, then free, then not free with a licence, then not
  free), whatever the version; between records of one version (sharing a
  DOI or PMID, not a preprint and its article), the cited-by count is
  OpenAlex's when one has it (the broadest index, and first for lookups,
  so `verify` and `lookup` agree), else the larger, and `issued` is the
  earliest (online first: OpenAlex's 2020-12-01 over Crossref's 2021-01),
  or of overlapping dates the more precise. Other years stay in
  `other_years`, where year matching finds them. A search's hits are each
  one source's record, as it gives it. `lookup --full` fills notices (Crossref) and open
  access from other providers, keeping each value's `source`.

## Resolve, verify and check

Candidates are gathered cheapest first: lookups of claimed identifiers
(including DOIs, arXiv IDs and labelled ISBNs found in the text) and
Crossref's citation matcher; then **title** searches on keyword providers.
Bare titles rank far better than whole references there, and the title is
located *inside* the reference via each matcher hit's title (or container
title, for untitled book chapters): `shared_title`. When no hit points to
one (a conference paper without a DOI, a book Crossref lacks), the title
the reference's shape suggests is searched instead of its keywords
(`probable_title`: a BibTeX title, a quoted one, else the sentence most
in sentence case that isn't a byline, publisher or locator), at the same
cost; and first, beside the hits', when every hit points only to part of
a title the reference marks as one (BibTeX's "graph convolutional
networks" of "Semi-Supervised Classification with Graph Convolutional
Networks") or to none of it (a publisher's words its hits share: "addison
wesley longman"), two title searches still at most. `verify` stops early only
on a decisive match that the claimed identifiers or two providers vouch
for: a digest or reprint can match a claim exactly while only a later
title search finds the original. Ties go to the earliest published. A
record a provider gives for an identifier it doesn't list (OpenAlex
answers a Zenodo version's DOI with the concept's record) carries that
identifier: the provider resolved it, so it isn't "does not exist".

A reference pasted in a tagged format is read as its fields, a
structured claim, as the bibliography readers read a file's entries
(core `tagged`): a Wikipedia citation template (`{{cite journal |last1=…
|first1=… |title=… |journal=… |year=… |volume=… |pages=… |doi=… |pmid=…}}`,
any `cite …` or `citation`; numbered `lastN`/`firstN`/`authorN` in order,
`vauthors`; a book's `chapter` the title, the book its container; `doi`,
`pmid`, `pmc`, `arxiv`/`eprint`, `isbn`, and identifiers in `url`), an RIS
record (`TY  - JOUR  AU  - …`, its lines run together on one or not; `SN`
an ISBN only when it is one) and an EndNote record (`%0 Journal Article
%A … %T … %J …`, `%R` the DOI). Their tags would otherwise be misread as
free text ("Journal Article" after `%0` as authors). A stdin line starting
with `{{`, or with `{` but not meant as JSON (no key or `}` next), is a
reference, not invalid JSON. A claim object's `text` that is one (an RIS
record on several lines, sent as `{"text": …}`) keeps the fields it gives;
fields the object gives beside it win. A BibTeX entry pasted as text stays text, but
its `eprint` (arXiv unless `archivePrefix` says otherwise) and `pmid` are
identifiers claimed, as DOIs in it already were.

Scoring looks for the record's fields *in* the claim rather than parsing
citation styles. The rules, each from a real failure:

- **Wrong year halves confidence.** Lookalike re-deposits carry the real
  title, authors and abstract under another year. Year scores halve per
  year off; the nearest of any edition's years counts. A year inside a
  link or identifier ("10.1113/jphysiol.1974.sp010694") is none cited.
- **Different authors halve confidence.** Similarly titled surveys and
  books by others otherwise pass.
- **One field can't verify.** "Deep learning" matches many works.
- **Title and year alone stay plausible** (at most 0.8): short generic
  titles ("Health technology assessment") match by coincidence. Authors or
  a shared identifier must corroborate. Without a title, authors, an
  identifier or the place, nothing is plausible: a year or journal alone,
  or a title of a word or two found in free text, matches by chance.
- **Titles compare on content words**, and a dropped subtitle still
  matches, as does a record's title cited without the second sentence it
  runs on into ("… to inform policy and practice. A review and
  discussion paper based on …"); a few shared topic words in free text
  are not a title match.
  A BibTeX entry's title is compared as a claimed title is, not looked
  for anywhere in the entry (its booktitle may be a proceedings' title).
- **A title with words added or replaced is partial, plausible at
  most** (#102): the work's title carried on in the same sentence ("Deep
  learning for medical image analysis": a joining or lowercase word, not
  the journal and year), given a subtitle it lacks or another one, a word
  before it, a word put in place of one of its own that isn't a
  misspelling ("Hydroxychloroquine in …" at the place of "Dexamethasone
  in …"), or words inserted among its own in one sentence ("… Septic Shock
  in Adults and Children (Sepsis-3)"); words dropped still match. A year
  put in place of the title's own is another title of a series ("Cancer
  statistics, 2018" isn't cited by "Cancer statistics, 2020"): a
  mismatch. A subtitle added to a book's whole title is no more words: book
  records often leave theirs out. Words replaced anywhere, several
  together (holdout-2's synonyms: "Likelihood Determinants Associated
  With …" for "Risk Factors …", "A Controlled Study" for "A Randomized
  Trial"), in free text or a title given apart: the two aligned on the
  words they share (give or take a letter; numbers only when equal), each
  stretch where both have words of their own counts a word replaced per
  pair, unless written otherwise (abbreviated, "vs"; shortened, "program";
  run together or split at a line's end, "mort- ality"; a number or Greek
  letter spelt out, "II", "alpha"), at most three words a side, in one run
  in free text (no stop or quote inside; at the title's ends, no
  punctuation). Half the title's words kept at least, half replaced at
  most; under that it's another title, scored as one.
- **A year inside the title is not the year cited** ("Cancer statistics,
  2020 … 2018" cites 2018): neither one inside the work's title where the
  reference gives it, nor, when another is given, one inside the title
  the reference suggests (`probable_title`), whatever work it's compared
  with. With the whole title, authors, volume and first page agreeing, a
  year two or more off is plausible (0.8, the year named), level with a
  partial title at another page, so the one confirming more ranks
  first. A journal's article "A" ("CA: A Cancer Journal for Clinicians")
  is no word an abbreviation must match; a series letter is.
- **Replies, errata and comments are not the original.** Their titles
  repeat the original's; confidence halves unless the claim cites one.
  Nor is the original one cited ("Corrigendum to “…” [Biochim. Biophys.
  Acta 1236 (1995) 197-200]", "Corrections to …"): plausible at most, so
  the notice's record ranks first when found. Not for a retraction cited
  ("retraction of" is a title's words too). A notice is one by its
  title's start ("Correction to", "Author Correction:", "Publisher
  Correction:", "Retraction Note"), a marker anywhere ("Erratum",
  "Reply"), the word after a dash at its end ("… – RETRACTION",
  Cambridge's), or its record's type, a correction's (holdout-4's
  `w:erratum-cited-as-original`: the original's title at the notice's
  place or with its DOI). Not a record typed a retraction: OpenAlex
  types retracted articles so, and IEEE's keep their DOI under "Notice
  of Retraction:", Elsevier's under "RETRACTED:". A claim cites a notice
  when a sentence of it starts as one, its year or quote aside ("(2015)
  'Correction: …'").
- **Online and print years both count**, so either year of an
  online-first paper matches, as do years across merged records.
- **Journal, volume and first page** come from claim fields or the text
  (Vancouver `2017;376(26):2545`, APA `376(26), 2545` or `Journal, 146,
  106274.`, Harvard `376(26), pp. 2545` or `pp.2545`, MLA `77.4 (2005):
  1321-1373`, IEEE's `Art. no. 615` for the page, a page with a section's
  letter, `e1234`, `S12`, `R3`, `L51`, BMJ's `m1123`, and a record's
  garbled dash, `205???210`, Nature `530, 484 (2016)`, also
  after a dotted name (`J. Org. Chem. 101, 6301 (1994)`), Chicago `94,
  no. 2 (2004): 317-321` (the issue labelled, not the volume) or `94, no.
  2 (2004).`, `vol./pp.`, BibTeX fields). Journals match by rule (each abbreviated word
  abbreviates the next significant word, or initials: NEJM, JAMA), not a
  list, so a journal the rule doesn't see only lowers the ranking. A
  different volume or page caps a match at plausible: a citation error,
  and what separates an article from replies printed in the same volume.
- **The wrong journal caps a journal article at plausible** (#80): a
  common error, and a tell of invented references. From free text the
  cited journal is read only where it can be told: the capitalised words
  right before a styled locator (`… placenta. Environ Sci Technol.
  2021;146:106274`, `Environmental Science & Technology, 146, 106274.`),
  back to the sentence before (a title ending in a capitalised word,
  `… Wuhan, China. Lancet.`, isn't a dotted abbreviation). Without a
  styled locator (#102), only after a title that has clearly ended (a stop
  or a closing quote): before the year ending the reference (`… JAMA.
  2020.`), its links, a labelled locator (IEEE `"…," Journal, vol. 8`,
  `no. 10223`, a number after the label: an initial "P." is none), a
  bracketed year (Chicago `"…" eClinicalMedicine (2010).`, `… Druck. ApJ
  (1939).`), or ending it after a quoted title (Harvard `'…',
  Chest.`); a BibTeX entry's `journal`; in prose, between a verb's "in"
  and a bracketed year (`reported in The Lancet (2020)`). After a quoted
  title, the words up to the locator are the journal as written, in
  sentence case too (`"…" Revue historique 35, no. 1`), dotted (IEEE's
  `Mach. Intell.,`) or with a colon (`JAMA: The Journal of …`). A
  publisher (`Springer; 2010:12-30`, `MIT Press; 2016.`), a lowercase
  word, or the work's own title in title case gives none. A run of dotted
  abbreviations that may have begun earlier (`… by Nanoparticles. J. Am.
  Chem. Soc.`, `… Sri Lanka. Cad. Saúde Pública.`) gives each reading
  that starts after a stop in it: the journal one of them, so it matches
  if any does and is clearly another only if every one is; of several
  readings, one word (`Sci.` of `… Creation. Psychol. Sci. 5, 14`) is
  the journal only if it names it by rule (`Lancet.` of `… China.
  Lancet.`), not as one of its words. A short abbreviation after another
  (`Sci.`, not `JAMA.`) is never read alone. In prose, "a paper in"
  (article, study, report) reads the journal up to a bracket (`the 2007
  Bar-Haim paper in Psychol Sci (…)`), and a verb's "in" reads it before
  its volume and pages (`showed in Cancer Epidemiology, 38, 211–214, in
  …`). A volume numbered like a year with its part and issue
  (`2007.0(52)`, a working-paper series') is no year cited. Different means clearly another: not the
  same by rule, neither name (abbreviated, spelling aside) inside the other
  (`Proc Natl Acad Sci U S A` ↔ `Proceedings of the National Academy of
  Sciences`), nor initials of part of it (PNAS), unless it's a sister
  journal (`publication::sister_journal`, round 5; until then a name
  inside the other was "not clearly another", and holdout-4's
  `w:sister-journal` verified 23 of 36): one name is the other's with
  subject words of its own, "The Lancet" for The Lancet Oncology, "JAMA"
  for JAMA Internal Medicine, "Nature" for Nature Medicine, "Diabetes"
  for Diabetes Care, "Circulation" for "Circulation: Heart Failure", or
  "JAMA Intern Med", "NEJM Evid", "JCO Oncol Pract", "JACC Heart Fail"
  for the parent's. The words the names share abbreviate each other or
  are a name's initials (JCO, JACC); a word left over on either side
  tells them apart, unless it's what an acronym stands for ("JAMA: The
  Journal of the American Medical Association", `MMWR Morb Mortal Wkly
  Rep`), a series letter, a structural word, a place, edition or article
  (`Lancet Lond Engl`, "Das Gesundheitswesen"), in brackets, or a
  subtitle describing the journal ("BJOG: An International Journal of
  …", "Physical review. B, Condensed matter"). A true abbreviation
  (`Lancet Infect Dis` for The Lancet Infectious Diseases) is no
  sister. Read from text, a reading may run on from the title's end
  ("… Smoker and in COPD. PLoS One": any part after a stop that is the
  journal counts) or be cut short after a dotted abbreviation ("Theory"
  of `IEEE Trans. Inform. Theory.`: a reading the record's name ends
  with is no sister, so "Energy" for Applied Energy goes untold). A
  journal's letters numbered in its volumes ("The Astrophysical Journal
  Letters, 195, L51") are the journal at its own volume and page. A word
  of three letters abbreviates by its letters, not its consonants ("Gut"
  is no Gastroenterology). Only journal articles,
  and records in a journal whose type isn't given (Europe PMC's
  editorials and letters), or a conference's abstract or a notice in a
  journal's volume (a supplement's), are held to it, structured claims or
  text (series, proceedings and repositories go by many names); a
  BibTeX `journal` is the journal cited even when the title has its
  words. A
  preprint is when the reference cites a journal (not a preprint
  server) unless the claimed identifier is its arXiv ID (computing and
  physics cite a venue beside it): it's another version. So is the
  published version's record, merged with its preprint's (OpenAlex's),
  cited in its journal by the preprint's DOI (a preprint server's prefix:
  bioRxiv's and medRxiv's `10.1101/` with digits after it, Research
  Square, Preprints.org, SSRN, PsyArXiv, OSF, SocArXiv, ChemRxiv,
  TechRxiv, Authorea; holdout-4's `w:preprint-as-journal`): plausible,
  the server named as the journal that differs. So is a journal's article cited as a preprint ("medRxiv
  preprint", "Research Square", no identifier). A record with no journal, volume or page (a
  Zenodo copy, a report; not a preprint, book, chapter, dataset or
  software) can't confirm a journal cited, nor can one without volume or
  page, its journal unconfirmed, confirm a journal, volume and page all
  cited, nor a book a BibTeX `@article`'s journal: plausible at most,
  unless a claimed identifier is its; and of
  equally confident candidates, the one confirming more of journal,
  volume and page ranks first. Candidates are scored with every
  identifier known for the work, whichever record of it is kept, and a
  record giving none of journal, volume and page (not a preprint) takes
  those of another record of the work merged with it; one in the same
  journal and volume, the page it leaves out (Crossref's BMC records
  give no article number, Europe PMC's do). A journal's words inside a
  link (`doi:10.1093/brain/…`), or inside the work's title where the
  reference gives it (`… assays for monitoring autophagy. bioRxiv` names
  no Autophagy), don't name it. A record OpenAlex types a preprint in a
  journal PubMed indexes (a PMID; Genome Research's, under bioRxiv's DOI
  prefix) is a journal's article. A record's page with a part number
  (BMJ's `698.1`) is the page cited as `698`. The
  journal read is in `read`, and a `journal` difference's `cited`.
  Matching volume and page corroborate like authors; with journal and
  volume agreeing, a title missing from a short reference doesn't count
  against it, but a reference that gives one (three or more words in a
  run that are none of the work's authors, journal, numbers or furniture;
  the journal's words only where it stands, their longest run: "The
  contribution of growth hormone in fetal design. Growth Hormone & IGF
  Research." gives a title) keeps its title mismatch: with the work's authors and the place, that
  is plausible (a translated title, or one work's title at another's
  place), never verified. A quoted title, or BibTeX's, is a title given
  however short (`"The essence of entanglement," Phys. Rev. A, vol.
  63`). Journal, volume and first page all agreeing
  (and no title against them) verify on their own (0.9; 0.8 a year off),
  so running
  headers (`Injury 47 (2016) 797–804`, `Nature 521, 436–444 (2015)`) are
  found by an OpenAlex search on volume, first page and year (±1). A
  reference whose every word is the work's authors, journal, numbers or
  furniture gives no title rather than another one, but then needs an
  identifier or the place. A reference that names no one (no "et al", no
  surname with initials) has no author mismatch.
- **A conference paper cited at another conference is plausible**
  (holdout-2's `k`: Pix2Pix, a CVPR paper, "In IEEE International
  Conference on Computer Vision (ICCV)"). The venue cited is a structured
  claim's container, a BibTeX entry's `booktitle` (else `journal`), or a
  reference's text after the work's title (links aside); the record's is
  its proceedings' name, else what an IEEE DOI names
  (`10.1109/cvpr.2017.632`, for a record merged with its preprint's, no
  venue kept). Proceedings' names are free, so the major conferences are
  named (`publication::CONFERENCES`: NeurIPS/NIPS, ICML, ICLR, CVPR, ICCV,
  ECCV, ACL, EMNLP, NAACL, KDD, AAAI, IJCAI, CHI, ICRA, IROS, ISCA, MICRO,
  SIGIR, … by acronym or their names' words in order; acronyms that are
  words, "ACL", "CHI", only in capitals): the venue clearly another when
  both name conferences and none in common. A preprint server's copy
  (arXiv's record, or a record with an arXiv ID) cited in a conference's
  proceedings ("In Proceedings of …", a conference named) can't confirm
  it, as a copy can't confirm a journal: plausible, the venue named,
  unless the claimed identifier is its or its DOI names the conference.
  Real NeurIPS citations whose only record is arXiv's are plausible
  too: the venue unconfirmed.
  Any other meeting is told by what it's about (holdout-3's `k`: an HRI
  paper "In The 26th International ACM SIGACCESS Conference on Computers
  and Accessibility (ASSETS '15)"; `publication::proceedings_agree`).
  Each name is reduced to its subject words, its furniture set aside
  (Proceedings of the, Companion, ordinals and years, Annual,
  International, World, Joint, ACM/IEEE and other publishers, SIG…,
  Conference/Symposium/Workshop on, pages, editors), and its acronyms
  (two or more capitals: "HRI", "RecSys"; not a publisher's or a
  numeral): a shared acronym is the same meeting; at least half the
  record's subject words in the venue cited (an abbreviation for its
  word: "Robot." for "Robotics") is the same; a third or fewer is
  another, plausible with the venue named; an acronym alone against a
  name with another acronym is another, against a name without one
  ("ASSETS" for "… Computers and Accessibility") can't be told. Only a
  venue the claim gives (a structured container, BibTeX's, or text
  after the title) as a meeting's ("In …", "Proceedings of …"), against
  a record's proceedings ("… Conference on …", "Companion of …", or
  "Proceedings of …" unless the record is a journal's article: the
  Royal Society's journals go by abbreviations this can't tell). A
  journal's article cited "In Proceedings of …" a meeting is another
  venue too. A record of the same version (a DOI shared) that gives its
  pages but no venue (OpenAlex's of ACM papers) takes the venue of the
  other record merged with it, where their numbers agree, so a venue
  cited is checked whichever record is kept.
  A meeting of the same society is told apart too (round 5, holdout-4's
  `w:conference-same-society`, 19 of 30 verified): an acronym right
  before a meeting word names who holds the meeting, not the meeting
  ("International AAAI Conference on Web and Social Media (ICWSM-24)"
  doesn't name AAAI's conference; "ICML Workshop on …" isn't ICML), in
  the named conferences and in subject comparison alike; and a venue
  cited by acronyms of its own, none the record's name can make (its
  letters in order, each word giving its initial and any of its later
  letters: "MWSCAS" of "Midwest Symposium on Circuits and Systems",
  "AIES" of "Conference on AI, Ethics, and Society"), is another
  meeting however many subject words they share ("ICECS" for the Midwest
  Symposium on Circuits and Systems, "ICMR" for "… Multimedia in Asia").
  Series' and publishers' acronyms (PMLR, CEUR, LNAI) are a venue's
  none. An acronym that fits is no proof: FSE cited as ICSE goes
  untold. A record in no venue, volume or page (a repository's copy
  typed an article, a talk's DataCite record typed a preprint; not a
  conference paper's record, its proceedings merely unnamed) can't
  confirm a meeting cited, as arXiv's can't: plausible, unless the
  claimed identifier is its.
- **A journal's article cited as a chapter of an edited book is
  plausible** (holdout-4's `w:article-as-chapter`, the reverse of the
  next rule): a reference citing its work "In" (or Harvard's "in" after
  a comma or quote) and then its editors ("editors", "(eds)", "(Eds.)",
  "ed." but not an edition's "2nd ed.", "edited by", "Hrsg.") holds a
  journal's article to another container, the journal named. Not when
  the book named is the record's container (a series' volume: "In: …,
  editor. Methods in Enzymology") or the venue is a meeting's
  proceedings.
- **A book's chapter cited as a journal's article is plausible**
  (holdout-3's `m`): a chapter, in no volume, cited with a volume and an
  issue (`Biogeochemical Cycles and Climate. 2019;23(5):159-175`,
  `Psychotherapy Research, 54(12), 341–356`), or with a structured
  journal (not its book's title) and volume. Its volume is the
  difference named, and a journal other than its book the venue. A book
  series' volume has no issue (`Lecture Notes in Computer Science, vol.
  9351`), and a chapter cited "In: …" is none. Nor does the book itself
  verify for it, its title where the journal stands with a volume and
  issue.
- **An identified work cited with other authors is plausible**: the
  claimed identifier, the whole title and the place name the work, and
  authors that differ are the citation's error (or a record's garbled
  byline: holdout-3 #138's one author "Ruff, Paul Jurica, Ronald M").
- **First author alone is an author match** ("Vaswani et al."); an
  unmentioned sole author is no match. But a name the claim gives that
  the work's byline lacks (#102: an invented co-author beside a real
  first author) makes the authors partial and the match plausible at
  most: in free text, a capitalised name word in the byline (the text
  before the title, or BibTeX's `author`) that is none of the record's
  names give or take a letter, nor a given name beside its family name;
  in a structured claim, a family name not on the record. A family name
  with initials of its own after a comma (`Harrowgate, O., & Palmer, H.`)
  is no given name of the next. Not checked for
  initials, an organisation's byline, a record cut short (100 authors)
  or naming an organisation with fewer than three people, or books.
  A byline naming people, none of them the record's, as any style writes
  them (IEEE `F. Eskdale-Mora, "…"`, MLA `Nadler, Steven. "…"`, BibTeX's
  `author`), has different authors, not unknown ones; two words of a
  title in capitals (`WOMEN ON ORAL`) are no name and initials: a family
  name in capitals takes initials set off by a stop or comma. A name
  with two or three capitals run together after it (`Thalubrecht HL`) is
  a family name, not a given name of the next. A record's name with its
  family name in capitals (`Seok-Bae LEE`, `A. H. LINSSEN`) or inverted
  (`Lubrano, Emanuele`, DataCite's) is a person's, so its byline is
  complete; so is a name of two to five words in a script without
  capitals (Arabic: `امحمد عثمان فيدان`), which can't be told a person's
  otherwise. A claimed double-barrelled name one part of which is an
  author's, the given initial agreeing (`Hirschi-Santelmo, Sandrine` for
  Sandrine Hirschi), is that author; organisations among a structured
  claim's authors ("Committee on Infectious Diseases") aren't compared.
  A label before the byline (a trial's or guideline's, as models write
  them: "EMPEROR-Reduced (empagliflozin, n=3,730): Packer M, …"; a few
  words before a colon, no comma among them outside brackets, then a
  name with its initials) is none of its names; BibTeX's accents
  (`Veli{\v{c}}kovi{\'c}`, `Tr{\k{e}}bacz`) are read as the letters they
  mark, in its fields and its text alike.
- **Another edition or version is plausible**: a book (or one reprinted
  whole as a chapter, cited with its publisher) with another year, and a
  dataset or software matching the claimed identifier and title, cited
  with another year than its record's latest version.
- **"RETRACTED:" title markers are ignored** when comparing titles.

**Sites** (#98). A claim's `site` (the page it was read from:
`claimFromPage`, `verify --url`, `--site`, JSON `site`) is evidence of the
publisher, never a claimed field: not compared as the journal, not a
difference. Only for a claim without identifiers (with them, the site says
nothing they don't), and only for a site in `site::PUBLISHERS`: publishers'
sites by registrable domain (the last two labels, three under a country's
`co`, `ac`, `com`, … : `pubs.acs.org` is `acs.org`, `notnature.com` isn't
`nature.com`; no Public Suffix List) and the DOI prefixes their works are
under (`nature.com`: `10.1038` and Palgrave's `10.1057`; `wiley.com`:
`10.1002` and `10.1111`; each checked against Crossref's `/prefixes`,
2026-11-15). Then:

- *Recall*: Crossref is asked for the title among the publisher's works,
  `query.title` with `filter=prefix:…`, one search per prefix (at most
  two), beside the title searches and whatever they find, nothing
  included: the open searches may rank the publisher's own work low, or
  miss it. Free, so the budget is as it was.
- *Ranking*: a tie-break, not a score. After ranking, the first gives way
  to the publisher's best-fitting candidate (a DOI under its prefixes)
  when that one matches about as well: confidence within 0.05 of the
  first's, its title agreeing as well (match, partial), and of the same
  verdict, plausible at least. 0.05 is less than any whole field's
  disagreement costs (a year one off, 0.125 with title, authors and year;
  the caps, 0.1; another journal, 0.06 and more), so only part of a
  title's or byline's words separate the two: the claim can't tell them
  apart, the site can. Of the publisher's, the most confident, then the
  best title, then the best whole title (Crossref keeps subtitles apart,
  so JAMA's "Treatment of Acute Ischemic Stroke" of 1986, "Open Trial
  With …", ties with the 2026 review on its main title; the review's
  whole title is the one cited). Confidences stay their own, so the
  verdict is the chosen work's own assessment, the one the first would
  have had: a title-only claim the site settles stays plausible. With no
  candidate from the publisher, nothing changes. `chosen_by_site` says
  when the site decided (another work would be first without it).

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

**Accuracy**: what the verdicts promise, the red team that measures them
(`bench/redteam/`, its verdict policy and corpus), the acceptance criteria
and release gate, and the results are in
[docs/accuracy.md](docs/accuracy.md). Wrong verdicts are reported with the
wrong-verdict issue form (`verify --report` fills it in) and become
red-team cases (`scripts/issue-to-case.py`).

**Benchmark**: `bench/citations.jsonl` (known-answer citations, optionally
requiring a specific match or retraction flag), run live by
`cargo run -p aspero-engine --example verify_bench` through an HTTP tape
(below): the first run costs ~$0.035, re-runs are free. Add a case for
every new failure mode found in the wild.

**HTTP tape** (a developer tool; not in `--help`): `ASPERO_HTTP_TAPE=<file>`
puts a `TapeClient` (engine `transport`) in front of every provider's
client, over a SQLite file of its own (store `SqliteTape`: `tape(key,
method, url, body, status, headers, response, recorded_at)`), so benchmark
and red-team runs pay for each distinct request once, ever, and are
reproducible. A request on the tape is replayed; one that isn't is sent,
and recorded if its answer is data: a 2xx, or a 404 (a meaningful "not
found") unless it says a budget is spent (OpenAlex's daily allowance at
zero with no prepaid credit left: a 404 paid from credit is data); never a 429, 5xx, other error or
transport failure, nor OpenAlex's `/rate-limit` (live state).
Identifier lookups are batched (OpenAlex's DOI filter, up to 100), so a
change that gives claims identifiers they lacked (a template read, an
`eprint`) changes every batch after it: replay those claims apart for the
rest to stay on the tape.
`ASPERO_HTTP_TAPE_MODE=replay-only` sends nothing: a miss fails as
`Offline`, logged "not on tape" (fully offline runs). The key is the
method, the URL normalised (query parameters sorted; `api_key`, `key`,
`mailto`, `email`, tokens dropped), the request headers that can change
the answer (`accept`; never `authorization`, `user-agent` or other
credentials) and the body. The tape stores the normalised URL, and
redacts what a response echoes of a key or contact address (the user
agent's included), so it never holds either and can be shared. Replays
skip pacing (the tape wraps the paced client), arrive as recorded
(`received_at` is `recorded_at`), and cost nothing: their `x-ratelimit-*`
headers are dropped and a recorded cost reads 0, so they count towards no
spend (usage, MCP sessions) and move no daily cap; misses sent count as
usual. A daily cap is checked before the tape is asked, so on a day past
it, record mode stops paid searches even where they're taped;
replay-only knows no budget and isn't capped. The harnesses default to a
tape: `verify_bench` to `target/bench-tape.sqlite` (`ASPERO_HTTP_TAPE`
overrides; `off` for the old `target/bench-cache`), `scripts/redteam.py`
to `target/redteam-tape.sqlite` (`--tape`, `--no-tape`, `--replay-only`).
With a tape, each run starts from an empty answer cache (the tape is
below it): the answer cache would add nothing but a dependence on its
own age, while an empty one keeps the requests a first run makes
(batched prefetches, then claims), so runs differ only by code.
`just tape-stats` (`scripts/tape.py`) shows entries, statuses, size and
recorded cost per host; `scripts/tape.py export` writes a self-contained
copy (or copy the file while nothing uses it) and `import` merges one
tape into another. The tape is durable: its table is never dropped, and
a change to key normalisation only turns old entries into misses.

Under a tape, identifiers are looked up one per request
(`EngineBuilder::taped`, which `StandardConfig::http_tape` sets): a
batch's key holds every ID in it (OpenAlex's `filter=doi:a|b|…`,
Crossref's `doi:`/`alternative-id:` filters, Europe PMC's `OR`ed
clauses), so a code change moving which claims carry identifiers
changed every later batch and missed the tape, though each ID's answer
was on it. One request per ID makes each answer, a "not found" too (a
404, or an empty list), an entry of its own, replayed however claims
are batched. A batch an older tape holds whole is still replayed: each
is first asked replay-only (transport `replaying_only`: a miss fails
unsent), and asked an ID at a time only if it isn't all there. The
answers are the same, only the requests differ: the batch paths already
answer as a lone ID's lookup does (OpenAlex asks a key its filter
missed alone). Recording costs no more: OpenAlex's single-work lookups
are free where a filter page is $0.0001, the other sources are free;
it sends more requests (a 100-ID batch is 100, paced), once per ID,
ever. The daily cap isn't asked about the size of batches under a tape
(none are sent), and a graph's spend still counts its lookups as
batches, so its budget stops the same with and without the tape.
Splitting batch responses into per-ID entries at the tape was the
alternative: it would keep batches on the wire, but needs each
source's response format and ID matching in the transport (OpenAlex
finds works by DOIs their records no longer carry), for no saving.

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
  never the work it answers), first author and years ±1; or one work
  through another (a record carrying one's DOI and the other's arXiv
  ID: every pair is compared). Merged ids are kept; counts are over
  merged seeds ("cited by k of n").
- **Retracted references:** the union of the seeds' references checked
  with `openalex:A|B|…,is_retracted:true`, 100 per request ($0.0001; the
  same as a lookup batch), plus, without calls, notices already cached:
  cached records' own flags and the registry's (Crossref) notices for
  their DOIs.
- **Odd one out:** a seed citing no other seed, cited by none, sharing no
  references, is `no_links`; with unknown references it's
  `no_reference_data`, unless another seed cites it (then it isn't odd);
  so is one a seed of unknown references may cite (one not dated more
  than a year before it): whether it's linked can't be told. Free. Nothing is odd in a set of fewer than two works.
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
    A work without a `cited_by_count` gets a page of its own, and counts
    as many citing works as it read (`capped` if that page wasn't its
    last): before, it was taken for uncited and never read.
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
  - *Score:* co-citation strength and the coupling count, each as a
    share of the highest found, weighted 0.8 co-citation and 0.2
    coupling (all to one when no work has the other). Weighted evenly
    (tried on two sets, 2026-11), coupling's long-reference-list
    favourites (broad reviews, theses, AI in food science) outranked the
    field's core works; co-citation-led, a set of eleven sepsis trials
    gets its peers (VASST, SAFE, CORTICUS, TRICC, VISEP) and the
    guidelines and definitions.
  - *Co-citation strength* weighs each citing work by 1/√(its
    references); `co_cited_with` still counts them. A work listing
    hundreds co-cites everything it lists: OpenAlex has 26 chapter
    records of the Oxford Handbook of Clinical Specialties (2020), each
    with the whole book's ~970 references, so a set of ED analgesia
    trials (one cited by the book) got CRASH-2, an airway guideline,
    parvovirus B19 and lithium, co-cited 26–27 times, in its top 20.
    Tried live (2026-11-14) on that set and the sepsis trials: 1/n
    (association strength) favoured citing works listing a handful of
    references (partial data); 1/ln(e + n) left the chapters' picks in the
    top 20; requiring two linking seeds didn't help (the book cites two
    of the set). 1/√n dropped them all, keeping the ED set's own field
    (PEMI, oligoanalgesia, Bijur 2005, the hydromorphone titration
    trials) and the sepsis set's top 20 as it was, a place or two apart.
  - The best 100 (per 100 of `limit`) are looked up in one batch, so works
    of the set (any shared identifier with any seed, resolved or not, or
    `same_work` with one), retracted works, drop, and versions merge
    (counts joined), as probably missing does; then the best `limit`,
    ties by id. Each says `co_cited_with`, `shared_references` and the
    `seeds` linking it.
  - Citing works dated more than a year before a work of the set they cite
    are set aside: OpenAlex gave a 2001 paper on mustard 300 references,
    2014 sepsis trials among them.
  - `incomplete` lists every stop, each naming the `stage` that stopped,
    with `done` of `total` counting that stage's things: `citing`, citing
    works read of those the set's `cited_by_count`s promise; `coupling`,
    pages read of coupling's sample; `lookup`, works answered of those
    asked about. Failures (`provider_failed`, with the `provider`) come
    first, then `budget` (the caller's daily cap on OpenAlex too: it is
    theirs, not a failing provider), each in stage order, then `capped`
    (at `citing`) when the plan sampled a much cited work or a work of
    unknown count wasn't read to the end, whatever else stopped the
    reading; each stop once. A retraction check says a failure, then the
    budget, likewise; so does a lookup of probably missing works. Before,
    one stop was said per section (the rest only logged), so a budget
    hid a sample and a failed coupling page; and before that, a failed
    lookup or coupling page said `provider_failed` with every citing work
    read (done = total), and nothing said why.
  - *Without a key* (seen 2026-11-14) OpenAlex refuses a cursor past the
    first page: 401 "Paging past the first page with cursor= needs a free
    API key". It had read as a rejected key, which stops the engine
    asking OpenAlex anything for its life, so coupling's second page
    failed and then the lookup, and related works came back unlooked-up.
    Without a key a 401 or 403 is `UnsupportedQuery` now, and
    `Capabilities::max_pages` (1 for OpenAlex without a key) caps each
    filter to its first page: each seed's 200 most cited citing works.
  - Live (2026-11): eleven sepsis trials, $0.0060 (60 pages, as estimated),
    16 s, 10,647 of 15,357 citing works; eight deep-learning works with
    LeCun 2015 (90,000 citing works), $0.0046, 13 s, 8,000 of 149,057.
    Cached, a second run is free and takes a second.
- **Spend.** Analyses run cheapest first (odd one out, probably missing,
  retracted references, related works). With a budget (US dollars), each
  paid request is refused when it could take the spend past it; what was
  stopped says so: `incomplete` lists each stop, `{kind:
  budget|provider_failed|capped, done, total}`, and is empty when the
  analysis (or the seeds' resolving, at the top level) ran to the end.
  `estimate_only` resolves seeds from the cache and free
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
  (and `ASPERO_CACHE_*`); the rest stay as they are. Three tables are
  accounting rather than answers, so rebuilds, `clear` and eviction keep
  them: the `pacing` slots, `mcp_spend`, each MCP key's engine spend
  per UTC day (see Agents (MCP)), and `prepaid_day`, each provider's
  prepaid balance over a budget day (see Resilience, the credit cap).
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
`openalex_daily_cap_usd`, CLI `--openalex-cap-usd` /
`ASPERO_OPENALEX_CAP_USD`)
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

OpenAlex also sells **prepaid credit**, spent only once the day's free
allowance is used up (`daily_used_usd` tops out at the allowance: it
doesn't count credit). A second cap holds the credit spent in a day
(#110): `EngineBuilder::daily_credit_cap`, `StandardConfig`/FFI
`openalex_daily_credit_cap_usd`, CLI `--openalex-credit-cap-usd` /
`ASPERO_OPENALEX_CREDIT_CAP_USD`. Unset, it's $0.25 a day
(`DEFAULT_OPENALEX_CREDIT_CAP_USD`; FFI `nil`); 0 never spends credit; more
allows more, and there's no unlimited setting. Before a paid call, with
"used" and "spent" always counting everything using the key:

1. free allowance used today < the allowance cap: go, on the allowance;
2. allowance cap ≤ used < the allowance: stop (the rest is held back for
   other tools);
3. the allowance all gone (by anything) and credit spent today < the
   credit cap: go, on credit, up to the cap;
4. the allowance gone and credit spent today ≥ the credit cap: stop until
   the reset.

So with an allowance cap below the allowance, a caller never takes the key
into credit itself; only other tools can. "All gone" is nothing left to a
millionth. A stop is `BudgetCapped` with `which` (`CapKind::Allowance` or
`Credit`; FFI `BudgetCap.capKind`, `cap_usd`/`spent_usd` the cap reached
and what's spent against it), worded "stopped at today's credit cap of
$1.00 ($1.02 of credit spent today)". Two overshoots are known: a call is
checked before it's sent and its cost known only after, so the last call
(or calls in flight together) can pass a cap by their own cost; and other
tools' spending is seen only in the next response, so one call can land
on credit even with a credit cap of 0. An engine built without a credit
cap (`EngineBuilder` alone, not `standard`) holds a used-up allowance to
the allowance cap only, as before #110: the day's use tops out at the
allowance, so a cap at or under it spends no credit and one above it (or
none) spends credit freely. With either cap set and the day's use unknown,
the first paid call is preceded by the free refresh above, so with the
default credit cap a keyed process asks `/rate-limit` once before its
first paid call.

**Credit spent today** (`Budget::prepaid_spent_today`, FFI
`prepaidSpentTodayUsd`, `aspero budget` `prepaid_spent_today_usd`) isn't
reported by OpenAlex, so it's told from the balance every response and
refresh carries: the first balance seen in the budget day (the UTC day
ending at `resets_at`, an hour back so a reset read a moment past midnight
names the day before), raised by any top-up since, less the balance now.
Headers' balances are not to be trusted one by one: OpenAlex answers from
many servers, some minutes behind, so requests made together say balances
up to $0.30 apart, the stale ones higher (seen 2026-11-16 on the red
team's tape: 9.84 and 9.66 interleaved within a second). Taking each rise
of a cent as a top-up ran one day's count to $5.33 for $0.34 spent. So a
header's balance only ever lowers the balance now (the lowest seen is the
nearest the truth), and never raises the start. Only `/rate-limit`, which
also says the credit bought (`prepaid_balance_usd`, `Usage.prepaid_bought`),
sets the balance outright, either way, and only a rise in the credit
bought is a top-up, raising the start by as much. A stop at the credit
cap is checked first with a free `/rate-limit` (once a minute at most;
with a key), so a stale low header can't stop calls for long. The day is
kept in the cache file's `prepaid_day (provider, day, start_usd, last_usd,
seen_at, bought_usd)` (`PrepaidDays`; one write transaction a sighting; a
week kept; a table of the first shape, without `bought_usd`, is dropped),
so every process sharing it (CLI, MCP, the app)
and every restart tells the same figure, counting everything using the
key from the day's first sighting by any of them; without a file, each
engine keeps its own. Like the allowance's use it counts everything
using the key, but a day first seen part-way through misses what was
spent before, as does a top-up seen only in headers (until `/rate-limit`
says it, it isn't one, and spend down to the old balance goes uncounted):
undercounts, accepted. A day's first sighting from a stale server starts
it high, an overcount of the staleness. `Budget::prepaid_expires_at` (FFI
`prepaidExpiresAt`) is `/rate-limit`'s `prepaid_expires_at`, a JavaScript
`Date` printed as a string ("Sun Feb 14 2027 12:08:05 GMT+0000
(Coordinated Universal Time)", ISO also read); headers don't carry it, so
it's kept from the last refresh.

**Prices** (`Budget::prices`, a `Prices` of OpenAlex's kinds of call:
`singleton`, `list`, `search`, `search_rerank`, `rerank_extra`, `content`,
`semantic`, `text`; `Budget::search_price`) are `/rate-limit`'s
`endpoint_costs_usd` (`Usage.prices`), so only with a key; headers don't
carry them, so they're kept from the last refresh. A fixed struct, not a
map, so `Usage` and `Budget` stay `Copy`: a kind OpenAlex adds is left out
until named. FFI `Budget.searchPriceUsd` and `prices: [BudgetPrice
{endpoint, usd}]`; `aspero budget` JSON `search_price_usd`, `prices_usd`.
**What's spendable today** (`Engine::spendable_today`; `aspero budget`
`spendable_today_usd`) follows `cap::reached`: the allowance left, held to
the allowance cap (`cap − used`); then, if paid calls would use the
allowance up (its cap at or above the allowance, or it's used up already),
the prepaid balance, held to the credit cap less the credit spent today;
without a credit cap, the whole balance only where the allowance cap is
above the allowance or unset. Divided by the search price and rounded
down, it's `aspero budget`'s "searches $0.001 each, about 850 left today"
(`searches_left_today`): an estimate, since it counts everything using the
key and lookups spend from the same budget.

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
  `prepaid_remaining`), which JSON and the FFI name in USD (`cost_usd`,
  `remaining_usd`, `limit_usd`, `prepaid_remaining_usd`). `GET /rate-limit` says the same as JSON
  (`daily_budget_usd`, `daily_remaining_usd`, `prepaid_remaining_usd`,
  `resets_at`) for free (`daily_used_usd` unchanged across calls, no cost
  header; checked 2026-11-14), but only to a key (401 "Authentication
  required" without; Bearer works): `Engine::refresh_budgets` asks it,
  and asks nothing without a key. `aspero budget` without a key (the CLI
  alone; the FFI's `refreshBudgets` still asks nothing) looks one work up
  alone, past the cache (free; limit $0.10, checked 2026-11-14), and
  reads the budget from its headers. Its body echoes the key's first and
  last three characters, masked in the fixture.
  A failing response is a spent budget (`QuotaExhausted`, which stops
  OpenAlex until the reset) when its body says `not_enough_credits`, or
  `-remaining-usd` is 0 with no `-prepaid-remaining-usd` left: credit is
  spent once the allowance is used up, so with credit left a failure is
  what its status says (a 429 rate limit, a server fault).
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
  given an equal share of the 100 values a filter takes (a loose name's
  best matches are what it means). A journal left out that names a full
  page of sources (more may follow, and would leak through) fails
  (`UnsupportedQuery`, asking for the exact name as a phrase) rather than
  being cut short; only the search can tell, not `search_supports`. The
  lookups aren't cached: each page asks again.
- **Crossref.** `mailto` puts requests in the polite pool (10 req/s single,
  3 req/s lists, 3 concurrent). **Cursor paging drops relevance order**
  unless `sort=score&order=desc` is sent. Lookalike re-deposits exist (2025
  copies of "Attention Is All You Need" with the real authors). Field
  queries: `query.author`, `query.title` (titles only, unlike
  `query.bibliographic`), `query.container-title`. They rank rather
  than require, scored together: in a tree, terms of one field join its
  parameter (as one term of all their words would), and a journal term
  goes alone among terms (filters beside it are fine), since beside
  keywords or a title works from any journal outrank it; such a tree is
  refused, saying so. It doesn't
  index DataCite DOIs (arXiv). Its "CSL" uses Crossref type names.
  `filter=prefix:10.1001` keeps to one registrant's DOIs, with
  `query.title` ranking them: JAMA's review first of 33,845 (checked
  2026-11-15). Subtitles are a separate `subtitle` field, which aspero
  doesn't join to the title. Top
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
- **Crossref and PIIs.** Elsevier deposits each work's PII as an
  `alternative-id`, compact and upper case (`filter=alternative-id:
  S2772940026000032`; repeated filters OR). Other publishers put other
  numbers there (JAMA none), so only PII-shaped values are read, and only
  when looking a PII up. One odd deposit holds `S0000000000000000`.
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

A **`Session`** is one connection: its `Caller` (empty over stdio; over
HTTP, the key) and a spend cap in USD (`--budget`, default $0.25). Tools that may spend are refused once the session has spent it,
checked before each call; a call's cost is known only after it, so the
last one can overshoot by its own cost. Calls at once can't overshoot
more: each holds an estimate of its cost while in flight (at OpenAlex's
prices: a search $0.001, one more per journal term; a citation two
searches and its lookups' list calls), and a call whose estimate doesn't
fit in what's left beside those held waits for the calls in flight,
running once it fits or alone. A call costing more than its estimate
(prices changed) overshoots by the difference. Spend is the engine's reported
usage (OpenAlex's `cost_usd`), as the CLI totals it, charged whether or not
the call succeeds (a document not identified was still searched for);
every result carries the
call's `usage` and the session's `spent_usd`, `budget_usd` and `left_usd`.
`search_supports` is offline and never refused.

### Streamable HTTP and host tools (P1, P2)

For apps that embed aspero: `aspero_mcp::http` (the `http` feature; the
CLI builds without it and serves stdio only) and, over it, the FFI's
`McpServer` and `McpHost` (the FFI's `mcp` feature, which
`scripts/xcframework.sh` enables).

- **Interface.** `McpServer.start(aspero, config, host)` (async; throws
  `McpError`: `portInUse { port }`, `toolNameClash { name }`,
  `setup { message }`) serves through the app's own `Aspero` handle, so
  its OpenAlex key, cap, disabled sources and cache apply and no second
  engine is built. `McpServerConfig`: `port` (23130), `keys`,
  `sessionBudgetUsd` ($0.25), `engineTools` (true; false serves only the
  app's tools). `McpKey`: `id`, `name`, `scopes`, `sha256` (hex),
  `dailyBudgetUsd` ($1). Then `setKeys` (at once), `hostToolsChanged`
  (re-reads the host's tools; throws on a clash, keeping the old ones),
  `port`, `status` (running, port, open sessions, the last request and
  its key's name, each key's last use and spend today) and `stop` (ends the sessions and
  frees the port before returning). The app implements `McpHost`:
  `tools()` (each a `HostTool`: name, title, description, input and
  output JSON Schemas, scope), `async call(name, arguments, caller)`
  (arguments a JSON object; `McpCaller` is the key's id, name and scopes;
  answers `HostToolResult { structured, summary }` or throws
  `HostToolError`: invalid arguments, not found, refused, failed) and
  `activity(McpActivity)`.
- **Transport.** `rmcp`'s Streamable HTTP service (a tower service; no
  axum) at `/mcp`, under a small hyper (HTTP/1.1) server of ours, bound to
  `127.0.0.1` only. The port is the app's choice and never moves: taken,
  `start` throws. Sessions are the protocol's (`initialize`, then
  `Mcp-Session-Id`), each with its own handler and budget; only the
  revisions with `initialize` are offered, and a call outside a session
  is refused, so budgets can't be dodged by going sessionless. A key has
  at most 16 sessions open (closed, or ended idle after rmcp's five
  minutes); another `initialize` is refused, 429 with a line saying so.
  Stopping
  first, at once, refuses further requests (503), cancels the sessions'
  streams and closes the listener; then closes every session and waits up
  to two seconds for requests in flight. A stop cut short (the app's task
  cancelled) finishes when called again; `status` says running until
  then.
- **Auth, on every request, in order.** The path; `Host` must be
  `127.0.0.1:<port>` or `localhost:<port>` and any `Origin` is refused
  (both 403, before auth); `Authorization: Bearer <token>`: the token's
  SHA-256 is compared with every key's in constant time (`subtle`'s
  `ct_eq`, all keys whatever matches), 401 with no detail otherwise; a
  session id must be one this key (same id and hash) opened, else 404.
  `rmcp`'s own Host and Origin checks are configured the same, behind
  ours. `setKeys` swaps the list atomically: a session whose key is gone
  or re-hashed is closed, and its next request fails auth.
- **Scopes.** `sources` for the engine tools; `readHistory` and
  `writeHistory` for the app's, each declaring one. `tools/list` shows a
  key only what it may call; a call without the scope is a tool error
  ("this key can't use … : it lacks the … scope") and the app isn't
  asked. Host tools are annotated read-only unless `writeHistory`; none is
  destructive, and there's no delete scope.
- **Tool-list changes.** `tools/list`, `tools/call` and the scope check
  read the request's key (as `setKeys` last left it) and the host's
  tools (as `hostToolsChanged` last read them) at each request; nothing
  is fixed when a session opens. The server declares
  `tools.listChanged`, and tells sessions when their list may have
  changed: a key kept by `setKeys` (same id and hash) with other scopes
  (as a set) gets `notifications/tools/list_changed` on its open
  sessions; `hostToolsChanged` (unless it throws) sends it to every open
  session. Each session's `rmcp` peer is kept from its `initialize`,
  with its key's id and hash, and dropped once its transport has closed;
  each notification is sent from a task of its own on the server's
  runtime (`Peer::notify_tool_list_changed`, given up after 10 s, errors
  ignored), so neither call waits on a client. `rmcp` puts it on the
  session's GET stream, or keeps it (up to 16 messages, the newest) until
  the client opens one. The engine tools' listing is built once, over
  either transport, since `Tools` never changes; over stdio the list
  never changes at all.
- **Host tools.** Names must differ from the engine tools' (even with
  `engineTools` off) and each other's, or `start` / `hostToolsChanged`
  throws `toolNameClash`; schemas must be JSON objects. A call's
  arguments must hold the input schema's `required` fields (checked
  here; the rest of the schema is the app's to check), then the app has
  30 s; a `HostToolError` or a timeout is a tool error (`isError`) with
  its message (a timeout's says the outcome is unknown: the app may still
  finish, so a retry could write twice), and `structured` must be a JSON object: the result's
  `structuredContent`, with `summary` as its text. Host tools cost $0.
- **Budgets.** Per session, as over stdio. Per key per UTC day: engine
  spend only, refused (a tool error; free tools still work) once a paid
  call would start at or past `dailyBudgetUsd`, overshooting as sessions
  do, estimates held across all the key's sessions. At most four paid
  calls per key run at once (more wait), which bounds what calls costing
  more than estimated can add. Kept in the cache file (`mcp_spend (key_id, day, usd)`, beside the
  versioned schema like the pacing slots: rebuilds, `clear` and eviction
  keep it; days over a month old are pruned), so restarts don't reset it;
  in memory without a cache. A spend that can't be read refuses the
  key's paid calls; one that can't be written is kept in memory, counted,
  and written with the key's next spend or check. `status` says each
  key's spend today (`McpKeyStatus.spentTodayUsd`): the same ledger
  figure plus the unrecorded, as the budget is checked against, read
  without writing (a read of the cache file per key; a figure it can't
  read counts as $0, logged), so "$0.12 of its $0.50 today" agrees with
  the refusal. The day is the UTC day (`McpStatus.spendDay`,
  `YYYY-MM-DD`), ending at `spendResetsAt` (midnight UTC), when every
  key's spend starts again from nothing.
- **Activity.** After every tool call (engine or app, success, error or
  refusal), the app's `activity` gets the key's name, the tool, a few
  words ("searched ‘lactate clearance’", "looked up 3 works", "verified 2
  citations", "identified a document"; an app tool's summary's first
  line; "error: …"), whether it failed, and its cost, on a thread of its
  own, in order, never holding up a request. Up to 256 wait for the app;
  while it's that far behind, more are dropped (they're informational).
  The same as data (#100), built with the summary from the same outcome,
  so the app can word it itself and tell the agent's mistakes from
  trouble elsewhere: `error`, the kind (`McpErrorKind`, mirroring
  `ToolErrorKind` case for case, with `other` last for kinds added later;
  an app tool's `HostToolError` keeps its kind, a missing required field
  is `invalidArguments`, a lacking scope `forbidden`, no answer in time
  `timeout`, an answer not a JSON object `failed`); `errorMessage`, the
  whole message without "error: "; `query`, what an engine tool was
  asked, as given (keywords, or a query tree in a line: `title:sepsis AND
  (journal:"Lancet" OR journal:"BMJ") AND years:2018–`; identifiers or
  citations joined with "; ", fields as `ClaimInput::describe` writes
  them; `identify`'s title hint, title or first line), at most 500
  characters, even when refused; and `count`, how many came back (a
  search page's works, works looked up, citations verified, one work
  identified). An app tool's `query` and `count` are empty: the app words
  its own tools. All four are additive, defaulting to `nil` in Swift.

**Threat model.** The server is for agents on the same machine, acting
for the user. Loopback only: nothing off the machine can connect. Every
request needs a key the app minted and can revoke; aspero holds only
SHA-256 hashes (the app keeps tokens in the Keychain), compared in
constant time. Browsers are the remaining local attacker: a page can
send requests to loopback, but it sends an `Origin` (refused) and, with
DNS rebinding, its own host name in `Host` (refused); it can't read a
token. Other local processes running as the user could read the app's
data anyway, so a key bounds what an agent may do (scopes, budgets), not
who the user is. No tool deletes anything; writes go only through the
app's `writeHistory` tools, which the app implements and can refuse.
Results are third-party data, and say so.

Next (#58, P3): progress notifications and resources.

## Apps (FFI)

`aspero-ffi` (UniFFI) gives apps one `Aspero` handle: `lookup`, `verify`,
`verify_each` (a callback per result as it finishes), `identify` (which
work a PDF's first page is), `search` (with
`searchCapabilities` saying what each source can search by, and
`searchSupports` whether it can search a query tree, and optionally
`since` a day), offline
`scan`, `claim_from_page` (an article page's claim, from its rendered
markup, address and dragged title: see Pages), `split_references` and
`split_lines` (laid-out lines with
indents, from PDFKit), `read_references` (BibTeX, RIS or CSL-JSON) and
`read_docx` (a Word document's Zotero or Mendeley citations), `export`
(BibTeX, RIS, CSL-JSON) and `cite` (a reference list in a bundled or
given CSL style, with `citation_styles`, `style_project` and
`style_credits` to list and credit them), `references` and `analyse`
(citation links, related works among them), and
for settings `budgets` (as last
reported: free), `refresh_budgets` (OpenAlex's `/rate-limit` with a key,
free; nothing asked without one; also each kind of call's price,
`Budget.prices`, `searchPriceUsd`), `check_openalex_key` (one free lookup), `cache_stats`,
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
`utf16` ranges for `NSString`, and `fromUrl` when read from a web
address. Hold one handle per app. With the `mcp`
feature (on in the xcframework), `McpServer` serves agents over HTTP
through the handle, and the app's own tools through `McpHost` (see
Agents (MCP)). `just xcframework` builds `AsperoFFI.xcframework` (static, arm64,
macOS 26+; `UNIVERSAL=1` adds x86_64, `MACOSX_DEPLOYMENT_TARGET`
overrides) and the Swift bindings; a workflow attaches both to each
release. Not published to crates.io.

Third-party notices ship inside the library and the CLI:
`dependencies()` (name, version, SPDX licence, repository; the crates
and the bundled CSL data) and `third_party_notices()`, and `aspero
licences [--full]`. `just notices` writes them, at release, to
`crates/{ffi,cli}/notices/` (`dependencies.json`, `notices.txt`); both
artefacts `include_str!` their own. The generator (`crates/notices-gen`,
not published) takes each artefact's crates from `cargo tree -e
normal,no-proc-macro --target aarch64-apple-darwin` (features resolved
for that package alone, with the ones its build enables, the FFI's `mcp`,
as its build does: `cargo metadata` unifies them
across the workspace and dev-dependencies, and names ~40 crates the
library doesn't contain). Proc macros run when building and aren't in
what ships; their own dependencies resolve for the building machine, so
counting them would make the list depend on where it's generated. It
takes their licences, repositories and authors from `cargo
metadata`. Licence texts are each crate's own `LICENSE*`, `COPYING*`,
`NOTICE*` and `COPYRIGHT*` files; copyright lines are lifted out and shown
with the crate, so identical texts (layout aside) appear once, after
every crate under them. A crate whose files don't cover its licence
expression gets the SPDX text from `crates/notices-gen/licenses`, and a
licence with no template there fails the run. The CSL styles and locales
are checked for their CC BY-SA 3.0 `<rights>` and listed as data, with
`styles --credits`. `notices-gen`'s tests (so `just check` and CI) fail
when the files are stale, and `just release-check` runs
`just notices --check`: after a version bump or a dependency change, run
`just notices`.

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
- **Pages over MCP**: a tool taking a page's markup and address (Pages),
  for agents that hold a page; not yet, as agents mostly have only the
  address, which `verify` and `lookup` already read.
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
- **Discovery**: OpenCitations (G3), watches ("new since last run"),
  history and sessions.
- **Integration**: DOI-as-citekey builds, Zotero interop.
- **hayagriva workarounds** in `bib/src/cite.rs` (a default `<name/>`;
  spaces before punctuation tidied): drop them once a release includes
  typst/hayagriva#520 and fixes empty date-part delimiters.

## Open questions

- Whether to cap OpenAlex spend per CLI command, as MCP sessions are
  (`--openalex-cap-usd` caps the day, not a command).
- The MCP budgets' defaults ($0.25 a session, $1 a key a day) once agents
  have used them for a while.
- Whether a lookup should ever hedge (ask two providers at once) for
  latency.

## Conventions

- Licence MIT OR Apache-2.0; edition 2024 on current stable, no MSRV yet.
- Conventional commits; small, focused changes; clippy and rustfmt clean.
- Tests: unit tests beside code, property tests for identifiers, fixture
  replay for providers, paused-time tests for retries, pacing and
  concurrency, the live benchmark for scoring. Fuzz targets (`fuzz/`, its
  own workspace, nightly: `just fuzz <target>`) for the readers of
  untrusted input: pages, identifiers, reference files and lists, .docx,
  claims and query trees.
