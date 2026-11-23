# Wild corpus 5: references as LLMs write them

`wild-5.jsonl` holds 864 references copied from 100 chatbot answers,
each one checked by hand against free bibliographic sources. It is test
data, held out: no aspero fix has been tuned on it. **The wrong entries
are what a model wrote.** They pair real titles, real researchers and real
identifiers in combinations that don't exist, and one is an invented work.
Don't cite, index or attribute them.

It was built as [wild-4](wild-4.md) was, with new prompts and a third
writer (opus), to measure aspero after the wild-4 round on model output
that no fix was tuned on.

## Generation

- **Independence.** The 100 prompts were written by a naive subagent that
  knew nothing about aspero: it was asked for realistic requests for
  sources, of the kind students, clinicians, researchers and journalists
  type, with wild-4's 100 prompts as topics to avoid. The writers
  were Claude 5.5 models: haiku, sonnet and opus. The judging used only
  public bibliographic sources, never aspero, and no one who judged read
  aspero's code or ran it.
- **Models.** 100 sessions on `claude -p --model haiku`, `--model sonnet`
  and `--model opus` (`claude-haiku-5-5`, `claude-sonnet-5-5`,
  `claude-opus-5-5`). Every session ran with
  `--tools "" --strict-mcp-config --safe-mode --no-session-persistence` and
  the system prompt "You are a helpful assistant.", one user turn each, up
  to six at a time, on 2026-11-23. (The sonnet and opus sessions also list
  `claude-haiku-5-5`, Claude Code's helper model.)
- **Domains.** Sessions 1–50 are medicine, nursing or allied health
  (nursing, midwifery, paramedicine, physiotherapy, occupational therapy,
  speech and language therapy, audiology, pharmacy, dentistry, public
  health) and 51–100 cover other fields.
- **Models by domain.** The models rotate haiku, sonnet, opus within each
  domain, which gives 34 haiku, 34 sonnet and 32 opus sessions (17, 17
  and 16 per domain).
- **Prompts.** Each prompt is its own kind of request, written the way a
  real user asks, often with a role ("2nd-year nursing student", "local
  reporter", "MPH dissertation"). The prompts don't follow wild-4's
  families; grouped loosely, with sessions (haiku/sonnet/opus):
  - critiques, debates and "is it evidence based?" (6/5/8): sensory
    integration therapy, intermittent fasting, the backfire effect, bike
    lanes and shops, the minimum wage, the 15-minute city, Tacoma Narrows
  - grey literature, theses, standards and reports asked for (3/6/5):
    the Soft Drinks Industry Levy, PhDs on male nurses, welded-joint fatigue
    standards, the EU CBAM, MT post-editing theses, Göbekli Tepe excavation
    reports
  - recent work (5/3/1): AI mammography screening, conduction-system
    pacing, sodium-ion anodes, the fourth global bleaching event
  - books asked for (3/3/4): the QALY's critics, Mauss's *Gift* and its
    translations, Beethoven's late style, the Sicilian mafia, the Gulag
  - identifiers asked for, "with DOIs", "plus arXiv" (4/2/1)
  - a named style: Harvard, APA 7, MLA, Bluebook, ABNT, RIS for Zotero
    (3/5/0)
  - confirm or trace one work (3/3/4): Kane 1988 on clozapine, the FAIR
    principles, Rayleigh and the blue sky, the trolley problem
  - non-English (6/5/4): one each in Arabic, Spanish, Portuguese, French,
    German, Japanese, Korean, Catalan, Italian, Chinese, Russian, Turkish,
    Indonesian, Dutch and Polish
- **Refusals.** Four haiku answers cited nothing and pointed to databases
  instead (sessions 10, 49, 72 and 99). 96 of the 100 answers say they were
  written from memory or ask for checking (a keyword count across the 16
  languages).
- **References extracted.** 1,078: 200 from haiku, 383 from sonnet and 495
  from opus.

## Method

As [wild-4's](wild-4.md#method), with these differences:

1. **Extraction.** The same `claude -p --model sonnet` extractor, with an
   ISBN field added. Each citation the answer gives is its own claim, as
   written: a reference line, a table row (its cells joined with " | "), a
   BibTeX or RIS entry, or a bullet with its notes. All 1,078 raw strings
   but one (a heading, judged unclear) are exact substrings of the answers.
2. **Lookup.** The same free sources, paced the same way, with the
   project's noreply address as the contact mailto: Crossref, Europe PMC
   (which carries PubMed), DataCite, arXiv and Open Library. Two sources
   were added: doi.org content negotiation for DOIs registered outside
   Crossref and DataCite, and Open Library's ISBN lookup. Neither OpenAlex
   nor aspero was used.
3. **Initialisms and locators.** The comparison now expands an initialism
   in a cited title against the record ("ACLR" for "anterior cruciate
   ligament reconstruction"), and looks up citations that give no title by
   first author, journal, year, volume and first page ("Stiell IG, et al.
   JAMA 2001;286:1841–8"). A locator-led citation is judged against the
   work its locator and first author identify, as wild-3 judged
   identifier-led ones.
4. **Manual review.** Every reference the comparison didn't pass as
   correct (406) was reviewed by hand, with targeted extra lookups, and
   so was a random 70 of the 672 it passed (seed 20261123); none of those
   70 was overturned. A second pass read the 136 riskiest passes (label- or
   locator-led, low title similarity, Open Library matches, non-article
   records, short claims) and overturned 3: a prose mention of a
   questionnaire matched to its PsycTESTS record, and two WHO publications
   matched to articles about them. Every hand decision is recorded with its
   evidence in `target/wild-5/parts/`, and the `why` of each line carries
   it.

   The automatic comparison was again weak on books, on terse citations
   and on venue abbreviations:
   - of its 135 "fabricated?" proposals, 34 were real and correct, 4 real
     but flawed, 96 unclear (mostly citations with no title, and grey
     literature) and one the fabricated work below;
   - books matched to reviews, to later reissues (dated up to 2026) or to
     reprinted chapters, giving false year, author and venue flags;
   - conference and journal short forms (ACM TOCS, SOSP, PODC, TAR, JAE,
     BJSM, JOSPT, "Bulletin of the WHO") drew false journal flags;
   - corporate authors (NICE, WHO, CDC) drew author flags, and an issue
     written "02" an issue flag.

### Classes

As [wild-4's](wild-4.md#classes), which follow
[wild-3's](wild-3.md#classes). How the rules were applied where this
corpus raised new cases:

- **Titles.** A dropped subtitle, or a subtitle shortened at its end, is
  real-correct; the main title cut by two or more words without an
  ellipsis is `title` (as wild-4's "losing 'and England'"). A title quoted
  inside a prose description by its opening words, or cited by its
  subtitle in quotation marks, is real-correct.
- **No title.** A citation that gives authors, venue and year with a
  description instead of a title ("Fox et al., *Age Ageing* 2014:
  systematic review of …") is unclear, as in wild-4; with a volume and
  first page it is judged against the work they identify.
- **Books.** Another edition, a translation named with its translator, or
  a year one off where Open Library holds both years is real-correct. A
  book confirmed only through a review of it in Crossref is real-correct
  with no ids, as in wild-3.

| Class | Category, `expect` | n |
|---|---|---|
| real-correct | `w:real-correct`, real: true. Expect verified or plausible. | 827 |
| real-flawed:fields | `w:real-flawed:<fields>`, real: false. Expect per the policy table. | 36 |
| chimera | `w:chimera`. **None found.** | 0 |
| fabricated | `a:wild-fabricated`, real: false. Expect not_found, or mismatch since it carries a real DOI. | 1 |
| unclear | Excluded: 123 citations with no title or locator (authors, venue and year with a description; a trial label; a prose mention); 40 grey literature (NICE, WHO, PHE and NHS England documents, HAS and AWMF guidelines, agency, think-tank and engineering reports, a protocol page); 22 works no free source indexes (law reviews, USENIX papers, archaeology journals, Chinese journals behind ChinaDOI); 9 chapters or originals held only as later reprints; 6 theses; 4 U.S. Supreme Court cases; 4 engineering standards; 3 web or magazine pieces; 3 whose edition or year no free source confirms. | 214 |

## Results

| | haiku | sonnet | opus | all |
|---|---|---|---|---|
| references extracted | 200 | 383 | 495 | 1,078 |
| unclear (excluded) | 43 | 81 | 90 | 214 |
| judged | 157 | 302 | 405 | 864 |
| real-correct | 137 (87.3%) | 291 (96.4%) | 399 (98.5%) | 827 (95.7%) |
| real-flawed | 19 (12.1%) | 11 (3.6%) | 6 (1.5%) | 36 (4.2%) |
| chimera | 0 | 0 | 0 | 0 |
| fabricated | 1 (0.6%) | 0 | 0 | 1 (0.1%) |
| wrong beyond a soft field (not only an issue or last page) | 20 (12.7%) | 11 (3.6%) | 6 (1.5%) | 37 (4.3%) |

Haiku was wrong a little more often than in wild-4 (12.7% against
10.8%), sonnet exactly as often (3.6%), and opus, new here, least of all.
With 37 wrong references, against wild-4's 75, this corpus measures the
wrong → verified rate coarsely: one wrong verified is 2.7 points.

Wrong rate (flawed, chimera or fabricated, as a share of judged
references) by slice. A session can fall in several slices or none.

| Slice | haiku | sonnet | opus |
|---|---|---|---|
| medicine/nursing/allied health | 18.1% (13/72) | 4.4% (4/90) | 1.6% (4/254) |
| other fields | 8.2% (7/85) | 3.3% (7/212) | 1.3% (2/151) |
| identifiers asked | 36.4% (4/11) | 16.7% (1/6) | 0.0% (0/4) |
| named style | 58.3% (7/12) | 1.9% (1/54) | — |
| grey literature or theses asked | 28.6% (2/7) | 6.5% (3/46) | 2.2% (1/46) |
| recent work | 14.3% (3/21) | 0.0% (0/17) | 50.0% (1/2) |
| critiques or debate | 15.2% (5/33) | 2.6% (1/38) | 1.3% (2/155) |
| non-English prompt | 16.7% (4/24) | 4.2% (2/48) | 2.0% (1/51) |
| books asked | 0.0% (0/21) | 0.0% (0/32) | 0.0% (0/165) |
| confirm or trace one work | 0.0% (0/4) | 0.0% (0/14) | 0.0% (0/15) |

The fields wrong among flawed references. A reference can have several.

| Field | haiku | sonnet | opus |
|---|---|---|---|
| doi | 7 | 1 | 0 |
| title | 5 | 4 | 6 |
| authors | 3 | 1 | 0 |
| year | 3 | 2 | 0 |
| journal | 3 | 4 | 0 |
| volume | 0 | 2 | 0 |
| issue | 1 | 0 | 0 |
| pages (first) | 0 | 1 | 0 |
| last page | 0 | 0 | 0 |
| pmid | 0 | 0 | 0 |
| arxiv | 0 | 0 | 0 |

The naive prompts rarely asked for identifiers, so few references carry
one: 122 a DOI, one a PMID and six an arXiv ID (wild-4's prompts asked for
them often). Of the references that carry a DOI, 17.9% of haiku's (7/39)
give a wrong one, plus the fabricated work's; sonnet's 2.2% (1/46); opus's
none (0/37). Three cited DOIs don't exist; the others belong to other
papers. By type, journal articles were wrong 5.7% of the time (36/637),
books never (0/185), and chapters, conference papers and reports once
(1/42).

## Hallucination patterns

1. **Haiku declines more and cites less.** All four answers that cited
   nothing are haiku's, and haiku gave 200 references to opus's 495. Where
   it did cite, it was the least accurate writer, and most so when asked
   for identifiers or a named style: in session 46 (Portuguese, ABNT "with
   DOI") four of the six references carry a wrong DOI.
2. **Near-miss DOIs are still haiku's main failure.** Right prefix,
   journal and year, last digits off; the cited number is another paper or
   doesn't exist:
   - Macinko 2006 jech.2005.038315 for …038323 and Rocha & Soares
     hec.1569 for hec.1607, both other papers in the same issue range
   - Aquino 2009 AJPH.2007.127100 and Macinko 2010
     hlthaff.2010.0352 ("provavelmente"), which don't exist, for …127480
     and …0251
   - Shepherd 2008 cgh.2008.02.010 for …02.058, van Baarsen 2001
     …971158 for …971103, Robinson 2018 ddi.12697 for ddi.12698
   - one from sonnet: Ozkula 2023 s11069-023-06142-0, which doesn't
     exist, for …06143-2
3. **The first invented work in a wild corpus.** Haiku's "Wongpakaran N,
   Wongpakaran T, Wedding D, Gwin KL (2011). Development and validation of
   a simple screening instrument for loneliness in older adults. *BMC
   Psychiatry* 11:21", with the DOI of an unrelated *BMC Psychiatry* 2011
   paper on paliperidone. The author list is that of the same group's 2013
   kappa paper (Gwet misspelt Gwin); the group's loneliness scale is the
   2020 RULS-6, a different title, year and journal. The answer itself said
   it was "less certain of this title".
4. **Venues moved to a sibling journal, all sonnet's and haiku's.**
   Rahman & Pallikadavath (*Econ Hum Biol*) in *Popul Space Place*;
   Mörsdorf (*Energy Policy*) in *Energy Economics*; Johnston et al.
   (*Build Res Inf*) in *BSERT*; Menezes et al. (*Applied Energy*) in
   *Energy and Buildings*, subtitle rewritten; Treisman (*Comp Polit
   Stud*) in *Perspectives on Politics*, subtitle rewritten; Simonovits et
   al. (*J Politics* 84(3):1806–11) in *APSR* with an invented locator
   116(3):1028–46; Joseph et al. (*JAMA Surg*, "geriatric trauma
   patients") in *Am J Surg* as "surgical patients".
5. **Years drift between sibling papers.** Haiku swapped the years of
   Hughes et al.'s two *Nature* bleaching papers (2017 and 2018); Fischer's
   2005 *Social Networks* review became 2002, Helmke et al.'s 2022 *AJPS*
   paper 2024, Green & Unruh's 2006 *Am J Phys* paper 1999 with volume 67.
6. **Invented co-authors.** Coleman 2013 gained four (Keogh, Hamer,
   Sheldon, Smith), Gillespie's Cochrane review Fairweather, Staudacher
   2017 Fayers (all haiku); sonnet added Agard, first author of
   Bertozzi's 2004 *JACS* cycloaddition paper, to Laughlin et al.'s 2008
   *Science* zebrafish paper.
7. **Opus errs only in titles.** All six of opus's wrong references are
   titles rewritten or cut (Ackland's cervical-collar paper, the Omairi
   ASI trial, the 2023 HRS pacing guideline, a CDC *MMWR* note that turns
   "two cases in newborns" into "a neonate", Volker & Handy's review,
   Dietrich's Göbekli Tepe cereal paper); its identifiers, locators and
   authors were never wrong. Its style is terse: initialisms in titles
   ("ACLR", "ASD"), abbreviated venues (AJSM, JOSPT, TAR), locator-only
   citations, and "Author et al., *Journal* Year: what it found" bullets,
   which are most of its 90 unclear references.
8. **Haiku and sonnet cut and rewrite titles too.** Twenge 2018 without
   "and Links to Increased New Media Screen Time", Bialy & Loeb without
   "'s Peculiar Acceleration", the PALACE trial's title rewritten, the
   Apple Heart Study design paper retitled in a RIS entry, Randive's "nine
   states" made "4 states".
9. **No chimeras.** Unlike wild-3 and wild-4, no reference joined one
   paper's title to another's locator.
10. **Books remain the safe ground.** None of the 185 judged books was
    wrong, and none of the 165 judged references in opus's four
    book-asking answers (the QALY's critics, translation studies,
    Beethoven, Mauss).

## aspero's results

**Not yet run.** This file is the held-out corpus only; aspero was not
run on it while it was built. The live score will come from the nightly
recorder, and the results belong in this section when they do.

When they are read: **these will measure the hard tail on purpose.** Every
reference is a model's own, often terse, in a table row or beside a trial
label or a note, and every wrong one is a near-miss of a real work but
one. They are not typical input. With 37 wrong references, one wrong
`verified` is 2.7 points.

## Notes

- **Instruction-like text.** No generated answer and no API response read
  in the review contained instruction-like text.
- **Independence.** The prompts came from a naive subagent with no
  knowledge of aspero. The judging read only wild-3.md, wild-4.md and
  wild-4's build scripts, and public sources: Crossref, Europe PMC,
  DataCite, arXiv, Open Library and doi.org. Nobody involved read aspero's
  code or ran it, and OpenAlex wasn't used.
- **Line format.** As wild-4: each line carries `n`, `category`, `real`,
  `claim`, `expect`, `why`, `ids` (left out when there are none), `model`
  and `session` (1–100). `n` runs 1–864 in session order. Books carry
  `openlibrary:` work ids; NCBI Bookshelf guidelines carry `pmid:` ids.
