# Held-out corpus 2 (`holdout-2.jsonl`)

A second held-out round for the red team: 876 citations, 441 real and 435
wrong, written after the rules it will judge and without reference to them.
It was built without reading the tuning corpus, earlier failures or held-out
rounds, the matching code or the verification sections of the design notes;
only the verdict-policy table in `README.md`, the categories in
`docs/accuracy.md` and the line format `scripts/redteam.py` consumes.

**The fabricated entries here are invented test data.** Every entry with
`"real": false` in categories `a`, `b`, `f:fake-id-invented` and `i` carries
invented author names (syllable-built surnames such as "Quelaquist" or
"Ystrelund" that belong to no identifiable researcher), and every invented
title, DOI, PMID, arXiv ID and ISBN was checked against the sources. Real
researchers appear only on their own real papers (chimeras keep a paper's
own title and authors and borrow the rest; swaps keep the authors with their
own work). Do not cite, index or attribute any of it.

Run it with `scripts/redteam.py bench/redteam/holdout-2.jsonl`.

**No longer held out.** Its failures shaped round 3 (see `README.md`,
"Round 3"); a later round needs a new held-out corpus.

## Line format

`n`, `claim` (a string, or a claim object: `text`, `title`, `authors`,
`year`, `journal`/`container`, `volume`, `pages`/`page`, `ids`),
`category`, `expect` (acceptable verdicts), `why`, `real`, plus `style`
(the format it is written in), `domain`, `lang`; real entries carry `ids`
(`doi:`, `pmid:`, `arxiv:`) and retracted ones `"retracted": true`. String
claims are one line; the 7 `pdf-lines` claims are `{"text": …}` objects
whose text keeps the hard line breaks (and broken hyphenation) of a PDF copy.

## How it was built

All lookups were free and anonymous-friendly: Crossref (`/works` with
`sample`, filters by ISSN, type, prefix and `update-type:retraction`,
`query.bibliographic` sorted by citations for well-known works, and title
search to confirm inventions don't exist), Europe PMC (typed PubMed
searches; DOI to PMID; PMID existence), the arXiv API (well-known papers by
ID, recent submissions, date windows; existence of IDs), Open Library
(classic books; ISBN existence), DataCite (Zenodo software and GitHub
release archives, PANGAEA, Dryad and figshare datasets, university theses)
and HEAD requests to doi.org (DOI existence). Requests were paced at 0.25 s
or more (6 s for arXiv). No OpenAlex, no `aspero verify`. Fetched records
were scanned for instruction-like text; none was found. Built 2026-11-16.

Real works were drawn from about 2,300 harvested records: random samples
from ~80 journals across fields and languages (including pre-1900 volumes
of the Lancet, BMJ, Philosophical Transactions and Annalen der Physik), the
most-cited works for ~130 topics, PubMed records by publication type, and
the sources above for the other types. Junk records (mastheads, news,
"In questo numero", supplementary figures, book reviews) were excluded. Each
work is used once across the whole file. Formatting was generated from the
metadata by a script, one style per entry, and about a third of the real
entries were given human imperfections.

## Counts

| Category | n | expect | What it is |
|---|---:|---|---|
| real:clean | 298 | verified (43: verified or plausible) | Every field right. Books cited by first-publication year, footnotes with pinpoint pages, PubMed issue years of 2027 and edited volumes allow plausible |
| real:imperfect | 143 | verified or plausible (10 others, below) | Initials, first author + et al., abbreviated journal, no issue/pages, dropped subtitle, one typo, lower case, stripped diacritics, surnames in capitals, no DOI; PDF artefacts; prose; MEDLINE bracketed translated titles; running-header locators; authorless Japanese records |
| a:invented | 38 | not_found (5: or mismatch) | Invented title, authors and locator in a real journal; 11 in German, French, Spanish, Portuguese, Italian, Dutch, Chinese, Japanese, Russian, Korean |
| b:wrong-authors | 30 | mismatch or plausible | Real title, journal, year; all authors invented |
| b:extra-coauthor | 26 | plausible | Real paper; one or two invented co-authors inserted, or the real co-authors replaced (first author kept); only styles that print every author |
| c:year-1 | 20 | verified or plausible | Year off by one |
| c:year-2plus | 26 | mismatch or plausible | Year off by 2–6 |
| c:wrong-journal | 26 | plausible | Real paper placed in another journal of the same field |
| c:wrong-locator | 26 | plausible | Volume or page range shifted |
| d:chimera | 30 | mismatch or not_found | A paper's own title and authors with another real paper's journal, year, volume and pages |
| e:id-swap | 33 | mismatch | 22 DOIs (often from the same journal), 7 PMIDs, 4 arXiv IDs of other works |
| f:fake-id | 30 | mismatch (4: or plausible) | Real details with a well-formed unregistered DOI (doi.org 404), PMID (unknown to Europe PMC), arXiv ID (unassigned number in the right month) or ISBN (valid checksum, unknown to Open Library) |
| f:fake-id-invented | 7 | not_found | Invented work with an unregistered DOI or unknown PMID |
| g:retracted | 36 | verified or plausible, with a retraction note | Retracted works (Crossref retraction notices, plus Wakefield 1998, the Surgisphere papers, Séralini 2012, STAP 2014, Hwang 2004/2005, PREDIMED 2013) cited plainly |
| h:preprint-as-journal | 14 | plausible | arXiv/Research Square/PsyArXiv preprints with no journal version cited in a journal with invented issue and pages |
| h:journal-as-preprint | 11 | plausible | 2003–2012 journal articles cited as medRxiv/bioRxiv preprints (the servers did not exist yet) |
| i:llm-hallucination | 38 | not_found (6: or mismatch; 9: mismatch) | Confident LLM-style references on real topics in real journals; 9 carry a real DOI of another work, the rest none or an unregistered one |
| j:title-synonym | 26 | plausible or mismatch | Right paper, two or three title words swapped for synonyms ("Survey"→"Review", "Detection"→"Identification") |
| k:wrong-venue | 18 | plausible (4: mismatch or plausible) | Real conference papers moved to another conference (CVPR↔ICCV, KDD→ICML, NeurIPS↔ICLR/ICML…), sometimes also another year |

Real, 441: 260 journal articles (Crossref; including reviews, guidelines,
trials, case reports, editorials, letters with replies), 45 PubMed records
(6 trials, 4 guidelines, 4 letters, 4 errata, 4 systematic reviews, 3
editorials, 3 comments, 2 case reports, 7 from 1950–1995 of which 6 have no
DOI, 6 with bracketed translated titles), 43 books (classics found on Open
Library, 1858–2016, in English, German, French, Spanish, Portuguese,
Italian, Dutch, Japanese and Chinese, cited with the first edition's year
and publisher, corrected by hand where Open Library's first-publication
year is wrong; and Crossref monographs), 12 chapters, 20 conference papers,
25 arXiv preprints (famous ones and November 2026 submissions), 8 other
preprints or posted content, 8 software records on Zenodo (several are
archived GitHub releases), 8 datasets, 5 theses, 7 reports (NBER working
papers). Years 1858–2026 (also 2027 issue dates); about 30 are from before
1950.

Domains (heuristic, from journal and title): real — medicine 159, life
sciences 54 (48 %), computer science 64, physics 25, economics 25, history
16, engineering 15, mathematics 14, psychology 14, education 12, chemistry
10, law 10, other 23. Wrong — medicine 167, life sciences 53 (51 %),
physics 45, CS 41, economics 28, chemistry 27, engineering 18, psychology
16, mathematics 12, education 7, law 4, history 3, other 14.

Languages: real 65 non-English (German 9, Russian 9, Portuguese 7,
Japanese 7, French 6, Spanish 6, Chinese 6, Italian 5, Dutch 5, Korean 4,
Polish 1); wrong 46 non-English.

Formats (real / wrong): BibTeX, RIS, EndNote, Zotero-style and other
structured objects 78 / 95; Vancouver, AMA and PubMed 67 / 92; Google
Scholar "Cite" output (MLA, APA, Chicago, Harvard, Vancouver) 46 / 50; APA
including Word bibliographies (smart quotes, en dashes, no-break spaces)
36 / 48; IEEE and Nature numbered lists 30 / 47; Wikipedia `{{cite
journal}}`/`{{cite book}}` 25 / 20; footnotes (Chicago notes, Bluebook,
OSCOLA) 16 / 7; prose 16 / 10; PDF copies (ﬁ/ﬂ/ﬀ ligatures, words broken
at line ends, lost spaces, hard line breaks) 14 / 9; national styles (DIN
style, French, ABNT, GB/T 7714, SIST 02, GOST) 13 / 12; plus arXiv,
proceedings, book, chapter, dataset, software, thesis and report styles.

## Ambiguities

- **c:year-1** is wrong by construction but the policy accepts `verified`
  (online-first versus print). `redteam.py` counts any `verified` on a
  non-real line as a false accept, so leave these 20 out when reading that
  headline.
- **j:title-synonym** has no policy row. Two or three swapped words with
  everything else right is a wrong citation, so `plausible` or `mismatch`;
  `verified` would be the failure being measured.
- **h** rows follow the policy (`plausible`); the reverse swap is marked
  policy-soft there too.
- **i** and **a** entries whose title is close to a real work (Crossref
  similarity 0.70–0.79, named in `why`) also accept `mismatch` against that
  work; entries reaching 0.80 were dropped.
- **f:fake-id** with ISBNs also accepts `plausible`: an ISBN unknown to
  Open Library may just be missing from it.
- **real** books, footnotes with a pinpoint page, edited volumes listed
  under their editors and PubMed records dated 2027 accept `plausible`.
  Five real descriptions without a title ("as shown by X and colleagues in
  their 2001 N Engl J Med paper on …") accept `plausible` or `not_found`;
  five title-and-year-only entries expect `plausible` (policy: at most).
- Domain and language labels are heuristic and only for slicing the
  results.
