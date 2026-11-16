# Held-out round 4 (`holdout-4.jsonl`)

No longer held out: its failures shaped round 5 (see
[README.md](README.md#round-5-the-fourth-held-out-corpus-and-the-first-wild-one-2026-11-16-unreleased)),
so a later round needs a new one.

**The fabricated and wrong entries here are invented test data.** Every
`a:` and `i:` entry cites a work that does not exist, under invented author
names; every `w:` and `e:` entry is a real work cited with a deliberate
error (an invented co-author, a changed word, another work's DOI, ...).
None of them may be cited, indexed or attributed to anyone.

This round measures how close to the floor `verify` is: most wrong entries
are the *subtle* errors a language model makes on real works, not invented
papers. It was built without looking at aspero's matching code, its
earlier corpora (`corpus.jsonl`, `failures.jsonl`, holdout rounds 1-3) or
their results, and without running aspero.

## Line format

As `corpus.jsonl`: `n`, `category`, `real`, `domain`, `style`, `claim`
(reference text, or a Claim object with `title`, `authors`, `year`,
`journal`, `volume`, `pages`, `doi`), `expect` (allowed verdicts), `why`,
optional `ids` (the real work(s) behind the entry) and `retracted`.
Category prefixes keep `scripts/redteam.py`'s scoring: `real-*` real,
`g:` retracted, `a:`/`i:` invented and `e:` identifier swaps (must never
verify), `w:` other wrong citations.

```sh
scripts/redteam.py bench/redteam/holdout-4.jsonl --failures
```

## How it was built (2026-11-16)

Only free sources: Crossref (`sample`, filters, bibliographic search),
Europe PMC, DataCite, Open Library and doi.org's handle API, paced at 0.25 s
or more between requests. No OpenAlex, no aspero runs. The contact address
went in query strings only and is in no file.

**Real (569).** Drawn at random, not hand-picked:

- medicine, nursing and public health (285, 50%): Europe PMC searches on
  random condition, nursing and public-health terms × random year
  (1992–2025) × random page, each hit's metadata taken from Crossref;
  Crossref samples from 50+ major journals (JAMA family, Lancet family,
  BMJ family, NEJM, nursing journals, ...); practice guidelines (Europe PMC
  `PUB_TYPE:"practice guideline"`); medRxiv/Research Square preprints;
- every other field: Crossref samples by ISSN from physics, chemistry,
  biology, earth science, maths, CS, engineering, economics, sociology,
  psychology, education and humanities journals;
- conference papers: Crossref samples of ACM and IEEE proceedings and AAAI
  (AAAI, ICWSM, ICAPS, HCOMP, SoCS, AIIDE), plus 8 USENIX papers written by
  hand (no DOIs);
- book chapters (Crossref sample, Springer/Routledge/CRC/Brill/OUP,
  including LNCS-style proceedings chapters), books (Crossref monographs
  and Open Library, single-edition where possible), reports and working
  papers (NBER, World Bank, RAND, OSTI, DTIC, NIHR), datasets (DataCite:
  Dryad, Zenodo, PANGAEA, ICPSR, UK Data Service, figshare), arXiv
  preprints (DataCite `10.48550`), journal versions of preprints,
  correction notices cited as such, and German, Portuguese, Spanish and
  French titles (non-English journals sampled by ISSN).

Written in Vancouver, AMA, APA, Harvard, Nature, IEEE, ACM, BibTeX, prose
and structured (JSON) form. 153 (27%) carry one or two human
imperfections, all in category `real-imperfect` with the kind in `why`:
first author + et al., three authors + et al., abbreviated journal, no
issue or pages, dropped subtitle, one transposed-letter typo, lower-case
title, diacritics dropped. These, and correct citations giving the print
year of an online-first article, accept verified or plausible.

**Wrong (481) and retracted (26)**, about 58% medicine/nursing/public
health. Each derived entry uses a different real work from the real set's.

| Category | n | Expect | What |
|---|---|---|---|
| `w:one-coauthor-replaced` | 32 | plausible, mismatch | one middle co-author of 4–6 replaced by an invented name, all authors listed |
| `w:coauthor-initials-swapped` | 26 | plausible, verified | given names of two co-authors swapped (ambiguous: initials errors are a listed human imperfection) |
| `w:year-off-by-one` | 30 | verified, plausible | year ±1 from every date of the work (policy) |
| `w:year-off-by-two` | 32 | mismatch, plausible | year ±2 from every date (print, issued, online-first within a year) |
| `w:sister-journal` | 36 | plausible | same publisher, similar name (JAMA ↔ JAMA Intern Med/Netw Open, Lancet ↔ Lancet Oncol/Infect Dis/Glob Health, Nature ↔ Nat Commun, BMJ ↔ BMJ Open, NEJM ↔ NEJM Evid, Circulation ↔ Circ Res, PRB ↔ PRL, ...), 70% with the real locator, 30% with the right DOI |
| `w:volume-off` | 22 | plausible | volume ±1 or ±2 |
| `w:first-page-off` | 22 | plausible | page range shifted by 1–10 |
| `w:conference-same-society` | 30 | plausible, mismatch | the same year at a sister venue of the same society (CHI→CSCW, ICRA→IROS, CVPR→ICCV, ICASSP→ICIP, SIGCSE→ITiCSE, ICAPS→AAAI, ISWC→ESWC, Raft at OSDI 14, ...) |
| `w:chapter-as-article` | 21 | plausible, mismatch | book chapter cited as an article in a topical real journal with an invented volume/issue |
| `w:article-as-chapter` | 12 | plausible, mismatch | article cited as a chapter of an invented edited book (invented editors) |
| `w:preprint-as-journal` | 25 | plausible, mismatch | preprint title (differing from the published title) with the journal's details; or the preprint DOI with the journal's name, year and locator; or an arXiv ID with the journal's name and year where the titles differ |
| `w:title-one-word-changed` | 40 | plausible, mismatch | one key word swapped (children→adolescents, nurses→physicians, female→male, ...); the altered title checked absent on Crossref |
| `w:erratum-cited-as-original` | 22 | mismatch, plausible | the article's authors and title with the correction notice's DOI and locator (or its locator without a DOI) |
| `e:doi-with-another-works-title` | 26 | mismatch, not_found | authors, journal, year and right DOI of one work, the title of another real work in the same field |
| `e:doi-one-character-off` | 30 | mismatch | the DOI's last or second-last digit changed so that it resolves to another real work (checked on Crossref) |
| `g:retracted-cited-plainly` | 26 | verified, plausible + flag | retracted works (Crossref `update-type:retraction`), cited without the "RETRACTED" prefix |
| `i:llm-hallucination` | 42 | not_found | plausible titles on real topics in real venues (sepsis, SGLT2, long COVID, nurse staffing, LLM privacy, ...), invented authors, some with DOIs that don't exist |
| `a:invented-work` | 33 | not_found | invented articles, books, chapters, reports, datasets, conference papers and preprints, some with unregistered identifiers |

Every `a:` and `i:` title was checked against Crossref's bibliographic
search (no work with that or a ≥85%-similar title) and every invented DOI
and arXiv ID against doi.org's handle API (not registered); one candidate
DOI that turned out to exist was dropped. Invented names are syllable
compounds (e.g. "Brinquist", "Thalmarsh-Velwick"), never real researchers.

## Ambiguities

- Initials swapped between co-authors: the surnames are all right, so
  `verified` is allowed beside `plausible`.
- A year one off, a chapter or article cited in the wrong container, a
  wrong conference: `mismatch` is allowed next to the policy's `plausible`.
- USENIX papers have no DOIs: real ones accept `plausible`, wrong-venue
  ones also accept `not_found`.
- Records are as the sources hold them: some old Crossref records have
  upper-case names or odd page fields, book reviews appear under their
  book's title, and Open Library's first-publication year may not be the
  edition cited.
