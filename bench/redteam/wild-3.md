# Wild corpus 3: references as LLMs write them

`wild-3.jsonl` holds 1,135 references copied from 100 chatbot answers,
each one checked by hand against free bibliographic sources. It is test
data, held out: no fix has seen it. **The wrong entries are what a model
wrote.** They pair real titles, real researchers and real identifiers in
combinations that don't exist. Don't cite, index or attribute them.

## Generation

- **Models.** 100 sessions, 50 each on `claude -p --model haiku` and
  `--model sonnet` (Claude Code 2.1.325). Every session ran with
  `--tools "" --strict-mcp-config --safe-mode --no-session-persistence` and
  the system prompt "You are a helpful assistant.", one user turn each, up
  to six at a time, on 2026-11-17. All four flags exist in this CLI version.
- **Domains.** Sessions 1–50 are medicine, nursing or allied health
  (physiotherapy, speech and language therapy, occupational therapy,
  podiatry, dietetics, audiology, midwifery, pharmacy, optometry,
  dentistry, paramedicine) and 51–100 cover other fields. In both halves
  the models alternate: odd sessions are haiku, even ones sonnet.
- **Prompts.** New prompts, none taken from wild-2, written the way a real
  user asks and weighted towards exact details. Session counts
  (haiku/sonnet) by family:
  - exact details, "with DOIs, PMIDs and page numbers", "give the arXiv
    numbers" (15/19): troponin 0/1-hour algorithms, Achilles tendinopathy
    loading, low-FODMAP trials, low-dose atropine, silver diamine fluoride,
    the 2D Ising model and the renormalization group, CRISPR-Cas9, zero-
    knowledge proofs, place and grid cells, climate sensitivity
  - niche subtopics (10/4): PICS-F, CO-OP for developmental coordination
    disorder, Safewards, the weekend effect, the Xhosa cattle-killing, the
    Hanseatic kontor at Bergen, Göbekli Tepe, Lapita pottery, the Kerala
    model, Tasmanian devil facial tumour disease
  - recent work, "papers from 2022–2024 on …" (5/6)
  - many references, "give me 15/20" (0/9)
  - a named style: Vancouver, APA 7, AMA, Harvard, BibTeX, Chicago, IEEE,
    MLA 9, ACS (8/4)
  - systematic-review asks, "list the RCTs included in …" (3/4)
  - non-English (5/2): Spanish and German two each, French, Portuguese and
    Japanese one each
  - books (4/2): nursing pathophysiology and pharmacology textbooks,
    graduate QFT and GR texts, undergraduate economics textbooks with
    editions and ISBNs, monographs on the Ottoman Empire, medieval Islamic
    philosophy and the Atlantic slave trade

  The families were assigned in prompt order while the models alternated,
  so the split by model is uneven (no haiku session asked for "many").
  Compare the slices below within a model, not across.
- **Refusals.** Three haiku answers declined to give a list from memory
  (sessions 21, 31 and 45) but still cited one work each. Most of the other
  answers say they were written from memory (87 of 100 say so or ask for
  checking), and many mark single entries "verify", "(?)" or "†".
- **References extracted.** 1,175: 430 from haiku and 745 from sonnet.

## Method

1. **Extraction.** A separate `claude -p --model sonnet` call with the same
   no-tools flags split each answer into references. For each one it copied
   the raw text and parsed the fields. All 1,175 raw strings were confirmed
   to be exact substrings of the answers.
   - **Claims.** Each claim is the reference as written, with markdown
     emphasis stripped. Table rows are joined with " | ", dropping a leading
     row number. DOIs or PMIDs on their own sub-lines under a reference are
     appended.
   - **Extractor errors.** Every identifier and title the extractor reported
     was checked against the answer. None was invented; seven titles that
     had looked absent only carried markdown italics inside them.
2. **Lookup.** Each reference was looked up in free sources only:
   - Crossref `/works/{doi}` and `query.bibliographic`, with and without
     author and year filters
   - Europe PMC REST: PMID, DOI, phrase-title and author-plus-year searches
   - the arXiv API: `id_list`, and title search for poor matches
   - DataCite DOIs (LIPIcs, arXiv DOIs)
   - the doi.org handle API, to tell whether a DOI exists
   - Open Library search and `/isbn/`, for books
   - Requests were paced (Crossref at about 1/s, with a contact mailto in
     the polite pool; arXiv at one request per 3 s) and every response was
     cached outside the repository. Neither OpenAlex nor aspero was used.
3. **Comparison.** The closest record, merged with other records of the same
   work (Crossref sometimes lists only a first author, and Europe PMC
   carries the print year), was compared field by field:
   - every listed author against the record's author list (accents folded,
     compound surnames joined)
   - year against every date on the record. For books, any edition year
     counts; for arXiv papers, the posting and update years and the year
     after the first posting (the conference year)
   - journal (abbreviations, initialisms and conference acronyms such as
     NeurIPS, ICLR, EMNLP accepted). Not checked for books.
   - volume, issue, first and last page (".e3" suffixes and "Suppl" issues
     accepted), DOI, PMID and arXiv ID, each ID checked against its own
     record
   - title, as word edits after normalising case, punctuation, hyphenation
     and British/American spelling
4. **Manual review.** Every reference the comparison did not pass as
   correct was reviewed by hand (370, with targeted extra lookups). So was
   a random 90 of those it passed. Two comparison faults surfaced during
   review and were fixed before the counts:
   - A check that compared only the main titles when both titles had
     subtitles hid rewritten subtitles.
   - arXiv lookups were failing silently on an HTTP-to-HTTPS redirect.

   The audit of passes found one error: a book (*Bowling Alone*) accepted
   against an unrelated conference paper with the same short title. All 14
   books that had passed against a non-book record (mostly reviews of the
   book) were re-grounded on Open Library. Decisions and evidence are in
   each line's `why`.

### Classes

| Class | Rule | Category, `expect` | n |
|---|---|---|---|
| real-correct | The work exists and every cited field matches. Human-level imperfection is allowed: initials, "et al.", an abbreviated journal, a dropped subtitle, one word changed, an initialism for spelled-out words ("DCD", "LLM", "ATS/ESICM/SCCM", "MAPbI₃"), a title cut short with an ellipsis, a trial label added ("(REMOTE-CR)"), an article number for pages, a date on the record, another edition or a reissue of a book, the v1 title of a retitled arXiv paper. Identifier-led short citations ("RWKV: arXiv:2305.13048") and table rows whose title cell describes the finding ("Spacers derive from foreign DNA \| J Mol Evol 60(2):174–182 \| DOI \| PMID") pass when the identifiers and locator match the cited first author and year (69 lines). | `w:real-correct`, real: true, ids with prefix. Expect verified or plausible. | 1,013 |
| real-flawed:fields | The citation clearly refers to one real work (its title, or its authors with its venue and locator), but the listed fields are wrong. Fields, comma-separated in the category: `title` (two or more words changed, or a subtitle added or rewritten), `authors` (any listed name not on the record), `year`, `journal`, `volume`, `issue`, `pages` (first page), `lastpage`, `doi`, `pmid`, `arxiv`. | `w:real-flawed:<fields>`, real: false, ids of the intended work. Expect per the policy table: a wrong ID gives mismatch. Journal, volume, pages or co-authors alone give plausible. A year one off, or a wrong issue or last page alone, gives verified or plausible. Anything else gives plausible or mismatch. | 116 |
| chimera | Parts come from two real works: one paper's title with a sibling paper's year, venue, locator or DOI. | `w:chimera`, real: false, no ids. Expect plausible, mismatch or not_found. | 6 |
| fabricated | No real work is referred to. | `a:wild-fabricated`. Expect not_found, or mismatch if it carries a real ID. **None found.** | 0 |
| unclear | Excluded from the file. | 15 grey literature (NICE, ASA, NHS England, WHO HEN, Council of Europe, EEF, CRS and CDS reports, a speech, a self-published manual); 6 web publications (Transformer Circuits Thread posts, a WALS chapter); 16 articles, chapters, monographs and IACR ePrints that no free source indexes (Helv. Phys. Acta 1953, JETP Lett. 1986, Scand. J. Statist. 1979, EPW, Istanbuler Mitteilungen, German and Spanish monographs, chapters held only as later reprints); 2 with no title or venue; 1 table row naming two works. | 40 |

Books carry `isbn:` ids where the cited ISBN resolves to the same work in
Open Library, and `openlibrary:` work ids otherwise (`scripts/redteam.py`
reads both). Books confirmed only through a review of them on Crossref
carry no ids.

## Results

| | haiku | sonnet | all |
|---|---|---|---|
| references extracted | 430 | 745 | 1,175 |
| unclear (excluded) | 11 | 29 | 40 |
| judged | 419 | 716 | 1,135 |
| real-correct | 360 (85.9%) | 653 (91.2%) | 1,013 (89.3%) |
| real-flawed | 54 (12.9%) | 62 (8.7%) | 116 (10.2%) |
| chimera | 5 (1.2%) | 1 (0.1%) | 6 (0.5%) |
| fabricated | 0 | 0 | 0 |
| wrong beyond a soft field (not only an issue or last page) | 54 (12.9%) | 61 (8.5%) | 115 (10.1%) |

Wrong rate (flawed or chimera, as a share of judged references) by slice.
Sessions per family differ by model (see Generation).

| Slice | haiku | sonnet |
|---|---|---|
| medicine/nursing/allied health | 28.7% (43/150) | 11.1% (33/297) |
| other fields | 5.9% (16/269) | 7.2% (30/419) |
| exact details asked for | 17.7% (23/130) | 17.3% (49/284) |
| systematic-review ask | 21.4% (6/28) | 4.3% (2/47) |
| non-English prompt | 20.0% (7/35) | 6.2% (1/16) |
| niche subtopic | 15.5% (11/71) | 3.7% (2/54) |
| named style | 13.0% (9/69) | 2.6% (1/39) |
| recent work (2022–24) | 4.3% (2/47) | 3.6% (4/112) |
| many references | — | 2.9% (4/137) |
| books asked for | 2.6% (1/39) | 0.0% (0/27) |

The fields wrong among flawed references, as a share of each model's
judged references. A reference can have several.

| Field | haiku | sonnet |
|---|---|---|
| title | 15 (3.6%) | 32 (4.5%) |
| doi | 17 (4.1%) | 10 (1.4%) |
| pmid | 8 (1.9%) | 16 (2.2%) |
| issue | 12 (2.9%) | 2 (0.3%) |
| volume | 10 (2.4%) | 4 (0.6%) |
| pages (first) | 10 (2.4%) | 4 (0.6%) |
| journal | 8 (1.9%) | 1 (0.1%) |
| year | 7 (1.7%) | 3 (0.4%) |
| authors | 6 (1.4%) | 0 |
| last page | 3 (0.7%) | 2 (0.3%) |
| arxiv | 1 (0.2%) | 0 |

Of the references that carry a DOI, 10.1% of haiku's (17/169) and 3.3% of
sonnet's (10/300) give a wrong one. Of those that carry a PMID, 27.6% of
haiku's (8/29) and 17.2% of sonnet's (16/93) give a wrong one. By type,
journal articles and other non-book, non-arXiv references were wrong 13.4%
of the time (112/838), references cited by arXiv ID 5.0% (8/159) and books
1.4% (2/138).

## Hallucination patterns

1. **Near-miss PMIDs are the new main failure.** Asked for PMIDs, both
   models give numbers a few digits from the real one, and those numbers
   belong to unrelated papers indexed the same week. For example:
   - Burtin 2009 cited as 19623055 (real 19623052). The cited number is a
     review of anaesthesia for awake neurosurgery.
   - Chiang 2022 cited as 34807718 (real …719), an opioid-withdrawal
     editorial.
   - Church 2020 cited as 33023521 (real 33028215), an Ethiopian
     birth-weight trial.
   - Beyer 2015 cited as 25972008, a crystal-structure paper.

   Most of these citations are otherwise right in every field, so a
   resolver that trusts the PMID lands on the wrong work.
2. **Near-miss DOIs persist.** They have the right prefix, journal and
   year, with the last digits off:
   - Gattinoni …NEJMoa010041 for …010043 (a T-cell lymphoma paper)
   - Rich …NEJM199511023331802 for …806 (Gilbert's syndrome)
   - Ripple and Beschta aspen …2007.05.019 for …05.006 (bat fatalities at
     wind turbines)
   - Kalisvaart …2005.53221.x, the MoCA paper

   Some don't exist at all, like Schaller …31637-7 for …31637-3 and Dale
   …06475-7 for …06475-2 (doi.org 404).
3. **Titles shortened to a phrase.** In compact styles the title becomes a
   gist while authors, journal, locator and DOI stay right. Sonnet's
   ACS-style list on perovskite stability shortened 21 of 28 titles
   ("Rubidium incorporation", "CuSCN hole extraction layers", "Iodide
   management in FA-based perovskites"). The Achilles trial list did the
   same ("Eccentric vs concentric calf training"). Elsewhere subtitles
   and study designs get rewritten:
   - "increase cancer survival rates and improve quality of life"
     (Silver and Baima)
   - "a preliminary study" for "a review of the literature" (Sung and
     Chang)
   - "gastroesophageal reflux" for "pulmonary aspiration" (Klein and Hobai)
   - "intravenous methylprednisolone" for "hydrocortisone infusion"
     (Confalonieri)
4. **Locator drift.** Title and authors are right, and the venue or locator
   belongs elsewhere:
   - Moran's prehabilitation meta-analysis in Colorectal Dis for Surgery
   - Onder's reality-orientation trial in BMJ for Br J Psychiatry
   - Tsui in AER for the Economic Journal
   - Pegg in J Mod Afr Stud for African Affairs
   - Poole's "March to Peterloo" as Past & Present no. 144 (1994) for 192
     (2006)
   - Kauffman 2007 as Ecol Lett 2010;13(12)
   - Siddle 2013 as PNAS 2010;107
5. **Chimeras from sibling papers.** Six cases (five haiku), each splicing
   two works by the same group:
   - Rubini Gimenez's hs-cTnI title on Reichlin's two-hour-algorithm
     locator and DOI
   - the CAPE COD trial named but given CAPE COVID's JAMA locator
   - the 2018 ERAS colorectal guideline's title and third author on the
     2012 colonic guideline's Clin Nutr locator
   - Freemantle's 2012 JRSM title with the 2015 BMJ paper's year
   - the EXTEND-IA TNK title on EXTEND-IA's NEJM locator
   - Cotet and Tsui's Scand J Econ title with their AEJ: Macroeconomics
     venue
6. **Invented co-authors are rarer than in wild-2.** Only haiku, six
   references: "Pochard F" on Kentish-Barnes 2015, "Barré O" on
   Lichtenstein 1999, "Dargie HJ" on Blue 2001, "Gorman P" and
   "Stevens-Kempen J" on Jaarsma 1999, "Hoogstraten N" on Kalisvaart 2006,
   and "Harrower A" for Harroun on Clayton's pharmacology text.
7. **No wholly invented works.** Across 1,135 judged references, every
   wrong one points at a real work or a blend of two. One sonnet entry
   that no source holds (a 2017 CO-OP group paper with no venue) is
   excluded as unclear, not counted as invented.
8. **Asking for exact details costs accuracy, for both models.** Both were
   wrong about 17% of the time on exact-detail asks, and this family holds
   49 of sonnet's 63 wrong references. Recent-work and many-reference asks
   were 3–4%, because the answers lean on well-known arXiv IDs and DOIs.
9. **Books and arXiv IDs are the safe ground.** Books were wrong 1.4% of the
   time, and every ISBN that Open Library holds matched its book.
   Identifier-led arXiv citations ("Gorilla (2023): 2305.15334") were right
   in all but one case: "Hidden attention of Mamba" given another paper's
   ID.

## Notes

- **Instruction-like text.** No generated answer and no API response
  contained instruction-like text. Two arXiv and Crossref abstracts mention
  "system prompts" as a research topic.
- **Independence.** This round was built without reading the other
  red-team corpora or results, the matching code, the publication rules
  (`crates/core/src/publication/`) or the resolver
  (`crates/engine/src/resolve/`), and without running aspero. Only
  `wild-2.md`, this directory's README and `scripts/redteam.py` (line
  format and scoring) were read.
- **Line format.** Each line carries `n`, `category`, `real`, `claim`,
  `expect`, `why`, `ids` (left out when there are none), `model` and
  `session` (1–100). `n` runs 1–1,135 in session order.
