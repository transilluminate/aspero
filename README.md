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

```sh
brew install transilluminate/tap/aspero          # macOS / Linux
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/transilluminate/aspero/releases/latest/download/aspero-installer.sh | sh
cargo install --git https://github.com/transilluminate/aspero aspero   # from source
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

# Check a whole bibliography (CI-friendly: exits 9 on errors)
aspero check refs.bib
aspero check --fail-on warning --format json thesis.bib

# Look up identifiers: DOIs, arXiv IDs, PMIDs, PMCIDs, ISBNs, URLs
aspero lookup 10.1038/nature14539 arXiv:1706.03762 "ISBN 978-0-262-03561-3"

# Search, resolve, and include abstracts
aspero search "protein folding" --year 2020.. --source europepmc
aspero resolve --abstract "Jumper J et al. Highly accurate protein structure prediction with AlphaFold. Nature 2021"

# Watch the requests (and concurrency) as they happen
aspero verify -v "Bishop CM. Pattern Recognition and Machine Learning. 2006"
```

On a terminal, results are tables (a single `verify` is a short report);
piped, they're JSON. Pick explicitly with `--format table|text|json|jsonl`:
`text` is a detailed report per result. Every command's `--help` has
examples and the exit codes.

What `verify` and `check` catch: invented papers, real DOIs attached to the
wrong work, nonexistent DOIs (with the right one suggested), wrong years,
misattributed authors, retracted papers, duplicates, and missing DOIs.
Lookalike re-deposits of famous papers are told apart by year and authors.

## How it behaves

- **Cheap**: free sources first; paid OpenAlex searches only when needed,
  with spend reported. A day's free budget covers hundreds of verifications.
- **Polite**: per-provider pacing, backoff on rate limits, contact address
  in the user agent; lookups fall back provider by provider rather than
  asking all at once.
- **Cached**: answers are kept (records 7 days, searches and "not found" 1
  day), so re-checking a bibliography is near-instant and free. `aspero
  cache stats|clear|path`.

## Library

The CLI is a thin adapter over `aspero-engine`; providers are separate
crates behind features. See [DESIGN.md](DESIGN.md) for the architecture,
scoring rules, provider gotchas and plans.

## Licence

MIT OR Apache-2.0, at your option.
