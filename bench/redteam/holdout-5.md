# Held-out corpus 5 (`holdout-5.jsonl`)

**The fabricated and wrong entries here are invented test data.** Every
`a:`, `e:`, `f:`, `i:` and `w:` line cites a work that doesn't exist, or a
real work with deliberately wrong details. Invented authors are unusual
hyphenated surname pairs; invented titles, DOIs, PMIDs and arXiv ids were
checked not to exist. None of it may be cited, indexed or attributed to
anyone.

1,232 citations, built on 2026-11-16 without reading the matcher, the
resolver, earlier red-team corpora or results: **583 real**, **613 wrong**
and **36 retracted** works cited plainly. The aim is a realistic mix of
what people paste into a citation checker (reference lists from theses,
grant applications, systematic reviews, clinical guidelines, nursing
essays and CS papers) rather than famous papers, so the score estimates
real-world performance; the wrong half also carries the subtle errors a
language model makes.

Line format as in `corpus.jsonl` (see `scripts/redteam.py`): `claim` (text,
or a JSON claim on 44 lines), `category`, `expect`, `why`, `real` (on every
line), `ids` where a work is meant (`doi:…`, `pmid:…`, `arxiv:…`; for a
wrong citation, the real work it was derived from), `retracted: true` on
`g:` lines, plus `n`, `domain` (`med`: medicine, nursing, allied health,
public health; `other`), `field`, `style`, `imperfection` and `source`.

## How it was built

Scripts and caches stayed outside the repository. Sources: Crossref (with
`sample=`, random query words and filters), Europe PMC (random topic and
year per query, a random pick from the results), DataCite (`random=true`),
the arXiv API (random offsets), Open Library (random subject offsets), and
doi.org HEAD requests, paced at 0.25 s or slower. No OpenAlex, no aspero.
Works cited more than ~250–300 times were skipped to avoid famous papers.

### Real (583; 320 med, 263 other)

| Category | n | What |
|---|---:|---|
| `real:article` | 283 | Europe PMC sample over 146 topics (nursing, midwifery, physiotherapy, OT, speech therapy, dietetics, pharmacy, paramedics, public health, clinical specialties), Crossref sample across the sciences, engineering, social sciences and humanities, and journal versions of preprints |
| `real:conference` | 75 | ACM and IEEE proceedings papers (Crossref, random and targeted at named ACM and IEEE conferences), and NeurIPS/ICML/ICLR/AAAI/ACL/EMNLP/CVPR/CHI/ICRA/KDD/USENIX/Interspeech/MICCAI/AISTATS papers found by their arXiv comment, cited by venue and year without a DOI (some with the arXiv id) |
| `real:preprint` | 43 | arXiv (20 categories), medRxiv/bioRxiv (Europe PMC), Research Square, PsyArXiv, Preprints.org |
| `real:guideline` | 33 | guideline articles (Europe PMC publication type), NICE and WHO guidelines on NCBI Bookshelf (corporate author), and the newest of several same-titled editions (ESMO, EAU, …) |
| `real:book` | 30 | Crossref books (with ISBN) and Open Library books (first-publication year; `plausible` allowed) |
| `real:non-english` | 26 | Portuguese, Spanish, French, German, Japanese and Chinese titles (Crossref; SciELO, Thieme, J-STAGE, …) |
| `real:chapter` | 25 | book chapters (Crossref) |
| `real:cochrane` | 21 | current Cochrane reviews and 8 superseded versions, each cited as the version it is |
| `real:report`, `real:dataset`, `real:software`, `real:grey` | 13, 11, 4, 9 | Crossref reports, DataCite datasets/software/reports (Zenodo, figshare, Mendeley Data, Borealis, …), StatPearls chapters |
| `real:erratum` | 10 | correction notices cited as corrections |

Styles: APA, Vancouver/NLM, Harvard (nursing-essay "Available at … (Accessed …)" included), AMA, IEEE, ACM, Chicago, MLA, Nature, BibTeX and JSON. 276 real claims carry a DOI, PMID or arXiv id. 155 (27%) carry one ordinary human imperfection, named in `imperfection` and `why` (lower-case or Title Case title, title typo, no pages, no year, first author's surname misspelt, first author then "et al.", abbreviated journal in a full-name style, dropped subtitle, dropped diacritics, online-first year): `expect` is `verified` or `plausible`. 38 more allow `plausible` for a stated reason (StatPearls and NICE update their year, Open Library gives first-publication years, an arXiv year one off the conference year).

### Wrong (613; 347 med, 266 other)

| Category | n | Expect | What |
|---|---:|---|---|
| `w:sister-journal` | 27 | plausible | real article in a sister or lookalike journal (PLoS One→PLoS Medicine, BMJ Open→BMJ, JAMA Network Open→JAMA, Journal of Nursing Management→Journal of Nursing Scholarship, …), no DOI |
| `w:coauthor-replaced` / `w:coauthor-added` | 28 / 24 | plausible | one co-author replaced by, or one added as, an invented name; first author right |
| `w:initials-reversed` | 20 | plausible, verified | first author's initials reversed (JA→AJ); **ambiguous**, as the policy treats initials as a human imperfection |
| `w:year-plus1` / `w:year-minus1` | 16 / 16 | verified, plausible | year one off (never equal to the record's print or online year) |
| `w:year-off2` | 32 | mismatch, plausible | year two or three off (26 articles, 6 proceedings papers with the proceedings year changed too) |
| `w:volume-off` / `w:page-off` | 20 / 20 | plausible | volume off by 1–3, page range shifted; no DOI |
| `w:title-word-changed` | 28 | plausible, mismatch | one title word changed in meaning (children→adolescents, acute→chronic, a country) |
| `w:title-synonym` | 24 | plausible, mismatch | one title word replaced by a synonym (outcomes→results, use→utilisation) |
| `w:preprint-as-journal` | 19 | plausible | preprint DOI and title with the journal version's venue, volume, pages and year; 4 arXiv papers cited with their journal reference (verified also allowed: **ambiguous** where the titles match) |
| `w:journal-as-preprint` | 8 | plausible | journal version's title cited as the preprint (the preprint is titled differently), no DOI; policy-soft |
| `w:erratum-as-original` | 15 | mismatch, plausible | original article with its correction notice's DOI, or with the notice's year/volume/page |
| `w:original-as-erratum` | 10 | mismatch, plausible | the notice's title ("Correction to: …") with the original's authors and locator |
| `w:chapter-as-article` / `w:article-as-chapter` | 12 / 10 | plausible, mismatch | chapter cited as an article in a "journal" named after the book (invented volume), or an article cited as a chapter in a book named after the journal (invented editor); **ambiguous** between wrong container and wrong work |
| `w:wrong-conference` | 28 | plausible | ACM/IEEE paper in a sister conference of the same society (CHI→UIST, ICRA→IROS, ICC→GLOBECOM, …), or an arXiv-hosted paper at a conference of the same organiser (EMNLP→NAACL, CVPR→ICCV) |
| `w:cochrane-old-version` | 14 | plausible, mismatch | superseded Cochrane version's authors with the current version's year (and sometimes DOI), or the current year with the old version's DOI |
| `w:guideline-old-edition` | 14 | plausible, mismatch | older edition of a same-titled guideline (ESMO, EAU, ERC, …) with the newest edition's year |
| `w:chimera` | 28 | mismatch, not_found | one work's title, another's authors, journal, year and locator |
| `w:real-title-wrong-authors` | 20 | mismatch, plausible | real title, journal, year and locator; every author invented |
| `e:doi-other-paper` | 24 | mismatch | real article with another real paper's DOI (same publisher prefix where possible) |
| `e:doi-one-char-off` / `f:doi-one-char-off` | 8 / 14 | mismatch | DOI with one digit changed: resolves to another work / does not resolve |
| `e:pmid-other-paper` / `f:nonexistent-pmid` | 13 / 1 | mismatch | two PMID digits transposed: another paper / no such PMID |
| `f:fake-doi-real-details` | 8 | mismatch | well-formed DOI of the right prefix that does not exist |
| `f:nonexistent-arxiv` | 10 | mismatch | arXiv id beyond that month's range |
| `a:invented` / `a:invented-fake-doi` | 23 / 17 | not_found (mismatch allowed with a fake DOI) | invented works (nursing, clinical, ML, economics, ecology, materials…) in a fitting real journal |
| `i:llm-hallucination` | 22 | not_found | LLM-style review titles ("Exploring the role of … : a systematic review") on real topics, real journals |
| `i:llm-hallucination-real-doi` | 21 | mismatch, not_found | the same, with an unrelated real paper's DOI |
| `i:llm-hallucination-fake-doi` | 5 | not_found, mismatch | the same, with a nonexistent DOI in the journal's pattern |
| `i:llm-hallucination-book` / `-conference` | 8 / 6 | not_found | invented textbooks from real publishers; invented papers at real venues |
| `g:retracted` | 36 | verified, plausible, flagged | retracted articles (Crossref retraction notices) cited plainly, "RETRACTED:" prefixes removed |

Checks: every invented title was searched with Crossref's bibliographic
query and dropped when a work with a near-identical title existed (20
dropped); every invented or altered DOI was checked with a doi.org HEAD
request (404), every altered PMID against Europe PMC, every altered arXiv
id against the arXiv API. Claims are unique.

## Ambiguities

- `w:initials-reversed` and the arXiv `w:preprint-as-journal` lines allow
  `verified`; `w:year-plus1`/`minus1` do by policy. They are wrong
  citations that cannot count as false VERIFIED.
- Chapter↔article, title changes, erratum↔original, Cochrane versions and
  guideline editions allow two adjacent verdicts, as the policy doesn't
  decide between a wrong field and a wrong work.
- Data are cited as the sources hold them: Europe PMC's year is the
  journal-issue year (an online-first year may differ by one), and some
  records carry publisher quirks (capitalised surnames, duplicated
  authors). Real misses on books, reports, datasets, StatPearls and NICE
  pages may be coverage gaps rather than matching errors.
- Several sister journals are publisher siblings or lookalike titles
  (Brain Sciences/Journal of Clinical Medicine, Journal of Hand
  Therapy/Hand Therapy) rather than one society's family.

No response seen while building this (Crossref, Europe PMC, DataCite,
arXiv, Open Library, doi.org) contained instruction-like text.
