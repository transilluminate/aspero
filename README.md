# aspero

Find, resolve and **verify** scholarly references from the command line —
built for AI agents first, and humans too.

Agents asked to cite literature tend to recall DOIs from memory (often
wrong) or try web search (usually blocked for bots). aspero asks the open
scholarly APIs instead — OpenAlex, Crossref, DataCite, doi.org, Europe PMC
and Open Library — politely, with caching, and answers in JSON with exit
codes an agent can act on.

```console
$ aspero verify "LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015. doi:10.1038/nature14540"
MISMATCH  LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015. doi:10.1038/nature14540
  doi:10.1038/nature14540 is "Reinforcement learning improves behaviour from evaluative feedback" (2015), openalex:W1589747210, but the citation matches "Deep learning" (2015), doi:10.1038/nature14539
```

## Install

Homebrew (macOS, Linux):

```sh
brew install transilluminate/tap/aspero
```

Prebuilt binary, via the install script:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/transilluminate/aspero/releases/latest/download/aspero-installer.sh | sh
```

From source, with Cargo:

```sh
cargo install aspero
```

Optional environment:

| Variable | Purpose |
|---|---|
| `OPENALEX_API_KEY` | Free OpenAlex key ($1/day budget; lookups free, searches $0.001) |
| `ASPERO_MAILTO` | Your contact address, sent to polite API pools (Crossref) |
| `ASPERO_CACHE_DIR` | Where to keep the answer cache |

More settings (a spending cap, sources to leave out, the cache's trust and
size) are under [Settings](#settings).

## Use

```sh
# Does this citation refer to a real work, with matching details?
aspero verify "Vaswani 2017 Attention is all you need"
aspero verify --title "Deep learning" --author LeCun --year 2015 --id 10.1038/nature14539
aspero verify - < citations.txt        # one claim per line, text or JSON

# Check a whole bibliography: BibTeX, RIS, CSL-JSON, or a Word document's
# Zotero/Mendeley citations (CI-friendly: exits 9 on errors)
aspero check refs.bib
aspero check paper.docx
aspero check --fail-on warning --format json thesis.ris

# Write what's found as BibTeX, RIS or CSL-JSON, or as a reference list in a style
aspero lookup --format bibtex 10.1038/nature14539 arXiv:1706.03762
aspero verify --format ris - < citations.txt
aspero search --format csl-json "sepsis fluid resuscitation" > zotero.json
aspero verify --style vancouver - < citations.txt   # `aspero styles` lists them

# Look up identifiers: DOIs, arXiv IDs, PMIDs, PMCIDs, NBK IDs, ISBNs, URLs
aspero lookup 10.1038/nature14539 arXiv:1706.03762 "ISBN 978-0-262-03561-3"

# Which paper is this PDF? From its first page's text and title
pdftotext -l 1 paper.pdf - | aspero identify --title "Deep learning"

# Find the identifiers in documents (offline), then look them up
aspero scan notes/*.md
aspero scan --format jsonl page.md | aspero lookup -   # each result echoes its mention

# Search, resolve, and include abstracts
aspero search "protein folding" --year 2020.. --source europepmc
aspero search --author lecun --title "deep learning" --sort most-cited
aspero search --query '{"all": [{"cites": "10.1038/nature14539"}, {"title": "survey"}, {"not_retracted": true}]}'
aspero search --query '{"all": [{"id": "10.1038/nature14539"}, {"not_retracted": true}]}'   # that work, if not retracted
aspero search sepsis --since 2026-11-01      # only works added since: watch a search
aspero resolve --abstract "Jumper J et al. Highly accurate protein structure prediction with AlphaFold. Nature 2021"

# Citation links: a work's references; and a set of works checked for
# retracted references, odd ones out, works probably missing from it, and
# related works
aspero references 10.1038/nature14539 --limit 50
aspero analyse refs.bib --estimate          # what it would cost, free
aspero analyse refs.bib --budget 0.01       # fractions of a cent for 50 works
aspero analyse refs.bib --kinds related     # works around the set, ~$0.006
aspero analyse 10.1056/nejmoa010307 10.1056/nejmoa1401602 10.1056/nejmoa1500896

# What's used of today's OpenAlex budget (free; needs a key)
aspero budget

# The crates and data aspero is built from, and their licences; --full
# prints the third-party notices
aspero licences

# Watch the requests (and concurrency) as they happen
aspero verify -v "Bishop CM. Pattern Recognition and Machine Learning. 2006"
```

On a terminal, results are tables (a single `verify` is a short report);
piped, they're JSON. Pick explicitly with `--format table|text|json|jsonl`:
`text` is a detailed report per result. `lookup`, `search` and `verify`
also take `--format bibtex|ris|csl-json`, and `--style apa|vancouver|…` or a `.csl`
file for a formatted reference list. Every command's `--help` has
examples and the exit codes.

What `verify` and `check` catch: invented papers, real DOIs attached to the
wrong work, nonexistent DOIs (with the right one suggested), wrong years,
misattributed authors, retracted papers, duplicates, and missing DOIs.
Lookalike re-deposits of famous papers are told apart by year and authors.
`analyse` reads the citation links between a set of works (from OpenAlex):
which cite retracted work, which are linked to none of the others,
which works most of them cite but the set lacks, and (`--kinds related`)
which works around them are read with them or share their references. A work whose references
OpenAlex doesn't have (often a book) is reported as such, never as
unlinked.

## Agents (MCP)

Agents that speak the Model Context Protocol can use aspero as tools
rather than through the shell. `aspero mcp` serves them over stdio, for the
agent to launch; with Claude Code:

```sh
claude mcp add aspero -- aspero mcp
```

The tools: `search` (a query tree, the JSON that `search --query` takes, or
keywords), `search_supports`, `lookup`, `verify` (one citation, text or
fields, or a batch), `identify` (a first page's text), and `citing_works` and
`references` (the works citing a work, and its references). Results are the
JSON the commands print, as structured content with a short text summary;
`full: true` gives every field.

Each session may spend up to `--budget` dollars (default $0.25)
on OpenAlex searches; once it has, tools that may spend are refused. Every
result reports its usage and the session's spend. The environment, cache
and pacing are the other commands'.

### Agents (MCP) in an app

An app that embeds aspero (the FFI's `McpServer`, in the xcframework) can
serve the same tools, and its own, over Streamable HTTP on
`http://127.0.0.1:<port>/mcp` (default port 23130), to agents holding a
bearer key the app minted:

```sh
claude mcp add --transport http aspero http://127.0.0.1:23130/mcp \
  --header "Authorization: Bearer <token>"
```

Only loopback, only with a key (aspero keeps its SHA-256, never the
token), and never from a browser. Each key has scopes (the sources;
reading or adding to the app's history) and a daily budget; each session
its own budget. The app hears of every tool call as it happens.

## How it behaves

- **Cheap**: free sources first; paid OpenAlex searches only when needed,
  with spend reported. A day's free budget covers hundreds of verifications.
- **Polite**: per-provider pacing, backoff on rate limits, contact address
  in the user agent; lookups fall back provider by provider rather than
  asking all at once.
- **Cached**: answers are kept (records 7 days, searches and "not found" 1
  day, references 30 days), so re-checking a bibliography is near-instant and free. `aspero
  cache stats|clear|path`. Before submitting, `--fresh` checks again past
  the cache (any retraction or correction since, a work registered since
  it was "not found") and caches the new answers; where a source fails,
  the cached answer is served, marked stale. `--no-cache` neither reads
  nor writes the cache.

## Settings

Each is a global flag, or its environment variable (the app sets them in
its `Config`):

| Flag (variable) | Effect |
|---|---|
| `--openalex-cap USD` (`ASPERO_OPENALEX_CAP`) | Stop OpenAlex's paid calls once today's use, as OpenAlex reports it for the key, reaches this; lookups (free) go on, checks fall back to the free sources, searches that need OpenAlex fail (exit 5). `aspero budget` shows today's use. |
| `--without europepmc,openlibrary` (`ASPERO_DISABLED_SOURCES`) | Sources never to ask: `openalex`, `crossref`, `datacite`, `doi`, `europepmc`, `openlibrary`. |
| `--cache-found-days N` (`ASPERO_CACHE_FOUND_DAYS`) | How long a found record is trusted (default 7). |
| `--cache-not-found-days N` (`ASPERO_CACHE_NOT_FOUND_DAYS`) | How long a "not found" is trusted (default 1). |
| `--cache-max-mb N` (`ASPERO_CACHE_MAX_MB`) | Limit the cache's size; the least recently used answers go first. `aspero cache stats` shows the size. |

## Library

The CLI is a thin adapter over `aspero-engine`, with its JSON shapes in
`aspero-json`; providers are separate crates behind features, and the MCP
server is `aspero-mcp`. Apps use `aspero-ffi`: Swift bindings and an
`.xcframework` attached to each release (`just xcframework` builds them).
See [DESIGN.md](DESIGN.md) for the architecture, scoring rules, provider
gotchas and plans.

Swift 6: the bindings UniFFI 0.32 generates (`aspero_ffi.swift`) don't yet
compile in Swift 6 language mode: its support for async foreign traits
(`McpHost`) passes non-`Sendable` closures to a `Task`. Compile that file's
target in Swift 5 mode (in SwiftPM, `swiftSettings:
[.swiftLanguageMode(.v5)]`); code using it can stay in Swift 6.

## Development

`just check` runs what CI runs (fmt, clippy, tests); `just` lists the rest:
the live benchmark, fixture recording, a release dry run.

## Licence

MIT OR Apache-2.0, at your option.

The bundled citation styles and locales are from the Citation Style
Language project, under CC BY-SA 3.0; see `crates/bib/styles/NOTICE.md`
and `aspero styles --credits`.

The crates aspero is built from keep their own licences: `aspero licences`
lists them, and `aspero licences --full` prints the third-party notices
(each licence text, with the crates under it and their copyright lines).
Apps get the same from the FFI's `dependencies()` and
`thirdPartyNotices()`.
