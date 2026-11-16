# Accuracy

How far to trust `verify`: what a verdict promises, how that is measured,
the bar a release has to clear, and where it stands. This applies the same
to the CLI, the MCP server and the library: they share one engine and one
verdict.

## What `verify` promises

**A fabricated or miscited reference should not come back `verified`.**
That is the promise the rest is built around. A wrongly verified citation
is the expensive error: it tells you, or the agent you've asked, to stop
looking. A real citation coming back `plausible` costs a minute's check.

The verdicts, read that way:

| Verdict | Means | What to do |
|---|---|---|
| `verified` | A real work matches the citation's title, authors and year (and the identifiers it claims, its journal, volume and page where given). | Use it. |
| `plausible` | It looks like this work, but something differs or too little was given to be sure; the reason names what. | The review queue: check it against the work shown. |
| `mismatch` | The identifier cited doesn't exist, or is another work's; or the same title with the wrong authors or year. | Fix it: the reason names the work the citation matches, where there is one. |
| `not_found` | No matching work. | Treat as unconfirmed, possibly invented. |
| `unverifiable` | The sources failed; nothing was decided. | Try again later. |

A retraction (or withdrawal, or expression of concern) is noted whatever
the verdict, and `check` makes a retracted reference an error.

What it can't check:

- **Whether the work says what it's cited for.** `verify` checks that the
  reference points at a real work with these details, not that the work
  supports the sentence citing it.
- **Works the open sources don't have.** OpenAlex, Crossref, DataCite,
  doi.org, Europe PMC and Open Library cover most journal articles,
  preprints, conference papers and books with an identifier, but not all
  grey literature, theses, reports, very old or regional works, or
  anything unregistered. Those come back `not_found` though they exist.
- **Errors the sources share.** A record wrong at the publisher (a
  misspelt name, a wrong year in the deposit) is wrong everywhere aspero
  looks.
- **Every citation style.** Fields are found in the reference rather than
  parsed per style; a journal, volume or page it can't read is not
  compared (see [DESIGN](../DESIGN.md#resolve-verify-and-check)). Giving
  fields as JSON is more reliable than free text.

## Verdict policy

What each kind of citation *should* get, whatever it gets today, is
written down as a table in
[bench/redteam/README.md](../bench/redteam/README.md#verdict-policy). In
short:

- **Real and right** (including ordinary human imperfections: initials,
  "et al.", an abbreviated journal, a dropped subtitle, a typo, no pages):
  `verified`, `plausible` acceptable for the imperfect.
- **A real work with one detail wrong** (year off by one, the wrong
  journal, volume or page, invented co-authors, a preprint cited as the
  journal article): `plausible`, naming what differs. A year off by two or
  more: `mismatch` (`plausible` at most).
- **Wrong in substance** (real title with the wrong authors, a chimera of
  two works, another work's DOI): `mismatch` or `not_found`, never
  `verified`.
- **Invented** (the work, or the identifier): `not_found`, or `mismatch`
  with the real work suggested when only the identifier is invented.
- **Retracted**: whatever the details earn, always with the retraction
  flagged.

## Method

The red team lives in [bench/redteam/](../bench/redteam/) and is run by
[scripts/redteam.py](../scripts/redteam.py): every claim goes through the
release build's `aspero verify - --format json` in one batch, live against
the sources, and is scored against its expected verdicts. `bench/citations.jsonl`
is the smaller known-answer benchmark run in development.

**The corpus.** Real citations, written as people write them: landmark and
randomly sampled works across medicine, the life sciences, physics,
computer science, the social sciences and the humanities, in several
languages; journal articles, preprints, conference papers, books,
chapters, datasets, errata, editorials and letters; in Vancouver, AMA,
APA, Harvard, Nature, IEEE, BibTeX-like, prose and structured form; some
with ordinary human imperfections. Against them, wrong citations in these
categories:

- invented works;
- a real title with wrong authors, or the right first author with invented
  co-authors;
- a real work with the wrong year, journal or locator (volume, page);
- chimeras: one work's title with another's authors, journal, year or
  locator;
- DOI swaps: one work's details with another work's DOI;
- well-formed identifiers (DOI, PMID, arXiv) that don't exist;
- retracted works, cited without saying so;
- preprint swaps: a preprint cited as the journal article, and the
  reverse;
- LLM-style hallucinations: plausible references on real topics, as a
  language model produces them.

**Fabricated entries use invented names.** Wrong citations carry invented
author names (or initials only), and every invented title and identifier
is checked against the sources to make sure no such work exists. They are
test data and must not be cited, indexed or attributed to anyone.

**Held-out rounds.** A corpus that fixes were tuned on overstates how well
they work. So each round of fixes is judged on a fresh corpus written
afterwards, which the fixes never saw: at least 300 wrong and at least 300
real citations, built the same way. Only held-out numbers count as
results.

**Adversarial rounds.** Separately, citations are crafted against the
rules themselves, the way a language model misremembers a well-known
paper (an extended title, invented co-authors, a year in the title masking
the wrong year), to find what a rule lets through. What gets through is
minimised to the smallest citation that still does, and recorded with the
rule responsible in `bench/redteam/failures.jsonl`.

## Acceptance criteria

Measured on the held-out corpus:

| | Target |
|---|---|
| Wrong → `verified` | ≤ 2% overall, and **0** for invented works, nonexistent identifiers and DOI swaps |
| Real → `mismatch` or `not_found` | ≤ 3% |
| Real → `verified` | ≥ 85% |
| Real → `verified` or `plausible` | ≥ 95% |
| Wrong → `plausible`, naming the field that differs | ≥ 80% |
| Retracted works flagged | 100% |

Medicine and the life sciences, where a wrong reference does the most
harm, are reported separately as well as in the totals.

**Release gate.** A change to matching must not make the held-out wrong →
`verified` rate worse. Every failure fixed becomes an offline test (a
recorded fixture), so it stays fixed without the network.

## Results

**Results will be published after the current fix and held-out round.**

The starting point, for honesty's sake: aspero 0.26.0 on the corpus the
current fixes are being tuned on (322 citations, 2026-11-16; details in
[bench/redteam/README.md](../bench/redteam/README.md)). Not a held-out
result:

| | 0.26.0 baseline |
|---|---|
| Wrong → `verified` | 31/152 (20.4%) |
| Invented works, DOI swaps and nonexistent identifiers caught | 69/69 |
| Real → `verified` or `plausible` | 147/155 (94.8%) |
| Retracted works flagged | 15/15 |

What got through was a *real* work cited wrongly: invented co-authors,
the wrong journal, an extended title, a chimera. An adversarial round of
58 citations built on those weaknesses verified 48 of them. Both sets are
in `bench/redteam/failures.jsonl`, each with a minimal reproduction and
the rule that let it through.

## Reproducing it

```sh
cargo build --release -p aspero
OPENALEX_API_KEY=… ASPERO_MAILTO=you@example.org scripts/redteam.py --failures
scripts/redteam.py --rescore --failures   # re-score saved results, offline
```

- **Cost.** Title searches on OpenAlex are paid ($0.001 each, from a free
  key's $1 daily allowance). The script caps the day's OpenAlex spend at
  $0.75 unless `ASPERO_OPENALEX_CAP_USD` says otherwise; a first run of
  the corpus cost about $0.30, and re-runs within the cache's lifetime
  (its own cache, `target/redteam-cache`) are free.
- **Contact address.** The sources ask callers for one, for their polite
  pools. Supply your own in the `ASPERO_MAILTO` environment variable; it
  is never stored in the corpus or results.
- Sources change: records are corrected, works are added, retractions are
  issued. A re-run months later may differ for reasons that have nothing
  to do with aspero.

## Reporting a wrong verdict

If `verify` gets one wrong, [open a wrong-verdict
issue](https://github.com/transilluminate/aspero/issues/new?template=wrong-verdict.yml).
From the CLI, `aspero verify --report "<citation>"` prints a link with the
form already filled in (the citation, the verdict, the JSON output, the
version). Nothing is sent: you open the link if you choose, and check it
first, as issues are public. Please report only citations from published
or otherwise shareable sources, never from an unpublished manuscript.

Each confirmed report becomes a case in the red-team corpus
(`bench/redteam/reports.jsonl`, by `scripts/issue-to-case.py`), and its fix
an offline test.
