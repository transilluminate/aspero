# Held-out corpus 6 (`holdout-6.jsonl`)

**The fabricated and wrong entries here are invented test data.** Every
`a:`, `e:`, `f:`, `i:` and `w:` line cites a work that doesn't exist, or a
real work with deliberately wrong details. Invented authors are unusual
hyphenated surname pairs; invented titles, DOIs, PMIDs and arXiv ids were
checked not to exist. `a:invented-book-real-author` and
`w:book-wrong-author` attach real authors' names to books they did not
write. None of it may be cited, indexed or attributed to anyone.

1,049 citations, built on 2026-11-17 without reading the matcher, the
resolver, earlier red-team corpora or results, and without running aspero:
**503 real**, **516 wrong** and **30 retracted** works cited plainly. Like
holdout-5 it mixes what people paste into a citation checker (theses, grant
applications, systematic reviews, guidelines, nursing essays, CS papers),
but the real half leans towards what round 6 tests: books and monographs
(humanities included), reprints and reissues, references written the way a
chatbot writes them, preprints, and conference papers.

Line format as in `corpus.jsonl` (see `scripts/redteam.py`): `claim` (text,
or a JSON claim on 21 lines), `category`, `expect`, `why`, `real` (on every
line; `true` on `g:` lines, whose works are real), `ids` where a work is meant (`doi:…`, `pmid:…`,
`arxiv:…`, `isbn:…`, `openlibrary:…`; for a wrong citation, the real work it
was derived from), `retracted: true` on `g:` lines, plus `n`, `domain`
(`med`: medicine, nursing, allied health, public health; `other`), `field`,
`style`, `imperfection` and `source`.

## How it was built

Scripts and caches stayed outside the repository. Sources: Crossref
(`sample=` with filters and random query words; title and container
queries for named conferences, anthologies and ESMO guidelines), Europe PMC
(random topic, year and page per query; preprints with and without a
published version; publication types Practice Guideline, Published Erratum,
Retracted Publication; StatPearls and NICE), DataCite (`random=true`, by
repository), the arXiv API (random categories and months; venue phrases in
the comment field), Open Library (subject and first-publication-year
searches, then each work's editions), and the doi.org handle API, paced at
0.25 s or slower per host (Crossref about 1/s, arXiv 3 s). No OpenAlex, no
aspero. Works cited more than ~250 times (Crossref or Europe PMC counts)
were skipped. Open Library has no citation counts: its books were limited
to 3–12 editions and low readership counts, and the list was then screened
by hand to drop famous works and
garbled records; the same for a Springer "Classics in Mathematics" reissue
sample, which was dropped entirely.

### Real (503; 245 med, 258 other)

| Category | n | Expect | What |
|---|---:|---|---|
| `real:article` | 164 | verified; verified, plausible (43) | Europe PMC sample over 150 topics (nursing, midwifery, physiotherapy, OT, speech and language therapy, dietetics, pharmacy, paramedics, public health, clinical specialties) plus a journal-family sample (PLoS One, BMJ Open, J Adv Nurs, …), and a Crossref sample across the sciences, engineering, social sciences and humanities (random query words) |
| `real:model-style` | 54 | verified, plausible | references as a chatbot writes them: "et al." after the first author, markdown italics or bold, title in quotes or first, no volume or pages, bare `arXiv:2103.01234`, "arXiv preprint arXiv:…", "arXiv preprint 2103.01234" in a list item |
| `real:conference` | 60 | verified, plausible; verified (28) | ACM and IEEE proceedings papers (Crossref; random, and targeted at CHI, UIST, KDD, CIKM, SIGIR, ICRA, IROS, ICC, ICASSP, EMBC, …), half with the DOI; and NeurIPS/ICML/ICLR/ACL/EMNLP/NAACL/CVPR/ICCV/ECCV/AAAI/IJCAI/Interspeech/MICCAI/AISTATS/UAI/COLING/WACV papers found by their arXiv comment ("Accepted at …"; workshops excluded), cited by venue and year without a DOI, a quarter with the arXiv id |
| `real:book` | 54 | verified; verified, plausible (16) | Crossref books with ISBN (humanities: history, philosophy, literature, classics; nursing, midwifery, primary care, hospice; other), sometimes with the ISBN or DOI; Open Library books cited with their first-publication year and first edition's publisher |
| `real:preprint` | 50 | verified | arXiv (25 categories, 2016–2025, no journal reference), medRxiv and bioRxiv with no published version recorded (Europe PMC), PsyArXiv and SSRN (Crossref) |
| `real:reprint` | 44 | verified, plausible | a classic book cited as a later reprint edition (Open Library edition with its ISBN): APA "(Original work published …)", Chicago "Reprint, …", Harvard "(1998 [1923])", MLA, or the reprint's year and publisher only; and journal articles reprinted in anthologies ("Critical Readings in …", "Classic Readings in …", "… Reader"), cited as the chapter, or with "Reprinted from"/"Reprinted in" |
| `real:guideline` | 15 | verified; verified, plausible (6) | practice guidelines (Europe PMC publication type) and NICE guidance on NCBI Bookshelf (corporate author) |
| `real:chapter` | 14 | verified | book chapters (Crossref) |
| `real:cochrane` | 14 | verified; verified, plausible (2) | Cochrane reviews, each cited as the version it is |
| `real:non-english` | 12 | verified | Portuguese, Spanish, French and German titles (Crossref language field) |
| `real:grey` | 8 | verified, plausible | StatPearls chapters |
| `real:dataset` | 8 | verified | DataCite datasets (Dryad, Zenodo, figshare, ICPSR, UK Data Service, EBAS, PANGAEA, …) |
| `real:erratum` | 6 | verified | correction notices cited as corrections |

Styles: APA, Vancouver/NLM, AMA, Harvard, Chicago, MLA, IEEE, ACM, ACL, Nature, BibTeX, JSON and model style (109 lines across the corpus). 151 real claims carry an imperfection named in `imperfection` and `why`: one ordinary human imperfection (lower-case or Title Case title, title typo, no issue or pages, first author's surname misspelt, first author then "et al.", abbreviated journal in a full-name style, dropped subtitle, dropped diacritics, online-first year), model formatting, or a reprint/anthology form; these allow `plausible`. `plausible` is also allowed for a stated reason on Open Library books (first-publication years), StatPearls and NICE (their year updates), and arXiv-hosted conference papers (the arXiv year can differ from the conference year).

### Wrong (516; 251 med, 265 other) and retracted (30)

| Category | n | Expect | What |
|---|---:|---|---|
| `w:sister-journal` | 22 | plausible | real article in a sister or lookalike journal (PLoS One→PLoS Medicine, BMJ Open→BMJ, J Adv Nurs→J Clin Nurs, Midwifery→Women and Birth, Phys Ther→J Physiother, Teaching and Teacher Education→Journal of Teacher Education, …), no DOI |
| `w:coauthor-replaced` | 20 | plausible | one co-author replaced by an invented name; first author right |
| `w:coauthor-added` | 15 | plausible | one invented co-author added; first author right |
| `w:year-plus1` | 12 | verified, plausible | year one later (never the record's print or online year) |
| `w:year-minus1` | 12 | verified, plausible | year one earlier (likewise) |
| `w:year-off2` | 20 | mismatch, plausible | year two or three off (articles; proceedings papers with the proceedings title's year changed too) |
| `w:volume-off` | 15 | plausible | volume off by 1–3, no DOI |
| `w:page-off` | 15 | plausible | page range shifted, no DOI |
| `w:title-word-changed` | 18 | plausible, mismatch | one title word changed in meaning (children→adolescents, women→men, qualitative→quantitative, Northern→Southern, a country), no DOI |
| `w:title-synonym` | 18 | plausible, mismatch | one title word replaced by a synonym (study→investigation, association→relationship, …), no DOI |
| `w:preprint-as-journal` | 15 | plausible; plausible, verified (4) | medRxiv/bioRxiv preprint's DOI and title with the journal version's venue, volume, pages and year; 4 arXiv papers cited with their journal reference (`verified` also allowed: **ambiguous** where the titles match) |
| `w:journal-as-preprint` | 8 | plausible | the journal version's title (the preprint is titled differently) cited as the medRxiv/bioRxiv preprint with the preprint's year, no DOI; policy-soft |
| `w:erratum-as-original` | 10 | mismatch, plausible | original article with its correction notice's DOI, or with the notice's year/volume/page |
| `w:original-as-erratum` | 8 | mismatch, plausible; mismatch, plausible, not_found (1) | the notice's title ("Correction to: …") with the original's authors and locator |
| `w:chapter-as-article` | 8 | plausible, mismatch | chapter cited as an article in a "journal" named after the book, invented volume; **ambiguous** |
| `w:article-as-chapter` | 8 | plausible, mismatch | article cited as a chapter in a book named after the journal, invented editor, a real publisher; **ambiguous** |
| `w:wrong-conference` | 22 | plausible | ACM/IEEE paper at a sister conference in the same year (CHI→UIST, KDD→CIKM, RecSys→SIGIR, ICRA→IROS, ICC→GLOBECOM, ICASSP→ICIP, EMBC→BIBM, …), no DOI; arXiv-hosted papers at a related venue (AAAI→IJCAI, WACV→CVPR, ACL→EMNLP, NeurIPS→ICML, …) |
| `w:cochrane-old-version` | 8 | plausible, mismatch | superseded Cochrane version's authors with the current version's year (and sometimes DOI), or the current version's authors and year with the old version's DOI |
| `w:guideline-old-edition` | 6 | plausible, mismatch | older edition of a same-titled ESMO guideline with the newest edition's year, no volume or pages |
| `w:chimera` | 20 | mismatch, not_found | one work's title, another's authors, journal, year and locator |
| `w:real-title-wrong-authors` | 18 | mismatch, plausible | real title, journal, year and locator; every author invented |
| `w:book-edition-year` | 12 | plausible, mismatch | a later edition's year presented as the first: "(Original work published 1962)" for a 1903 book, "(1st ed.)" on a later edition, "First published 1967" |
| `w:book-wrong-publisher` | 12 | plausible, mismatch | real Crossref book with another plausible publisher (Routledge→Palgrave Macmillan, OUP→CUP, Springer→Elsevier, …), no ISBN or DOI |
| `w:book-wrong-author` | 12 | mismatch, plausible | real book title, year and publisher attributed to the (real) author of another book in the sample |
| `e:doi-other-paper` | 20 | mismatch | real article with another real paper's DOI (same prefix where possible) |
| `e:doi-one-char-off` | 8 | mismatch | DOI with one digit changed that resolves to another work |
| `f:doi-one-char-off` | 10 | mismatch | DOI with one digit changed that does not resolve |
| `e:pmid-other-paper` | 10 | mismatch | two adjacent PMID digits transposed: another paper |
| `f:nonexistent-pmid` | 3 | mismatch | two PMID digits transposed: no such PMID |
| `f:fake-doi-real-details` | 8 | mismatch | real details with a well-formed DOI of the right prefix that does not exist |
| `f:nonexistent-arxiv` | 12 | mismatch | real arXiv paper with an id beyond that month's range (`2508.38414`), some in model style |
| `a:invented` | 16 | not_found | invented studies (nursing and allied-health trials, ML, ecology, development economics, materials, archaeology, education) in a fitting real journal |
| `a:invented-fake-doi` | 12 | not_found, mismatch | the same with a nonexistent DOI in the journal's pattern |
| `a:invented-book-real-author` | 10 | not_found, mismatch | an invented book attributed to a real author of a Crossref book, with that author's publisher |
| `i:llm-hallucination` | 18 | not_found | LLM-style review titles ("Exploring the role of … : a systematic review", "Harnessing … : an integrative review") on real topics, real journals; some in model style |
| `i:llm-hallucination-real-doi` | 15 | mismatch, not_found | the same, with an unrelated real paper's DOI |
| `i:llm-hallucination-fake-doi` | 6 | not_found, mismatch | the same, with a nonexistent DOI in the journal's pattern |
| `i:llm-hallucination-book` | 10 | not_found | invented textbooks from real publishers |
| `i:llm-hallucination-conference` | 10 | not_found | invented papers at real venues (NeurIPS, ICML, ICLR, CVPR, ACL, EMNLP, CHI, ICRA, AAAI, KDD) |
| `i:llm-hallucination-arxiv` | 8 | not_found, mismatch | invented arXiv papers with an id beyond the month's range, model style |
| `i:llm-hallucination-real-arxiv` | 6 | mismatch, not_found | invented arXiv papers with an unrelated paper's real arXiv id, model style |
| `g:retracted` | 30 | verified, plausible, flagged | retracted articles (Europe PMC publication type, Crossref retraction notices) cited plainly, "RETRACTED:"/"Notice of Retraction" prefixes removed |

Checks: every invented title was searched with Crossref's bibliographic
query (books also with Open Library's title search) and dropped when a work
with a near-identical title existed (word-overlap 0.6 or more, or the title
contained in a found one); every invented or altered DOI was checked with
the doi.org handle API, every altered PMID against Europe PMC, every
altered arXiv id against the arXiv API (none exist; the `e:` ones resolve to
another work, its title checked to differ). Claims are unique. 100 lines
(two random samples of 50) were checked by hand against fresh source
records; the defects found there and in review (an IEEE "Notice of Retraction" title prefix, a
correction notice titled like its original, NLM journal-title suffixes, a
workshop paper read as a main-conference one) were fixed in the generator
and the corpus rebuilt.

## Ambiguities

- `w:year-plus1`/`minus1` allow `verified` by policy, and the four arXiv
  `w:preprint-as-journal` lines do where the titles match; they cannot count
  as false VERIFIED.
- Chapter↔article, title changes, erratum↔original, Cochrane versions,
  guideline editions, book edition years and book publishers allow two
  adjacent verdicts, as the policy doesn't decide between a wrong field and
  a wrong work. A wrong publisher and an edition year presented as the
  first are new: the policy names neither ("Another edition of a cited
  book" is plausible).
- `real:reprint` lines cite a reprint edition, often only by its year and
  publisher: whether a source holds that edition decides the verdict, so a
  miss may be a coverage gap. Anthology lines carry both DOIs in `ids`.
- `real:model-style` lines drop volume and pages and put "et al." after the
  first author by design; `w:` and `f:` lines in model style
  (`style: model`) test the same formatting on wrong citations.
- Data are cited as the sources hold them: Europe PMC's year is the
  journal-issue year (Crossref's may be the online year), and some records
  carry publisher quirks (capitalised surnames, odd short journal titles,
  Open Library first-publication years that are an early reprint's).
- Several sister journals are publisher siblings or lookalike titles
  (Midwifery/Women and Birth, Quantum Information Processing/Quantum
  Reports) rather than one society's family.

No response seen while building this (Crossref, Europe PMC, DataCite,
arXiv, Open Library, doi.org) contained instruction-like text.
