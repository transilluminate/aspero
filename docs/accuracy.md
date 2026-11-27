# Accuracy

How far to trust `verify`: what a verdict promises, how that is measured,
the bar a release has to clear, and where it stands. This applies the same
to the CLI, the MCP server and the library: they share one engine and one
verdict.

> **The results below measure the hard tail on purpose.** The wrong
> citations in every red-team corpus are built to be difficult. Each is a
> real work with one field subtly off: a sister journal, one co-author
> swapped, the year one out, an older Cochrane version, a chapter cited
> as an article. These are mistakes a careful reader misses too. The
> obvious errors are invented works, identifiers that don't exist, and
> another paper's DOI or PMID. aspero has caught every one of those in
> every round. So "wrong → `verified` 7.4%" means 7.4% of near-misses,
> not 7.4% of what gets pasted. The real citations are messy on purpose
> too: typos, abbreviations, missing fields, bare arXiv numbers, model
> output. The targets are set high, and each round climbs the hardest
> cases that are left.

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

These are the hardest cases (see the note at the top), not typical
input.

**Round 9 (0.43.1), real model output, held out** —
`bench/redteam/wild-6.jsonl`. It measures the hard tail on purpose, and
more than wild-5: its prompts were weighted toward the requests that draw
the most errors (identifiers asked, named citation styles, recent work,
grey literature) and its writers toward the model that errs most, so that
wrong references are numerous enough to measure. The corpus holds 748
references from 120 model sessions, each checked by hand without aspero
(see [wild-6.md](../bench/redteam/wild-6.md)). 83 are wrong, so one wrong
`verified` is 1.2 points. It was run live on 2026-11-26 and 2026-11-27, on
the release it was held out from:

| Criterion | 0.43.1 | Target |
|---|---|---|
| Wrong → `verified` | 8/83 (9.6%) | ≤ 2% — not yet met |
| Correct → `verified` | 564/665 (84.8%) | ≥ 85% — not yet met |
| Correct → `verified` or `plausible` | 630/665 (94.7%) | ≥ 95% — not yet met |
| Correct → `mismatch` or `not_found` | 35/665 (5.3%) | ≤ 3% — not yet met |
| Wrong → `plausible` naming the field that differs | 36/36 (100%) | ≥ 80% |

Each wrong verified matched the work the model meant with one field
wrong: five reworded titles, two wrong journals, one changed co-author.
Half the correct citations missed are arXiv papers listed by a short
label beside a correct identifier, from two answers; the rest are books,
grey literature, and three DOIs with a dagger glued to their end. Against
wild-5 on 0.43.0, the real-citation figures are level or better and wrong
→ `verified` is lower, on a harder corpus.

**Round 8 (0.43.0), real model output, held out** —
`bench/redteam/wild-5.jsonl`. Like wild-4, it measures the hard tail on
purpose: references as models write them, whose wrong ones are near-misses
of real works. This time the prompts came from a writer who knew nothing
of aspero, and the references were judged without it. The corpus holds 864
references from 100 model sessions, each checked by hand (see
[wild-5.md](../bench/redteam/wild-5.md)). Only 37 are wrong, so one wrong
`verified` is 2.7 points. It was run live on 2026-11-24 and 2026-11-25, on
the release it was held out from:

| Criterion | 0.43.0 | Target |
|---|---|---|
| Wrong → `verified` | 5/37 (13.5%) | ≤ 2% — not yet met |
| Correct → `verified` | 693/827 (83.8%) | ≥ 85% — not yet met |
| Correct → `verified` or `plausible` | 783/827 (94.7%) | ≥ 95% — not yet met |
| Correct → `mismatch` or `not_found` | 44/827 (5.3%) | ≤ 3% — not yet met |
| Wrong → `plausible` naming the field that differs | 15/15 (100%) | ≥ 80% |

The one invented work was not verified. Each of the five wrong verifieds
matched the work the model meant, with one field wrong: three titles cut
or rewritten, and two in the wrong journal with no volume or pages to
check it against. In medicine and allied health, 3 of 21 wrong references
verified and 87.8% of correct ones did.

The correct citations missed are mostly books (27 of 44), among them
nine Italian titles and books cited by their main title alone, and
journal references written without a title (9). Every real-citation
figure is better than wild-4's held-out run on 0.38.3.

**Round 7 (0.38.3), real model output, held out** —
`bench/redteam/wild-4.jsonl`. Like every red-team corpus, it measures the
hard tail on purpose: references as models write them, in table cells and
beside trial labels and notes, whose wrong ones are near-misses of real
works. It doesn't measure typical use. The corpus holds 1,135 references
from 100 fresh model sessions on new topics, each checked by hand (see
[wild-4.md](../bench/redteam/wild-4.md)). 75 of them are wrong, so one
wrong `verified` is 1.3 points. It was run live on 2026-11-22, on the
release it was held out from:

| Criterion | 0.38.3 | Target |
|---|---|---|
| Wrong → `verified` | 2/75 (2.7%) | ≤ 2% — not yet met |
| Correct → `verified` | 875/1060 (82.5%) | ≥ 85% — not yet met |
| Correct → `verified` or `plausible` | 979/1060 (92.4%) | ≥ 95% — not yet met |
| Correct → `mismatch` or `not_found` | 72/1060 (6.8%) | ≤ 3% — not yet met |
| Wrong → `plausible` naming the field that differs | 20/21 (95.2%) | ≥ 80% |

The two wrong verifieds:
- an arXiv paper cited at another meeting than its own, where only the
  arXiv record was found;
- a journal article cited as a working paper whose number is another
  paper's.

None was in medicine or allied health (0 of 41 wrong there). Most of the
correct citations missed were model-written lists read wrongly (trial
labels and commentary in table cells, bare ISBN-13s, short journal names
such as "PRD"), and books whose own title was never searched.

0.39's fixes were tuned on wild-4, so wild-4 no longer counts as held out.
On 0.39.1, wild-4 gives:
- wrong → `verified` 1/75;
- correct → `verified` 956/1060 (90.2%);
- correct → `verified` or `plausible` 1031/1060 (97.3%);
- correct → `mismatch` or `not_found` 11/1060 (1.0%).

wild-5 (round 8, above) was the next held-out measurement.

**Every corpus on 0.39.1.** These corpora have all been tuned on, so they
check that the fixes hold. They aren't held-out scores. All were run live
on 2026-11-22:

| Corpus | Wrong → `verified` | Real → `mismatch` or `not_found` | Real → `verified` | Real → `verified` or `plausible` |
|---|---|---|---|---|
| holdout-1 | 2/351 (0.6%) | 2/382 (0.5%) | 354/382 (92.7%) | 380/382 (99.5%) |
| holdout-2 | 1/399 (0.3%) | 1/441 (0.2%) | 377/441 (85.5%) | 430/441 (97.5%) |
| holdout-5 | 13/613 (2.1%) | 10/583 (1.7%) | 535/583 (91.8%) | 573/583 (98.3%) |
| holdout-6 | 39/516 (7.6%) | 10/503 (2.0%) | 473/503 (94.0%) | 493/503 (98.0%) |
| wild-2 | 4/122 (3.3%) | 7/910 (0.8%) | 835/910 (91.8%) | 902/910 (99.1%) |
| wild-3 | 9/122 (7.4%) | 25/1013 (2.5%) | 850/1013 (83.9%) | 988/1013 (97.5%) |
| wild-4 | 1/75 (1.3%) | 11/1060 (1.0%) | 956/1060 (90.2%) | 1031/1060 (97.3%) |
| Target | ≤ 2% | ≤ 3% | ≥ 85% | ≥ 95% |

The other criteria:
- Invented works, nonexistent identifiers and another work's DOI or PMID:
  none verified, in every corpus that has them (0/169, 0/146, 0/180,
  0/182).
- Wrong → `plausible` naming the field that differs: 92.9% or more
  everywhere.
- Retracted works flagged: all of them, except one of holdout-1's 27.

0.39.2 changed two things, by an offline replay:
- that retraction is flagged (27/27);
- 6 of holdout-6's 8 chapters cited as articles are now `plausible`, so
  its wrong → `verified` is 33/516 (6.4%).

What's left of wrong → `verified`:
- **holdout-6's books, most of it.** A book cited with another publisher
  verifies 12/12, and a later edition's year cited as the first verifies
  9/12. Today's records can't tell which edition a year or publisher
  belongs to. holdout-6's other 18 (12 on 0.39.2) span ten kinds, one to
  seven each.
- **Small groups elsewhere.** holdout-5's 13 are spread over nine kinds,
  wild-2's 4 are reworded titles, and wild-3's 9 span six kinds.

wild-3's correct → `verified` (83.9%) is the one real-citation figure
under its target.

**Round 6 (0.32.0 candidate), held out** — `bench/redteam/holdout-6.jsonl`:
1,049 citations built the same way as holdout-5, weighted towards books
and monographs (humanities included), reprints, model-style formatting,
preprints and conference papers, with three new kinds of wrong book
citation. Run live on 2026-11-17, beside 0.31.0 on the same requests:

| Criterion | 0.31.0 | Round 6 | Target |
|---|---|---|---|
| Wrong → `verified` | 45/516 (8.7%) | 38/516 (7.4%) | ≤ 2% — not yet met |
| … leaving out the new book-publisher and book-edition-year kinds | 25/480 (5.2%) | 18/480 (3.8%) | |
| Invented works, nonexistent identifiers, another work's DOI or PMID → `verified` | 0/182 | 0/182 | 0 |
| Real → `mismatch` or `not_found` | 11/503 (2.2%) | 10/503 (2.0%) | ≤ 3% |
| Real → `verified` | 471/503 (93.6%) | 473/503 (94.0%) | ≥ 85% |
| Real → `verified` or `plausible` | 492/503 (97.8%) | 493/503 (98.0%) | ≥ 95% |
| Wrong → `plausible` naming the field that differs | 200/211 (94.8%) | 207/217 (95.4%) | ≥ 80% |
| Retracted works flagged | 30/30 | 30/30 | 100% |

What gets through: a book cited with another publisher (12 of 12; aspero
doesn't compare publishers, which change with imprints and reissues) or a
later edition's year as if it were the first (8 of 12), and a book chapter
cited as an article in a "journal" named after the book with an invented
volume (7 of 8). On the corpora round 6 was tuned on, wrong → `verified`
fell from 36/613 to 13/613 (holdout-5), and correct citations missed from
52/910 to 6/910 (wild-2, offline replay).

**Round 6, real model output, held out** — `bench/redteam/wild-3.jsonl`:
1,135 references from 100 fresh Claude Haiku and Sonnet sessions, weighted
towards asks for exact details (DOIs, PMIDs, pages), each checked by hand.
Run live on 2026-11-18:

| Criterion | 0.31.0 | Round 6 | Target |
|---|---|---|---|
| Wrong → `verified` | 26/122 (21.3%) | 25/122 (20.5%) | ≤ 2% — not yet met |
| Correct → `verified` | 783/1013 (77.3%) | 836/1013 (82.5%) | ≥ 85% — not yet met |
| Correct → `verified` or `plausible` | 922/1013 (91.0%) | 971/1013 (95.9%) | ≥ 95% |
| Correct → `mismatch` or `not_found` | 88/1013 (8.7%) | 42/1013 (4.1%) | ≤ 3% — not yet met |

Round 6 halved the correct citations called wrong. What still gets
through is mostly one thing: a correct DOI beside a PMID a few digits off,
which belongs to an unrelated paper (17 of the 25). The identifier check
passes when any cited identifier agrees, and a bare PMID in a table cell
isn't read (#148). Without those, about 8 of 122.

**aspero 0.30.0, held out** — `bench/redteam/holdout-5.jsonl`: 1,232
citations built independently of the code (by an agent that never read the
matching rules or earlier corpora), shaped like what people paste: reference
lists from theses, reviews, guidelines and papers, about half from medicine
and allied health, including Cochrane versions, guideline editions, grey
literature, conference papers, preprints, errata and non-English works. Run
live on 2026-11-16.

| Criterion | Result | Target |
|---|---|---|
| Wrong → `verified` | 35/613 (5.7%) | ≤ 2% — not yet met |
| Invented works, nonexistent identifiers, another work's DOI or PMID → `verified` | 0/180 | 0 |
| Real → `mismatch` or `not_found` | 12/583 (2.1%) | ≤ 3% |
| Real → `verified` | 533/583 (91.4%) | ≥ 85% |
| Real → `verified` or `plausible` | 571/583 (97.9%) | ≥ 95% |
| Wrong → `plausible` naming the field that differs | 276/286 (96.5%) | ≥ 80% |
| Retracted works flagged | 36/36 | 100% |

What still gets through is a real work cited with one subtle error: the
wrong conference of the same field (8 of 28), a journal article cited as
a preprint (4 of 8) or as a book chapter (4 of 10), a sister journal (4
of 27), an older Cochrane version or guideline edition (2 of 14 each).
Nothing invented, and no identifier belonging to another work, was
verified.

**How it got here.** Each round fixed what one corpus showed and was then
measured on a corpus it had never seen:

| Round (release) | Measured on | Wrong → `verified` | Real accepted |
|---|---|---|---|
| — (0.26.0) | the first corpus, as found | 31/152 (20.4%) | 94.8% |
| 1 (0.27.0) | holdout-1 (unseen) | 29/351 (8.3%) | 97.6% |
| 2 | holdout-2 (unseen, harder formats) | 40/399 (10.0%) | 90.5% |
| 3 | holdout-3 (unseen) | 36/426 (8.5%) | 96.5% |
| 4 | holdout-4 (unseen, subtle errors) | 74/481 (15.4%) | 98.1% |
| 5 (0.30.0) | holdout-5 (unseen) | 35/613 (5.7%) | 97.9% |
| 6 (0.32.0 candidate) | holdout-6 (unseen, books and reprints) | 38/516 (7.4%) | 98.0% |
| 6 (0.32.0 candidate) | wild-3 (unseen model output) | 25/122 (20.5%) | 95.9% |
| 7 (0.38.3) | wild-4 (unseen model output) | 2/75 (2.7%) | 92.4% |
| 8 (0.43.0) | wild-5 (unseen model output, naive prompts) | 5/37 (13.5%) | 94.7% |
| 9 (0.43.1) | wild-6 (unseen model output, weighted to error-prone requests) | 8/83 (9.6%) | 94.7% |

Each held-out corpus was built to be harder than the last, so the rates
aren't a smooth series; what they show is that invented works and other
works' identifiers have been caught throughout, and that each round's
fixes generalise to the kinds of error they target (on the corpora each
round was tuned on, wrong → `verified` reached 0–2%).

**Real model output.** `bench/redteam/wild-1.jsonl`: 394 references
written by Claude Haiku and Sonnet asked for literature reviews without
tools, each checked by hand against Crossref, Europe PMC and Open Library.
93% (Sonnet) and 86% (Haiku) were correct; the commonest error was
invented co-authors after a correct first author. 0.30.0 verifies 85.7%
of the correct ones and lets 3 of the 31 wrong ones through (a chimera of
two trials, another paper's NBER number, an invented book co-author); it
was tuned on this corpus, so treat it as a check, not a held-out score.

**Real model output, held out.** `bench/redteam/wild-2.jsonl`: 1,032
references from 100 fresh sessions of Claude Haiku and Sonnet asked, without
tools, for reviews, reference lists and exact citations (half in medicine
and allied health; some in Spanish, German, French, Portuguese and
Japanese), each checked by hand. None was a wholly invented work: every
wrong one was a real work cited wrongly, most often by a DOI a few digits
off that lands on another paper in the same issue (Haiku 21% wrong, Sonnet
7%). 0.30.0 had never seen it:

| Criterion | Result | Target |
|---|---|---|
| Wrong → `verified` | 5/122 (4.1%) | ≤ 2% — not yet met |
| Correct → `verified` | 777/910 (85.4%) | ≥ 85% |
| Correct → `verified` or `plausible` | 857/910 (94.2%) | ≥ 95% — not yet met |
| Correct → `mismatch` or `not_found` | 52/910 (5.7%) | ≤ 3% — not yet met |

None of the near-miss DOIs verified. The 52 correct citations aspero
missed are mostly books it doesn't find (humanities monographs in Open
Library or Crossref), bare arXiv numbers in model output, and reprints;
they're the next round's target (issue #126).

The corpora, their method notes (`bench/redteam/holdout-*.md`,
`wild-*.md`) and each round's notes (`bench/redteam/README.md`) are in the
repository.

## Reproducing it

```sh
cargo build --release -p aspero
OPENALEX_API_KEY=… ASPERO_MAILTO=you@example.org scripts/redteam.py --failures
scripts/redteam.py --rescore --failures   # re-score saved results, offline
```

- **Cost.** Title searches on OpenAlex are paid ($0.001 each, from a free
  key's $1 daily allowance). The script caps the day's OpenAlex spend at
  $0.75 unless `ASPERO_OPENALEX_CAP_USD` says otherwise, and prepaid
  credit, once the allowance is used up, at $5 a day unless
  `ASPERO_OPENALEX_CREDIT_CAP_USD` says otherwise; a first run of
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
