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

# Write what's found as BibTeX or RIS, or as a reference list in a style
aspero lookup --format bibtex 10.1038/nature14539 arXiv:1706.03762
aspero verify --format ris - < citations.txt
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
aspero search sepsis --since 2026-11-01      # only works added since: watch a search
aspero resolve --abstract "Jumper J et al. Highly accurate protein structure prediction with AlphaFold. Nature 2021"

# Citation links: a work's references; and a set of works checked for
# retracted references, odd ones out and works probably missing from it
aspero references 10.1038/nature14539 --limit 50
aspero analyse refs.bib --estimate          # what it would cost, free
aspero analyse refs.bib --budget 0.01       # fractions of a cent for 50 works
aspero analyse 10.1056/nejmoa010307 10.1056/nejmoa1401602 10.1056/nejmoa1500896

# Watch the requests (and concurrency) as they happen
aspero verify -v "Bishop CM. Pattern Recognition and Machine Learning. 2006"
```

On a terminal, results are tables (a single `verify` is a short report);
piped, they're JSON. Pick explicitly with `--format table|text|json|jsonl`:
`text` is a detailed report per result. `lookup`, `search` and `verify`
also take `--format bibtex|ris`, and `--style apa|vancouver|…` or a `.csl`
file for a formatted reference list. Every command's `--help` has
examples and the exit codes.

What `verify` and `check` catch: invented papers, real DOIs attached to the
wrong work, nonexistent DOIs (with the right one suggested), wrong years,
misattributed authors, retracted papers, duplicates, and missing DOIs.
Lookalike re-deposits of famous papers are told apart by year and authors.
`analyse` reads the citation links between a set of works (from OpenAlex):
which cite retracted work, which are linked to none of the others, and
which works most of them cite but the set lacks. A work whose references
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

## How it behaves

- **Cheap**: free sources first; paid OpenAlex searches only when needed,
  with spend reported. A day's free budget covers hundreds of verifications.
- **Polite**: per-provider pacing, backoff on rate limits, contact address
  in the user agent; lookups fall back provider by provider rather than
  asking all at once.
- **Cached**: answers are kept (records 7 days, searches and "not found" 1
  day, references 30 days), so re-checking a bibliography is near-instant and free. `aspero
  cache stats|clear|path`.

## Library

The CLI is a thin adapter over `aspero-engine`, with its JSON shapes in
`aspero-json`; providers are separate crates behind features, and the MCP
server is `aspero-mcp`. Apps use `aspero-ffi`: Swift bindings and an
`.xcframework` attached to each release (`just xcframework` builds them).
See [DESIGN.md](DESIGN.md) for the architecture, scoring rules, provider
gotchas and plans.

## Development

`just check` runs what CI runs (fmt, clippy, tests); `just` lists the rest:
the live benchmark, fixture recording, a release dry run.

## Licence

MIT OR Apache-2.0, at your option.

The bundled citation styles and locales are from the Citation Style
Language project, under CC BY-SA 3.0; see `crates/bib/styles/NOTICE.md`
and `aspero styles --credits`.
