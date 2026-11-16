# Held-out round 1 (`holdout-1.jsonl`, 760 citations)

Used in round 2 of the fixes ([README.md](README.md#round-2-the-held-out-corpus-2026-11-16-unreleased)):
no longer held out.

A fresh corpus for judging the current round of matching fixes (see
[docs/accuracy.md](../../docs/accuracy.md#method)). It was written without
looking at `corpus.jsonl`, `failures.jsonl` or the matching code, and
without running `aspero verify`: the expected verdicts come from the policy
table in [README.md](README.md#verdict-policy) alone.

**The fabricated entries are invented test data.** Categories `a`–`f`, `h`
and `i` cite works that don't exist, or real works with deliberately wrong
details. Invented works carry invented author names (rare or hyphenated
surnames, or a common surname with initials only, never an identifiable
researcher's byline) and must not be cited, indexed or attributed to
anyone.

## Format

One JSON object per line, as `scripts/redteam.py` reads it:
`n`, `category`, `real`, `domain`, `lang` (non-English only), `style`,
`imperfection` (real citations with one), `claim` (text, or a claim object
with the keys `aspero verify` accepts), `expect`, `why`, `ids` (the right
work's identifiers, as `doi:`/`pmid:`/`arxiv:`; for a wrong citation, the
identifier it claims) and `retracted`. Run it with
`scripts/redteam.py bench/redteam/holdout-1.jsonl`.

## How it was built (2026-11-16)

Real works came from free sources only: random Crossref samples
(`sample=` per journal ISSN and year range, ~100 journals across
medicine, life sciences, physics, chemistry, CS proceedings, social
sciences, humanities and non-English journals, plus books, chapters,
theses and datasets), Europe PMC (errata, guidelines, trials, letters,
editorials, reviews, consortium bylines, case reports, 1950s MEDLINE, and
non-English papers, across four year windows), the arXiv API, Open
Library (books), and doi.org content negotiation (DataCite datasets).
Each was written out as a careful person would, in the style its field
uses; 30% carry one ordinary imperfection (lower-case or typo'd title,
dropped subtitle, missing issue or pages, shortened page range,
abbreviated journal, "et al.") or were copied from a PDF (a word broken
across a line, a ligature, a doubled space). Those expect `verified` or
`plausible`; the rest `verified`.

Wrong citations were derived from *other* real works in the same samples
(none is also used as a real citation), or written by hand:

- invented titles (`a`, `i`, and invented `f`) were checked against
  Crossref's bibliographic search: the nearest title is recorded in `why`
  (Jaccard ≤ 0.62, none the same work);
- every nonexistent identifier in `f` was checked: DOIs against the
  doi.org handle API (not found), PMIDs against Europe PMC (no record),
  arXiv IDs against the arXiv API;
- retracted works (`g`) are retraction targets from Crossref
  (`filter=update-type:retraction`), plus the Wakefield 1998 Lancet paper,
  cited without the "RETRACTED" prefix;
- preprint swaps (`h`) use preprints with no journal version on record
  (cited in a field-appropriate journal with a plausible invented volume
  and pages) and journal articles with no preprint on record (cited as a
  medRxiv/bioRxiv/arXiv/SSRN preprint, respecting when each server
  existed).

Requests were paced at ≥ 0.25 s (3 s for arXiv). No response contained
instruction-like text. OpenAlex was not used.

## Counts

| Category | n | expect |
|---|---:|---|
| real (all types) | 382 | `verified` (exact) or `verified`/`plausible` (imperfect, PDF, print/online year differ) |
| a: invented work | 35 | `not_found` |
| b: real title, invented authors | 30 | `mismatch`/`plausible` |
| b: real authors + invented co-author(s) | 26 | `plausible`/`mismatch` (policy unspecified) |
| c: year off by one | 12 | `verified`/`plausible` (policy) |
| c: year off by 2–10 | 24 | `mismatch`/`plausible` |
| c: wrong journal (half with locator) | 30 | `plausible` |
| c: wrong volume or first page | 30 | `plausible` |
| d: chimera | 30 | `mismatch`/`not_found` |
| e: another work's DOI (22) or PMID (8) | 30 | `mismatch` |
| f: nonexistent ID, real details (18) / invented details (12) | 30 | `mismatch` / `not_found` |
| g: retracted, cited plainly | 27 | `verified`/`plausible` + retraction flag |
| h: preprint as journal (16) / journal as preprint (12) | 28 | `plausible` / `plausible`/`verified` (policy-soft) |
| i: LLM-style hallucination | 46 | `not_found` (`mismatch` allowed where a real volume/page or real ID is given) |
| **wrong total** | **378** (351 excluding retracted) | |

Real citations by type: journal articles 179, non-English 37, books 30,
conference papers 28, preprints 28 (arXiv, bioRxiv, medRxiv, SSRN,
Research Square), datasets 13, reviews 10, consortium bylines 9, theses 8,
trials 6, chapters 5, errata 5 (cited as errata), editorials 5, letters 5,
guidelines 4, journal + volume + page only 5, title + year only 5
(`plausible`). Years 1859–2026 (before 1950: about 30; 2020s: 82).

| Domain | real | wrong |
|---|---:|---:|
| medicine | 161 | 163 |
| life sciences | 43 | 43 |
| physics | 39 | 38 |
| chemistry | 22 | 23 |
| computer science | 53 | 48 |
| social sciences | 38 | 43 |
| humanities | 26 | 20 |

Medicine and life sciences are 53% of the real and 54% of the wrong
citations. Non-English (`lang`): German 23 real / 9 wrong, Spanish 8 / 8,
Portuguese 7 / 5, French 5 / 3, Italian 2, Japanese 1.

| Style | real | wrong |
|---|---:|---:|
| Vancouver | 56 | 70 |
| APA | 53 | 74 |
| structured claim | 43 | 49 |
| BibTeX-like | 37 | 30 |
| Nature | 35 | 45 |
| Harvard | 32 | 16 |
| Chicago | 29 | 20 |
| AMA | 23 | 24 |
| IEEE | 19 | 29 |
| prose | 19 | 3 |
| copied from a PDF | 14 | 13 |
| MLA | 13 | 5 |
| running header / title + year | 9 | 0 |

## Where the policy is soft

`why` says so on each case; the scorer may want to report these apart:

- **c:wrong-year-1** — the policy accepts a year off by one (`verified`),
  so these count as false VERIFIED in `redteam.py` while being "as
  expected". All are pre-2005 print-only articles, where the year is
  simply wrong.
- **b:wrong-coauthors** — DESIGN lets the first author alone match; the
  red team counts listed invented co-authors as wrong.
- **h** journal-as-preprint — not specified by the policy.
- **i** with a real journal, volume and pages — the locator may land on an
  unrelated real article (a placed chimera), so `mismatch` is allowed.
  Two hallucinations deliberately carry a real DOI / arXiv ID of an
  unrelated paper.
- Real hard cases: Freud's *Traumdeutung* dated 1900 (issued 1899);
  MapReduce and XGBoost with their full titles where Crossref has only the
  part before the colon; NIPS "Generative adversarial nets" vs the arXiv
  "Networks"; MEDLINE's bracketed English title for an Italian paper;
  1902 Lancet lectures recased; ZPhys bylines with "v."; preprints later
  published under the same title (both identifiers in `ids`).
