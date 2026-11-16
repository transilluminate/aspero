# Wild corpus 2: references as LLMs write them

`wild-2.jsonl` holds 1,032 references copied from 100 chatbot answers,
each one checked by hand against free bibliographic sources. It is test
data. **The wrong entries are what a model wrote.** They pair real titles,
real researchers and real identifiers in combinations that don't exist.
Don't cite, index or attribute them.

## Generation

- **Models.** 100 sessions, 50 each on `claude -p --model haiku`
  (claude-haiku-5-5) and `--model sonnet` (claude-sonnet-5-5). Every
  session ran with `--tools "" --strict-mcp-config --safe-mode
  --no-session-persistence` and the system prompt "You are a helpful
  assistant.", one user turn each, on 2026-11-16.
- **Domains.** 50 sessions are medicine, nursing or allied health and 50
  cover other fields. In both halves the models alternate.
- **Prompts.** Each prompt is written the way a real user asks and pushes
  the model towards the edge of what it knows. Each belongs to one of these
  families (haiku/sonnet session counts in brackets):
  - niche subtopics (8/13): chlorhexidine bathing and CLABSI in PICUs,
    fairness metrics for clinical risk prediction, carbon pricing in small
    open economies, Byzantine iconoclasm historiography, the Maragha
    school, Viking silver hoards
  - recent work, "papers from 2022–2024 on …" (10/8)
  - many references, "give me 15" (4/5)
  - exact details, "with DOIs and page numbers" (7/10)
  - a named style: APA 7, Vancouver, Harvard, AMA, BibTeX, Chicago,
    Bluebook, ACM (9/6)
  - systematic-review asks, "list the RCTs included in …" (7/3)
  - non-English (5/5): Spanish, German, French, Portuguese and Japanese,
    two each
- **Refusals.** Three answers declined to give any references from memory:
  sessions 4 (sonnet), 27 and 61 (haiku). Most of the other answers say
  that they were written from memory, and many mark single entries "verify".
- **References extracted.** 1,116: 402 from haiku and 714 from sonnet.
  Sonnet gives more references per answer.

## Method

1. **Extraction.** A separate `claude -p --model sonnet` call with no tools
   split each answer into references. For each one it copied the raw text
   and parsed the fields. All 1,116 raw strings were confirmed to be exact
   substrings of the answers.
   - **Claims.** Each claim is the reference as written, with markdown
     emphasis stripped. When an answer put DOIs or PMIDs on their own
     sub-lines under a reference, they are appended as ` DOI: …` or
     ` PMID: …` (22 claims).
   - **Extractor errors.** An identifier or title the extractor reported
     but the answer never wrote was discarded.
2. **Lookup.** Each reference was looked up in free sources only:
   - Crossref `/works/{doi}` and `query.bibliographic`, with and without
     author and year filters
   - Europe PMC REST: DOI, PMID, phrase-title, title-plus-year and
     author-plus-year searches
   - the arXiv API: batched `id_list`, and title search with backoff on
     429s
   - DataCite: DOIs, and the title search over `arxiv.content`
   - the doi.org handle API, to tell whether a DOI exists
   - Open Library search, for books
   - Requests were paced (Crossref at about 1/s, because of its current
     public-pool limit) and every response was cached. No mailto was sent,
     because none was configured. Neither OpenAlex nor aspero was used.
3. **Comparison.** The closest record was compared field by field against
   the citation:
   - every listed author against the record's author list (accents
     folded, compound surnames joined)
   - year against every date on the record (print, online, issued)
   - journal (abbreviations and initialisms accepted)
   - volume, issue, first and last page, DOI, PMID and arXiv ID
   - title, as word edits after normalising case, punctuation,
     hyphenation and British/American spelling
4. **Manual review.** Every reference the comparison did not pass as
   correct was reviewed by hand (335, with targeted extra lookups). So was
   a random 85 of those it did pass. The audit found one error: a citation
   given only as author, year and DOI, accepted against the DOI's record
   when the DOI belonged to another paper. The rule was fixed and the
   comparison re-run. Decisions and evidence are in each line's `why`.

### Classes

| Class | Rule | Category, `expect` |
|---|---|---|
| real-correct | The work exists and every cited field matches. Human-level imperfection is allowed: initials, "et al.", an abbreviated journal or Bluebook form, a dropped subtitle, one word changed, "LLMs" or "RAG" for the spelled-out words, an ACM article number for pages, a date on the record (online or print), a reissue or another edition of a book, the v1 title of a retitled arXiv paper. Identifier-led short citations ("Christiano et al., Deep RL from Human Preferences, 1706.03741") pass when the ID's record has the cited first author and year. | `w:real-correct`, real: true, ids with prefix. Expect verified or plausible. |
| real-flawed:fields | The citation clearly refers to one real work (its title, or its authors with its venue and locator), but the listed fields are wrong. Fields: `title` (two or more words changed, or a subtitle added or rewritten), `authors` (any listed name not on the record), `year`, `journal`, `volume`, `issue`, `pages` (first page), `lastpage`, `doi`, `pmid`, `arxiv`. | `w:real-flawed:<fields>`, real: false, ids of the intended work. Expect per the policy table: a wrong ID gives mismatch. Journal, volume, pages or co-authors alone give plausible. A year one off, or a wrong issue or last page alone, gives verified or plausible. Anything else gives plausible or mismatch. |
| chimera | Parts come from two real works: one paper's title with a sibling paper's authors, year, volume or DOI, two real titles blended, or a real title credited to a researcher who isn't on it. | `w:chimera`, real: false. Expect plausible, mismatch or not_found. |
| fabricated | No real work is referred to. | `a:wild-fabricated`. Expect not_found, or mismatch if it carries a real ID. **None found.** |
| unclear | Excluded from the file. | 84 references: 24 with no title (vitamin D table rows "Aglipay 2017 \| JAMA", project descriptions); grey literature (NICE, CDC, WHO, PHE, SCoR, EPUAP and BIS documents, World Bank and ILO working papers, government reports); legislation and court judgments; law-review and humanities articles, chapters and monographs that no free source indexes. |

## Results

| | haiku | sonnet | all |
|---|---|---|---|
| references extracted | 402 | 714 | 1,116 |
| unclear (excluded) | 28 | 56 | 84 |
| judged | 374 | 658 | 1,032 |
| real-correct | 296 (79.1%) | 614 (93.3%) | 910 (88.2%) |
| real-flawed | 74 (19.8%) | 41 (6.2%) | 115 (11.1%) |
| chimera | 4 (1.1%) | 3 (0.5%) | 7 (0.7%) |
| fabricated | 0 | 0 | 0 |
| wrong beyond a soft field (not only an issue or last page) | 77 (20.6%) | 38 (5.8%) | 115 (11.1%) |

Wrong rate (flawed or chimera, as a share of judged references) by slice:

| Slice | haiku | sonnet |
|---|---|---|
| medicine/nursing/allied health | 33.3% (43/129) | 9.2% (22/238) |
| other fields | 14.3% (35/245) | 5.2% (22/420) |
| exact details asked for | 29.6% | 6.3% |
| non-English prompt | 38.2% | 4.3% |
| named style | 26.7% | 4.1% |
| niche subtopic | 19.1% | 12.4% |
| systematic-review ask | 18.2% | 6.7% |
| many references | 16.3% | 6.5% |
| recent work (2022–24) | 4.5% | 1.8% |

The fields wrong among flawed references, as a share of each model's
judged references. A reference can have several.

| Field | haiku | sonnet |
|---|---|---|
| doi | 38 (10.2%) | 14 (2.1%) |
| title | 16 (4.3%) | 8 (1.2%) |
| authors | 16 (4.3%) | 6 (0.9%) |
| volume | 11 (2.9%) | 2 (0.3%) |
| pages (first) | 10 (2.7%) | 2 (0.3%) |
| issue | 8 (2.1%) | 2 (0.3%) |
| year | 6 (1.6%) | 9 (1.4%) |
| journal | 6 (1.6%) | 0 |
| last page | 3 (0.8%) | 5 (0.8%) |
| pmid | 0 | 1 |
| arxiv | 1 | 0 |

Of the references that carry a DOI, 19.4% of haiku's (39/201) and 5.3% of
sonnet's (15/285) are wrong. Of those, 21 don't exist and the rest resolve
to another real work. By type, preprints cited by arXiv ID were wrong 1.4%
of the time, books 2.7% (many more books were unverifiable) and journal
articles 15.9%.

## Hallucination patterns

1. **Near-miss DOIs are the main failure.** They have the right prefix,
   journal and year, a few digits off, and usually point to an unrelated
   paper in the same journal issue. One example is the cited
   `10.1016/j.jen.2008.04.011` for a paper whose DOI is
   `…2008.05.003`, the cited DOI being a 2008 health-policy piece.
   Some others fail the check digit (`…80069-2` for `…80069-0`). Everything
   else about these references is usually right, so a resolver that trusts
   the DOI lands on the wrong work.
2. **Invented co-authors after correct leading authors.** One to three real
   names are replaced by plausible ones from the same field. Examples:
   "Drago L, Lo Fo Wong D" on the SHEA CLABSI 2022 update, "Lindqvist R"
   among 19 RN4CAST authors, "Lenca" for Lenzini, and "Farmer" for Vignes.
   A model sometimes credits a paper to the wrong first author
   ("Wu et al." for Hi-ToM, "Xiao et al." for MuMA-ToM).
3. **Title drift.** The main title stays and a subtitle is invented or
   rewritten:
   - Armellino: "the example of hand hygiene electronic monitors"
   - Hirth: "The effect of wind and solar power on electricity prices"
   - Gaywood: "a review of the process and lessons learned"
   - Kawaguchi: "evidence of intergenerational transmission"

   Or the study design is wrong:
   - "randomised, controlled trial" for a cluster-randomised crossover
     trial
   - "multicentre observational" for a prospective cohort
   - an added ": a randomized controlled trial"

   Whole titles get paraphrased while the authors, venue and DOI stay
   right. PARIS-2 became "High-flow oxygen therapy in children with
   bronchiolitis".
4. **Locator drift.** Title and authors are right, and the year, volume or
   pages belong to a neighbouring issue or a later paper by the same author.
   Examples: Raison, JAMA 329(23) for 330(9); Costa 1996, 46(2) for 2003,
   53(2); Joskow, AER 100 for 101.
5. **Chimeras from sibling papers.** Seven cases, each splicing two real
   works:
   - **The same group, a sibling paper.** Rayment-Jones "Project20" title +
     the Midwifery 2020 paper's author list and locator; Snaith 2008
     title + the 2015 follow-up's year, volume and co-author; Lenferink
     2017 + Zwerink 2014's Cochrane number and DOI; Simard 2012 + Cordier
     et al.'s journal and DOI.
   - **Two titles blended.** TRANX-H trial + a meta-analysis subtitle;
     the ADA 2019 pulpal-pain guideline + the 2015 prosthetic-joint report.
   - **A real title, another researcher.** A COVI-PRONE JAMA title
     credited to a researcher who is not on it.
6. **No wholly invented works.** Across 1,032 judged references, every
   wrong one points at a real work or a blend of real works. Ungrounded
   detail is the risk now, not invention.
7. **Hedges are informative but not reliable.** For haiku, 30% of the
   references it hedged were wrong ("verify", "least sure of the DOI"),
   against 17% unhedged. For sonnet it was 13% against 5%. Most wrong
   references carried no hedge.
8. **Asking for more detail costs accuracy.** Asking for DOIs, page numbers
   or a strict style raises haiku's error rate (30% and 27%). Asking for
   recent work lowers it for both models (2–5%), because answers lean on
   well-known arXiv IDs. Non-English prompts were haiku's worst slice
   (38%).
9. **Short identifier-led citations are usually right.** Entries like
   "Rafailov et al., DPO | 2305.18290 | NeurIPS 2023" matched their arXiv
   records in all but one case, where the arXiv ID belonged to an unrelated
   paper.

## Notes

- **Instruction-like text.** No API response and no generated answer
  contained instruction-like text.
- **Independence.** This round was built without reading the other red-team
  corpora or results, the matching code, the publication rules
  (`crates/core/src/publication/`), `text*`, `tagged.rs` or the resolver
  (`crates/engine/src/resolve/`).
- **Line format.** Each line also carries `model` and `session` (the
  session number, 1–100). `n` runs 1–1,032 in session order.
