# Red team: does a wrong citation ever come back VERIFIED?

aspero's promise: a fabricated or wrong citation must not verify; a real,
correct one should. `bench/citations.jsonl` checks that on 49 hand-picked
cases. This directory measures it at scale and records what gets through.

**The fabricated entries here are invented test data.** Categories `a`–`i`
in `corpus.jsonl` and every `adversarial` entry in `failures.jsonl` cite
works that don't exist, or real works with deliberately wrong details. They
use invented author names (unusual hyphenated pairs) or initials only, and
must not be cited, indexed or attributed to anyone.

## Verdict policy

What each kind of citation should get, from DESIGN ("Resolve, verify and
check") and `crates/core/src/matching/mod.rs` (verified ≥ 0.85, plausible
≥ 0.6).

| Citation | Should get |
|---|---|
| Real, every field right | verified |
| Real, human imperfections: initials, "et al.", abbreviated journal (rule or initials: NEJM), no issue/pages, dropped subtitle, one typo, lower case | verified (plausible acceptable) |
| Year off by one (online-first vs print, preprint vs version of record) | verified or plausible |
| Year off by two or more | mismatch (plausible at most) |
| Wrong journal, for a journal article | plausible (capped 0.8, #80) |
| Wrong volume or first page | plausible (capped 0.8) |
| Title and year only, no authors or ID | plausible at most (0.8) |
| Year/journal alone, or a 1–2-word title in free text | not_found |
| Journal + volume + first page, no title (running header) | verified (0.9; 0.8 a year off) |
| Fields missing (no year, no journal) | verified when title and authors agree |
| Real title, wrong authors | mismatch or plausible, never verified |
| First author right, listed co-authors invented | not specified: DESIGN makes the first author alone a match (for "et al."). This red team counts listed-but-wrong co-authors as a wrong citation: plausible |
| Real title or real place, the rest from another work (chimera) | mismatch or not_found |
| Claimed DOI/PMID/arXiv ID is another work's | mismatch |
| Well-formed ID that doesn't exist, details real | mismatch (the right work suggested) |
| ID that doesn't exist, details invented; wholly invented work | not_found |
| Retracted, cited plainly | verified or plausible, **with a retraction note** (`check`: error) |
| Preprint cited as a journal article (journal, volume, page) | plausible |
| Journal article cited as a preprint (medRxiv/arXiv) | not specified; counted here as a wrong venue: plausible (policy-soft) |
| Reply, erratum, comment carrying the title, not cited as one | not the original (confidence halved) |
| Erratum, editorial, letter cited as such | verified |
| Another edition of a cited book | plausible |

## Corpus (`corpus.jsonl`, 322 entries)

Bench format (`claim` as text or a Claim object, `expect`, `why`, optional
`ids`/`retracted`) plus `n`, `category`, `real` and `style`.

- **Real, 155.** 62 hand-picked works (looked up by DOI on OpenAlex:
  landmark trials, consensus definitions, lab methods, epidemiology, physics,
  CS, social science; consortium and 100+ author bylines; a German 1905
  classic), 30 drawn at random from OpenAlex (`sample` with seed 20261116,
  by field: medicine, biochemistry, immunology, neuroscience, nursing,
  pharmacology, physics, CS, social science, psychology, economics, arts &
  humanities, pre-1980, 2024–26, and French/German/Spanish/Portuguese/
  Japanese titles), 23 more of particular types from the same samples
  (preprints, datasets, books, chapters, errata, editorials, letters), and
  40 written by hand (NeurIPS/ICML/ICLR/CVPR/ICCV/KDD papers, arXiv-only
  papers, 15 books, a PhysioNet dataset). Written as a person would, in
  Vancouver, AMA, APA, Harvard, Nature, IEEE, BibTeX-ish, prose ("Huang et
  al. found in The Lancet (2020) — …") and structured claims; 21 carry one
  imperfection (see `why`). About half are medicine/life sciences.
- **Fabricated or wrong, 152**, derived from the real pool unless noted:
  a. invented (15, hand-written) · b. real title, invented authors (14) and
  first author right, co-authors invented (8) · c. year 2–6 off (12), wrong
  journal with and without a locator (14), wrong volume or pages (11) ·
  d. chimera: one work's title, another's authors/journal/year/locator (12)
  · e. one work's details with another's DOI (12) · f. well-formed IDs that
  don't exist (12) · h. preprint cited in a journal, and journal articles
  cited as preprints (12) · i. LLM-style hallucinations for 30 topics
  (sepsis fluids, transformer interpretability, long COVID, SGLT2, …).
- **Retracted, 15** (g), cited without saying so: must come with a
  retraction flag.

Every `a`/`i` title was checked against Crossref's bibliographic search
(no work with that or a near-identical title), and every identifier in `f`
against doi.org's handle API, Europe PMC and arXiv (none exist), on
2026-11-16. One `i` entry deliberately carries a real arXiv ID of an
unrelated paper.

## Running it

```sh
cargo build --release -p aspero
OPENALEX_API_KEY=… ASPERO_MAILTO=… scripts/redteam.py --failures
scripts/redteam.py --rescore --failures      # re-score the saved results
```

Claims go to `aspero verify - --format json` in one batch through
`target/redteam-cache`, with OpenAlex capped at $0.75/day unless
`ASPERO_OPENALEX_CAP_USD` says otherwise. The first run cost $0.30 of
OpenAlex searches; re-runs within the cache TTLs are free. It prints the
acceptance criteria (each PASS or FAIL) after the table. Exit 1 if any
fabricated or wrong citation verified.

## Results (2026-11-16, aspero 0.26.0)

| | |
|---|---|
| **False VERIFIED among fabricated/wrong** | **31/152 (20.4%)** |
| Retracted works accepted without a retraction flag | 0/15 (all 15 verified, all flagged) |
| Real-citation recall (verified or plausible) | 147/155 (94.8%); 143 verified |
| Real citations accepted as another work | 8 (3 of them wrong in substance) |

```
category              verified  plausible   mismatch  not_found     n
real:book                   14                     1          2    17
real:chapter                 2                                      2
real:conference             13          1                     3    17
real:consortium             13          2                          15
real:dataset                 5                     1                6
real:editorial               3                                      3
real:erratum                 2          1                           3
real:exact                  35                                     35
real:imperfect              21                                     21
real:letter                  2                                      2
real:long-authors            1                                      1
real:non-english             7                                      7
real:preprint                9                                      9
real:sampled                16                     1               17
a:invented                                                   15    15
b:wrong-authors                         6          7          1    14
b:wrong-coauthors            8                                      8
c:wrong-journal              8          5                     1    14
c:wrong-locator              3          8                          11
c:wrong-year                 1          3          8               12
d:chimera                    6          1                     5    12
e:doi-swap                                        12               12
f:fake-id                                          6          6    12
g:retracted                 15                                     15
h:preprint-swap              5          4                     3    12
i:llm-hallucination                     1          5         24    30
```

Wholly invented works (a, i), swapped DOIs (e) and nonexistent identifiers
(f) are caught every time. What gets through is a *real* work cited wrongly.

**Adversarial round**: 58 citations crafted the way an LLM misremembers
(well-known medical and ML papers, initials only), 48 verified:
title extended or subtitled (12/12, structured 4/5), invented co-authors
(5/5), a year in the title masking the wrong year (7/7), year one off plus
wrong journal (3/3), wrong journal (8/10), right authors and locator with
another title (5/6), wrong numbers in unread styles (3/6), lookalike copies
(1/4). The other 10 include deliberate controls showing where each rule
does hold.

## Results after the fixes (2026-11-16, unreleased)

Each failure pattern below got a rule (see DESIGN, "Resolve, verify and
check", and CHANGELOG), tested offline first (`red_team_minimised_failures_stay_fixed`
and one test per rule in `crates/core/src/matching/tests.rs`).
No corpus label was changed. `scripts/redteam.py` now prints the agreed
acceptance criteria:

```
PASS  wrong -> verified                                0/152 (0.0%)  <= 2%
PASS  invented, fake ID, DOI swap -> verified           0/69 (0.0%)  <= 0%
PASS  real -> mismatch or not_found                    3/155 (1.9%)  <= 3%
PASS  real -> verified                              147/155 (94.8%)  >= 85%
PASS  real -> verified or plausible                 152/155 (98.1%)  >= 95%
PASS  wrong -> plausible names a differing field      69/71 (97.2%)  >= 80%
PASS  retracted flagged                              15/15 (100.0%)  >= 100%
```

```
category              verified  plausible   mismatch  not_found     n
real:book                   14          1                     2    17
real:conference             15          1                     1    17
real:consortium             14          1                          15
real:dataset                 5          1                           6
real:erratum                 2          1                           3
real (other 9 kinds)        97                                     97
a:invented                                         1         14    15
b:wrong-authors                         6          7          1    14
b:wrong-coauthors                       8                           8
c:wrong-journal                        14                          14
c:wrong-locator                        11                          11
c:wrong-year                           10          2               12
d:chimera                              10                     2    12
e:doi-swap                                        12               12
f:fake-id                                          6          6    12
g:retracted                 15                                     15
h:preprint-swap                        11                     1    12
i:llm-hallucination                     1          1         28    30
```

Real citations accepted as another work: 3 (from 8). A wrong year with
the work's whole title, authors, volume and pages is plausible, the year
named (10 of the 12 `c:wrong-year`).

**Adversarial round**: 1 of the 48 recorded false VERIFIEDs still
verifies: `Kahneman D. Thinking, Fast and Slow: The Psychology of
Intuitive Judgment.` (an invented subtitle on a book whose record has
none; book records often leave real subtitles out, so a subtitle added
to a book's title is tolerated). Real misses left: SHAP (a NeurIPS paper
without a DOI), Said's *Orientalism* and Cohen's *Statistical Power
Analysis* (books no searched index returned).

## Failures (`failures.jsonl`)

Every false VERIFIED from the corpus (31) and the adversarial round (48),
each with the original claim, a minimal reproduction (`minimal`, re-run
live: `minimal_result`), the matched record, the per-field checks, the
pattern, a diagnosis and the rule that let it through (`rule`, file:line at
0.26.0); and the 8 real citations missed and 3 accepted as another work.
The patterns, most likely in LLM output first:

| Pattern | Example (minimal) | Rule |
|---|---|---|
| wrong-coauthors | `Tuyls K, Rourke-Ellison D. Evolutionary game theory and multi-agent reinforcement learning. 2005.` | `matching/text.rs:158-172`, `authors.rs:65-76` |
| wrong-journal-unread | `Huang C, et al. Clinical features of patients infected with 2019 novel coronavirus in Wuhan, China. JAMA. 2020.` | `publication.rs:335-338` |
| two-errors | `Hochreiter S, Schmidhuber J. Long short-term memory. Neural Netw. 1998.` | `matching/mod.rs:446-449` + the above |
| title-extension | `LeCun Y, Bengio Y, Hinton G. Deep learning for medical image analysis. Nature. 2015;521:436-444.` | `matching/text.rs:129-137` |
| structured-subtitle | `{"title": "Deep learning: a primer for clinicians", "authors": ["LeCun, Y."], …}` | `matching/text.rs:49-56` |
| year-in-title | `Siegel RL, Miller KD, Jemal A. Cancer statistics, 2020. CA Cancer J Clin. 2018;70(1):7-30.` | `matching/mod.rs:243-249, 436-450` |
| placed-wrong-title | `Niwa T. Quantum entanglement in migratory songbirds. J Microbiol Methods. 2005;61:251.` | `matching/mod.rs:515-520, 582-593` |
| lookalike-escape | `Tversky A, Kahneman D. Judgment under uncertainty: heuristics and biases. Science. 1974;185:1353.` (verifies a 1973 tech report) | `matching/mod.rs:497-507` |
| wrong-journal-dotted | `Witte SS. … street sex in New York City. Gastroenterology. 1999;10:162.` | `publication.rs:363-381` |
| unread-locator | `Silver, D. et al. Mastering the game of Go … Nature 530, 484 (2016).` | `publication.rs:228-320` |
| preprint-swap | `Yuan H. Cross-embodiment dexterous grasping with reinforcement learning. Science Robotics. 2024.` | `matching/mod.rs:460-464` |

No API response seen while building this (OpenAlex samples and lookups,
Crossref, doi.org, Europe PMC, arXiv) contained instruction-like text.
