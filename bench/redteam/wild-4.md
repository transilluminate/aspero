# Wild corpus 4: references as LLMs write them

`wild-4.jsonl` holds 1,135 references copied from 100 chatbot answers,
each one checked by hand against free bibliographic sources. It is test
data, held out: no fix has seen it. **The wrong entries are what a model
wrote.** They pair real titles, real researchers and real identifiers in
combinations that don't exist. Don't cite, index or attribute them.

It was built as [wild-3](wild-3.md) was, with new prompts, to measure
round 7 (#148) and later rounds on model output that no fix was tuned on.

## Generation

- **Models.** 100 sessions on `claude -p --model haiku` and
  `--model sonnet` (Claude Code 2.1.328; `claude-haiku-5-5` and
  `claude-sonnet-5-5`). Every session ran with
  `--tools "" --strict-mcp-config --safe-mode --no-session-persistence` and
  the system prompt "You are a helpful assistant.", one user turn each, up
  to six at a time, on 2026-11-20.
- **Domains.** Sessions 1–50 are medicine, nursing or allied health
  (physiotherapy, speech and language therapy, occupational therapy,
  optometry, dentistry, dietetics, pharmacy, midwifery, paramedicine) and
  51–100 cover other fields.
- **Models by family.** Unlike wild-3, the models alternate within each
  domain's prompt family, so every family splits evenly between them
  (haiku takes the odd one out). That gives 54 haiku and 46 sonnet
  sessions.
- **Prompts.** 100 new prompts, none on a wild-2 or wild-3 topic, written
  the way a real user asks. Session counts (haiku/sonnet) by family:
  - exact details, "with DOIs, PMIDs and page numbers", "arXiv IDs" (9/8):
    balanced crystalloids, SGLT2 inhibitors in HFpEF, ketamine vs
    etomidate, Nordic hamstring exercise, fluoride varnish, outdoor time
    and myopia, prehospital tourniquets, late-preterm steroids, the Higgs
    discovery, GW150914/GW170817, AlphaFold, diffusion models,
    predator-prey cycles, mixture-of-experts models, perovskite-silicon
    tandems, the onset of plate tectonics, the Flynn effect
  - niche subtopics (8/8): P6 acupressure, PPPD, the Lidcombe Program,
    medically tailored meals, kangaroo mother care, the PEDIG amblyopia
    trials, pharmacist-led hypertension care, intentional rounding, the
    Hattusa archives, the Great Stink, Tok Pisin, sea otters and kelp,
    Sámi reindeer herding, the Althing, Vibrio fischeri quorum sensing, the
    Tulsa Race Massacre
  - recent work, "papers from 2021–2024 on …" (6/6)
  - many references, "give me 15/20" (6/4)
  - a named style: Vancouver, BibTeX, AMA, Harvard, APA 7, Chicago, IEEE,
    MLA 9, ACS (6/6)
  - systematic-review asks, "list the RCTs included in …" (5/4)
  - non-English (8/6): Spanish and German four each, French, Italian two
    each, Chinese and Japanese one each
  - books (6/4): midwifery, clinical pharmacology and paramedic textbooks,
    the history of anaesthesia, principlism, algebraic topology, organic
    chemistry, the Mughal Empire, behavioural economics, reinforced
    concrete design
- **Refusals.** Four haiku answers declined to give a full list from
  memory (sessions 21, 34, 45 and 48); session 34 cited nothing. 85 of the
  100 answers say they were written from memory or ask for checking.
- **References extracted.** 1,194: 488 from haiku and 706 from sonnet.

## Method

As [wild-3's](wild-3.md#method), with these differences:

1. **Extraction.** The same `claude -p --model sonnet` extractor. All
   1,194 raw strings are exact substrings of the answers. A reference in a
   markdown table row is the whole row, its cells joined with " | " (a
   leading row number of one to three digits dropped), so a year or DOI
   cell stays in the claim. (The extractor had kept only the title cell of
   session 73's rows.)
2. **Lookup.** The same free sources, paced the same way, with a contact
   mailto (the project's noreply address) in the User-Agent. Neither
   OpenAlex nor aspero was used.
3. **Manual review.** Every reference the comparison didn't pass as correct
   (376) was reviewed by hand, with targeted extra lookups, and so was a
   random 90 of the 818 it passed (seed 20261120). Four reviewers took
   sessions 1–46, 47–62, 63–82 and 83–100. None of the 90 audited passes
   was overturned.

   The automatic comparison was weak on books and on several citation
   forms, which the review corrected by hand. Every one of its 106
   "fabricated?" proposals was a real work it had missed:
   - Books were matched to reviews of the book, to authorless guide
     entries or to later digital reissues (dated 2013–2025), giving false
     year, author and journal flags.
   - Open Library's title search returns nothing for "Title: Subtitle", and
     its search results give often only the first author and carry no work
     key. Books then fell to unrelated author-only hits.
   - Physics journal abbreviations (PLB, PRL, EPJC, JHEP), book series read
     as journals, collaboration names (ATLAS, CMS), initialisms in titles
     (AI, ESC), folded umlauts (Schönrath, Schoenrath), Cochrane's
     year(issue) volume and page forms like ".e3" or "; quiz 103-4" drew
     false flags.
   - A short arXiv-led label could lose to an unrelated best match even
     when its arXiv ID resolved correctly.

   Books carry `openlibrary:` work ids (the best title match among the
   first author's works in Open Library, by full and then main title, at a
   similarity of 0.9 or more) or `isbn:` ids where the reviewer confirmed
   the cited ISBN.

### Classes

As [wild-3's](wild-3.md#classes). One addition: a book with a different
publisher alone is real-correct (imprints and reissues change it, #126);
publisher and year both wrong is real-flawed.

| Class | Category, `expect` | n |
|---|---|---|
| real-correct | `w:real-correct`, real: true. Expect verified or plausible. | 1,060 |
| real-flawed:fields | `w:real-flawed:<fields>`, real: false. Expect per the policy table. | 71 |
| chimera | `w:chimera`, real: false. Expect plausible, mismatch or not_found. | 4 |
| fabricated | `a:wild-fabricated`. **None found.** | 0 |
| unclear | Excluded: grey literature (guidelines, agency and think-tank reports, a building code, a protocol specification), works no free source indexes (theses, chapters, monographs, a 1946 MA thesis), originals held only as later reprints, table rows with no title. | 59 |

## Results

| | haiku | sonnet | all |
|---|---|---|---|
| references extracted | 488 | 706 | 1,194 |
| unclear (excluded) | 15 | 44 | 59 |
| judged | 473 | 662 | 1,135 |
| real-correct | 422 (89.2%) | 638 (96.4%) | 1,060 (93.4%) |
| real-flawed | 48 (10.1%) | 23 (3.5%) | 71 (6.3%) |
| chimera | 3 (0.6%) | 1 (0.2%) | 4 (0.4%) |
| fabricated | 0 | 0 | 0 |
| wrong beyond a soft field (not only an issue or last page) | 49 (10.4%) | 22 (3.3%) | 71 (6.3%) |

Both models were more often right than in wild-3 (haiku 85.9%, sonnet
91.2% there). With 75 wrong references, against wild-3's 122, this corpus
measures the wrong → verified rate less finely: one wrong verified is
1.3 points.

Wrong rate (flawed or chimera, as a share of judged references) by slice.

| Slice | haiku | sonnet |
|---|---|---|
| medicine/nursing/allied health | 19.3% (26/135) | 6.8% (15/221) |
| other fields | 7.4% (25/338) | 2.0% (9/441) |
| exact details asked for | 19.7% (12/61) | 2.8% (4/145) |
| systematic-review ask | 16.7% (3/18) | 5.1% (3/59) |
| named style | 14.3% (7/49) | 0.0% (0/87) |
| niche subtopic | 14.1% (11/78) | 8.3% (9/108) |
| recent work (2021–24) | 8.2% (4/49) | 14.3% (7/49) |
| non-English prompt | 8.0% (6/75) | 1.2% (1/84) |
| books asked for | 6.5% (4/62) | 0.0% (0/58) |
| many references | 4.9% (4/81) | 0.0% (0/72) |

The fields wrong among flawed references. A reference can have several.

| Field | haiku | sonnet |
|---|---|---|
| doi | 21 | 6 |
| title | 14 | 4 |
| issue | 9 | 3 |
| pages (first) | 7 | 4 |
| volume | 7 | 3 |
| authors | 6 | 6 |
| year | 6 | 4 |
| journal | 4 | 2 |
| last page | 2 | 1 |
| pmid | 0 | 1 |
| arxiv | 0 | 0 |
| publisher | 1 | 0 |

Of the references that carry a DOI, 13.2% of haiku's (22/167) and 2.6% of
sonnet's (7/266) give a wrong one. Few references carried a PMID (3 of
haiku's, 26 of sonnet's; wild-3 had 122), so this corpus says little about
wrong PMIDs. By type, journal articles and other non-book, non-arXiv
references were wrong 10.1% of the time (62/612), references cited by
arXiv ID 2.3% (4/177) and books 2.6% (9/346).

## Hallucination patterns

1. **Near-miss DOIs are the main failure.** Right prefix, journal and
   year, last digits off, and the cited number is often another real paper
   or doesn't exist:
   - Beddoe & Murphy 2004 …20040701-05 for …-07
   - Wang 2009 Tai Chi art.24820 (a reply letter) for art.24832
   - Hopkins 2008 nature07381 for nature07465
   - Matthey 2008 da.20378 for da.20415
   - Wisner 2002 NEJMcp011541 and Teasdale 2008 …2007.01.008, which don't
     exist, for …011542 and …2007.01.007
   - Lee 2015's Cochrane review cited by its 2025 update's DOI (.pub5 for
     .pub4)
2. **Invented co-authors on trial-group papers.** The PEDIG amblyopia
   trials gained "Cole SR", "Cotter SA" and "Antoine CH" (sonnet), HOT-COVID
   gained "Crescioli E", and the Marinho fluoride-varnish review, cited by
   its 2013 update's DOI, carries the 2002 version's co-authors; a consortium label led to the wrong first
   author (MATISSE as "Madhi SA" for Kampmann; the 2024 ESC hypertension
   guideline as "Mancia G" for McEvoy).
3. **Rewritten titles and subtitles, mostly in books and older work.**
   Viñas's *El oro de Moscú: alfa y omega de un mito franquista* as
   "Historia de una mistificación"; Böick's *Die Treuhand: Idee – Praxis –
   Erfahrung* as "Die Treuhandanstalt. Der Umbau der ostdeutschen
   Wirtschaft"; Weeden's *Hittite Logograms and Hittite Scholarship*
   (Harrassowitz) as "… and Greek Ritual" (Oxford); Flynn 2009's "Raven's
   gains in Britain 1938-2008" as "Raven's data as a key to the puzzle";
   Miller's bloodfeud paper losing "and England".
4. **Locator and year drift between sibling papers.** The two Sims
   intentional-rounding papers swap years and venues (2018 BMJ Qual Saf,
   2020 Int J Nurs Stud); Christiansen's 2018 J Clin Nurs review cited as a
   2015 JBI Database paper; Stenseth's 2002 Science paper cited at Science
   1997;277:1283; Margolis 2013 at JAMA volume 309 for 310; the BOX trial
   at pages 1496-1505 for 1456-1466.
5. **Chimeras from sibling papers.** Four (three haiku): Butler's EHJ
   empagliflozin title on Neuen's Circulation locator, DOI and PMID; Lee's
   Clin Rehabil Tai Chi title on Chen's BMC Complement Altern Med venue and
   DOI; the ATLAS+CMS combination title on ATLAS's own EPJC paper's
   locator, DOI and arXiv ID; Paulay & Priestley's seismic design title
   with Priestley, Calvi & Kowalsky's authors and year.
6. **No wholly invented works.** As in wild-3, every wrong reference
   points at a real work or a blend of two.
7. **Sonnet barely errs outside medicine.** 2.0% wrong in other fields,
   none at all in named-style, many-reference and book asks. Its errors
   cluster in niche medical topics and in recent work (7 of 49), where
   DOIs and years of 2023–24 papers drift.
8. **Books and arXiv IDs remain the safe ground.** Books were wrong 2.6% of
   the time (eight of nine haiku's), arXiv-led references 2.3%.

## Notes

- **Instruction-like text.** No generated answer and no API response
  contained instruction-like text.
- **Independence.** The reviewers read only wild-3.md, this directory's
  README (the verdict policy) and the build scripts; none read aspero's
  code or ran it. The prompts were written by the maintainer agent that
  works on aspero's matching code, so this round is less blind than
  wild-3 at the prompt stage. No prompt was aimed at a known weakness, and
  the families and their mix follow wild-3.
- **Line format.** Each line carries `n`, `category`, `real`, `claim`,
  `expect`, `why`, `ids` (left out when there are none), `model` and
  `session` (1–100). `n` runs 1–1,135 in session order.
