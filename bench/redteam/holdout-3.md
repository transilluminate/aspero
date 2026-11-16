# Held-out round 3 (`holdout-3.jsonl`, 966 entries)

No longer held out: its failures shaped round 4 (see
[README.md](README.md#round-4-the-third-held-out-corpus-2026-11-16-unreleased)),
so a later round needs a new one.

A third held-out corpus, written without looking at the earlier corpora,
their failures or results, or the matching code. It was built from the
verdict-policy table in [README.md](README.md#verdict-policy), the claim
input schema (`crates/json/src/claim.rs`) and the line format read by
`scripts/redteam.py`.

**The fabricated entries here are invented test data.** Categories `a`,
`b`, `d`, `f`, `i` and the wrong-detail categories cite works that don't
exist, or real works with deliberately wrong details. Invented authors use
made-up surnames, each checked against Crossref's author search with no
exact match, so no invented work is attached to a real person. Don't cite,
index or attribute these entries to anyone.

| | n |
|---|---|
| Real (`real`, `real:imperfect`, `real:underspecified`) | 508 |
| Wrong (`a`–`f`, `h`–`m`) | 426 |
| Retracted (`g`) | 32 |

## Line format

Same as the other corpora: `claim` (text, or a claim object using only the
fields `crates/json/src/claim.rs` accepts), `category`, `expect` (accepted
verdicts), `why`, `real`, optional `ids` (the work's own identifiers, for
real and retracted rows) and `retracted: true` on `g` rows. Extra fields:
`n`, `style` (how it's written), `domain`, `lang` (the language of the
reference style), `source` and `title_lang` (the title's language, on real
rows). Claims copied from PDFs, BibTeX and RIS keep their line breaks
(87 claims), so `redteam.py` sends them as `{"text": ...}`.

## How it was built

All sampling was done on 2026-11-16 with fixed seeds, through free APIs
only: Crossref (with a `mailto` in the query string), Europe PMC, arXiv,
Open Library, DataCite and doi.org's handle API. Requests were spaced at
least 0.25 s apart (3 s for arXiv). No OpenAlex calls, and aspero was not
run.

- **Crossref journal articles**: for each of 33 domains (12 medical
  specialties plus 21 others), 1–2 random terms from a per-domain list,
  a random offset of 0–400 and a random start year. Kept only items with
  a title, authors with family names, a journal, a year, a volume and a
  page. Dropped: single-page items (often abstracts); supplements;
  errata, replies, comments and book reviews; garbled titles; and anything
  Crossref marks `updated-by` (retractions, corrections).
- **Non-English titles**: Crossref queries written in each language,
  keeping titles in the right script or with its diacritics.
- **Proceedings**: Crossref `proceedings-article` under the ACM (10.1145)
  and IEEE (10.1109) prefixes, plus Springer LNCS chapters (ISSN
  0302-9743).
- **Book chapters**: Crossref `book-chapter`.
- **Europe PMC**: PubMed records (cited with their PMIDs), PMC-only
  records (`SRC:PMC`, cited with their PMCIDs) and preprints (`SRC:PPR`).
- **DataCite**: datasets (Zenodo, Mendeley Data, PANGAEA and others),
  software, dissertations and posters.
- **arXiv**: random offsets in 18 categories (math, stat, astro-ph, cs.RO,
  cs.HC, q-bio, physics.ao-ph, cond-mat.mtrl-sci, econ, cs.CL, …).
- **Open Library**: subject searches, each cited from one specific
  edition (publisher, year, ISBN).
- **Retracted works**: Crossref `filter=update-type:retraction` across
  25 topics, following `update-to` to the retracted article (21 from
  Retraction Watch data, 11 from the publisher). "RETRACTED:" prefixes
  are stripped from titles, and at most 5 come from one DOI prefix.

Each work is used at most once. Real works and the works that wrong
citations are built from don't overlap, except for chimeras and ID swaps,
which use two works.

**Checks.** Every invented title (`a`, `i`, `f:nonexistent-id-invented`)
was searched on Crossref (`query.bibliographic`). Any title at least 0.8
similar to an existing title was rewritten; three were. The similarity
is in each `why`, and the highest left is 0.78. Each nonexistent ID was
checked:

- DOIs: doi.org handle API says the DOI doesn't exist.
- PMIDs: Europe PMC returns no hit for `EXT_ID:… AND SRC:MED`.
- arXiv IDs: the arXiv API returns no entries.

Typo'd DOIs (`l`) were checked the same way. Four of the 26 turned out to
be real DOIs of other works, and their `why` says so. Preprints cited as
journal articles had no journal article with a similar title (≥0.75) in
Crossref. Journal articles cited as preprints had no similar
posted-content record.

**Instruction-like text.** Every fetched title, name and venue was
scanned for instruction-like text ("ignore previous", "system prompt" and
similar). None was found.

## Real citations (508)

- `real` 355 (`expect: verified`).
- `real:imperfect` 148, 29% of real (`verified` or `plausible`). Each has
  one human imperfection, named in `why`:
  - authors cut to "et al.";
  - no issue or pages;
  - a transposed-letter typo;
  - title in lower case;
  - no year;
  - abbreviated journal;
  - dropped subtitle;
  - diacritics dropped from names;
  - no issue and no DOI;
  - a messy student reference;
  - a PDF copy with line breaks, hyphenation and ﬁ/ﬂ ligatures;
  - an older edition of a book;
  - a conference paper by acronym only.
- `real:underspecified` 5: bare slide footers ("Lah et al., Crit Care
  2011") with no title or locator. The policy row "year/journal alone"
  says `not_found`, so `expect` is `not_found`/`plausible`/`unverifiable`.
  Because `real` is true, `redteam.py` counts a `not_found` here as a real
  miss. Filter with `--category real:underspecified` if that skews recall.

By source: Crossref random articles 261, Crossref non-English 66, Europe
PMC 54 (30 PubMed with PMID, 12 PMC-only, 12 preprints), Crossref
proceedings 26 (ACM 14, IEEE 12), arXiv 25, Open Library books 25,
DataCite 26 (9 datasets, 8 theses, 5 software, 4 posters), LNCS 10,
book chapters 10, bare slide footers 5.

By domain:

| Domain | n |
|---|---|
| Core medicine (12 specialties) | 200 (39%) |
| Non-English titles (6 per language, mostly health, nursing and the social sciences) | 66 |
| Robotics | 18 |
| Computer science | 17 |
| Dentistry | 15 |
| Public health | 14 |
| HCI | 13 |
| Nursing | 13 |
| Earth/climate | 12 |
| Statistics | 12 |
| Bioinformatics | 12 |
| Sociology | 11 |
| Linguistics | 11 |
| Epidemiology | 10 |
| Astronomy | 10 |
| Veterinary | 9 |
| Economics | 9 |
| Psychology | 9 |
| History | 8 |
| Pharmacology | 8 |
| Mathematics | 8 |
| Materials | 7 |
| Law | 7 |
| Philosophy | 6 |
| Education | 2 |
| Ecology | 1 |

Title languages: English 442. Spanish, Portuguese, German, French,
Italian, Chinese, Japanese, Korean, Arabic, Turkish and Polish have 6
each.

## Wrong citations (426) and retracted (32)

| Category | n | expect | What |
|---|---|---|---|
| a:invented | 35 | not_found | Hand-written mundane works: 30 articles (6 in Spanish, Portuguese, German, French, Turkish, Polish), a book, a thesis, a dataset, a CHI paper |
| b:wrong-authors | 24 | mismatch, plausible | Real title, journal, year and locator; every author invented (4 in non-English styles) |
| b:extra-coauthor | 20 | plausible, mismatch | Real authors with one invented co-author inserted (policy: plausible; mismatch allowed, see below) |
| c:year-off-by-1 | 15 | verified, plausible | ±1 year |
| c:year-off-by-2plus | 28 | mismatch, plausible | −4 to +5 years, 6 in non-English styles |
| c:wrong-journal | 20 | plausible | Another real journal from the same domain |
| c:wrong-locator | 20 | plausible | Volume shifted 1–4, or pages shifted 40–400 |
| d:chimera | 30 | mismatch, not_found | 15 with A's title and authors and B's journal, year and locator; 15 with A's title and the rest from B (years differ by ≥2) |
| e:id-swap-doi / -pmid / -arxiv | 15 / 10 / 5 | mismatch | One work's details with an unrelated work's identifier |
| f:nonexistent-doi / -pmid / -arxiv | 13 / 7 / 5 | mismatch | Real details with a well-formed ID that doesn't exist (DOI with 3 characters changed, PMID 45–50M, arXiv number ≥90000 in that month) |
| f:nonexistent-id-invented | 4 | not_found | Invented work with a nonexistent DOI under the journal's real prefix |
| g:retracted | 32 | verified, plausible + retraction flag | Cited plainly, any style |
| h:preprint-as-journal | 13 | plausible | Preprint with no journal version, cited in a plausible journal with invented volume and pages |
| h:journal-as-preprint | 12 | plausible, mismatch | Journal article (2016+) cited as a medRxiv, bioRxiv, Research Square or PsyArXiv preprint (policy-soft) |
| i:llm-hallucination | 48 | not_found (3: not_found, mismatch) | Confident titles on topical subjects in famous venues; 11 carry a nonexistent DOI, 3 a real PMID of an unrelated paper |
| j:reworded-title | 25 | plausible, mismatch | Real work with 2–4 synonym swaps in the title, the way an LLM paraphrases |
| k:conf-wrong-venue | 13 | plausible | ACM or IEEE paper cited in another conference of the same publisher |
| k:conf-wrong-year | 13 | mismatch, plausible | Year ±2–3, with the proceedings title's year changed to match |
| l:doi-typo | 26 | mismatch, plausible | Correct citation with one DOI character substituted or deleted (4 typos hit other real DOIs) |
| m:chapter-as-article | 25 | 10: plausible, verified; 15: plausible, mismatch | 10 give the book title as the journal, with an invented volume and issue; 15 give a real journal of the field |

Wrong citations by domain: core medicine 77, HCI 29, public health 23,
nursing 22, veterinary 19, epidemiology 18, psychology 18, dentistry 17,
robotics 17, mathematics 16, linguistics 15, earth/climate 15,
pharmacology 15, philosophy 14, materials 14, statistics 13,
bioinformatics 13, astronomy 13, sociology 12, law 11, economics 11,
non-English titles 10, history 7, computer science 5, neuroscience 2.

## Formats (all 966)

| Style | n |
|---|---|
| Vancouver, numbered as copied (some with PMID/PMCID) | 108 |
| Localised styles (below) | 102 |
| APA 7 with DOI URLs | 88 |
| Conference: ACM reference format, IEEE, LNCS, APA, acronym-only, BibTeX `@inproceedings` | 63 |
| Claim objects in CSL-JSON/Zotero-export shape | 55 |
| AMA | 50 |
| Wikipedia `{{cite journal}}`/`{{cite book}}` | 37 |
| arXiv: Nature, IEEE, APA, BibTeX `@misc`, bare, claim object | 35 |
| BibTeX | 34 |
| Harvard | 34 |
| Student messy | 32 |
| Nature | 29 |
| Books: APA, Chicago, Vancouver, Harvard, Wikipedia, footnote, ISBN, German, French and Spanish | 27 |
| Chicago bibliography | 25 |
| MLA | 25 |
| RIS | 24 |
| Preprint (Vancouver, APA, Nature) | 24 |
| Chicago notes | 23 |
| IEEE | 23 |
| In a sentence | 23 |
| Slide footer "Author et al., Abbr year;vol:page" | 22 (plus 5 bare) |
| PDF copy | 21 |
| Tweet or blog | 19 |
| Chapters | 10 |
| Datasets, theses, software, posters (APA `[Data set]`, DataCite, BibTeX `@phdthesis`/`@misc`) | 28 |

The localised styles:

- Spanish "y cols.", "Disponible en";
- Portuguese ABNT "v. n. p.", "Acesso em";
- German "u. a.", "In:", "S.";
- French « », "n°", "et coll.";
- Italian "e coll.";
- Chinese GB/T 7714 `[J]`, "等";
- Japanese SIST 02, "ほか";
- Korean "외";
- Arabic "المجلد، العدد، ص", "وآخرون";
- Turkish "ve ark.";
- Polish "i in.".

They are applied both to native-language titles and to English works cited
by non-English writers.

## Ambiguities

- `b:extra-coauthor`: DESIGN matches on the first author, so the policy
  calls this "not specified, counted as plausible". `mismatch` is also
  accepted.
- `c:year-off-by-1` and `c:year-off-by-2plus` take the year from
  Crossref's print date first. When the online year differs, a "+2" claim
  can sit one year from the online date.
- `h:journal-as-preprint`: the policy itself is soft. Both `plausible`
  and `mismatch` are accepted.
- `j:reworded-title`: no policy row. A paraphrased title on otherwise
  correct details sits between `plausible` (recognisably the work) and
  `mismatch`, so both are accepted.
- `l:doi-typo`: the four typos that are other works' real DOIs behave
  like ID swaps (`e`). The rest behave like nonexistent IDs (`f`). Both
  accept `mismatch` and `plausible`.
- `m:chapter-as-article`, book title as the journal: the chapter has no
  volume to contradict the invented one. `verified` is defensible, and
  `plausible` is preferred.
- Real books cited from a later edition than the first (Open Library
  `first_publish_year`) accept `plausible`.
- Some real records keep the source's quirks: given and family names
  swapped, upper-case titles, DOIs with parentheses or `..`. They are
  deliberately unedited.
