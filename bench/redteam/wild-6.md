# Wild corpus 6: references as LLMs write them

`wild-6.jsonl` holds 748 references copied from 120 chatbot answers,
each one checked by hand against free bibliographic sources. It is test
data, held out: no aspero fix has been tuned on it. **The wrong entries
are what a model wrote.** They pair real titles, real researchers and real
identifiers in combinations that don't exist. Don't cite, index or
attribute them.

It was built as [wild-5](wild-5.md) was, with new prompts, 20 more
sessions and a different mix of writers, to measure aspero after the
wild-5 round on model output that no fix was tuned on.

## Generation

**This corpus is weighted toward error-prone requests and toward haiku on
purpose, so that wrong references are numerous enough to measure. It is
harder than typical use.** wild-5 held 37 wrong references, so one wrong
`verified` moved its headline rate by 2.7 points; wild-6 holds 83.

- **Independence.** The 120 prompts were written by a naive subagent that
  knew nothing about aspero: it was asked for realistic requests for
  sources, of the kind students, clinicians, researchers and journalists
  type, with wild-4's and wild-5's prompts as topics to avoid, and it was
  told to favour the request kinds that drew the most errors in wild-5. The
  writers were Claude 5.5 models: haiku, sonnet and opus. The judging used
  only public bibliographic sources, never aspero, and no one who judged
  read aspero's code or ran it.
- **Models.** 120 sessions on `claude -p --model haiku`, `--model sonnet`
  and `--model opus` (`claude-haiku-5-5`, `claude-sonnet-5-5`,
  `claude-opus-5-5`). Every session ran with
  `--tools "" --strict-mcp-config --safe-mode --no-session-persistence` and
  the system prompt "You are a helpful assistant.", one user turn each, up
  to six at a time, on 2026-11-25. (The sonnet and opus sessions also list
  `claude-haiku-5-5`, Claude Code's helper model.)
- **Domains.** Sessions 1–60 are medicine, nursing or allied health
  (pharmacy, audiology, physiotherapy, midwifery, paramedicine,
  occupational therapy, speech and language therapy, dentistry, public
  health) and 61–120 cover other fields.
- **Models by domain.** Within each domain the models rotate haiku,
  sonnet, haiku, opus, haiku, sonnet, which gives 60 haiku, 40 sonnet and
  20 opus sessions (30, 20 and 10 per domain). wild-5 had 34, 34 and 32.
- **Prompts.** Each prompt is written the way a real user asks, often with
  a role ("band 6 in a community palliative care team", "MPH
  dissertation", "council meeting next Tuesday"). By request kind, with
  sessions (haiku/sonnet/opus):
  - identifiers asked for, "with DOIs", "PMIDs", "arXiv IDs" (16/10/4)
  - a named style: Harvard, APA 7, Vancouver, AMA, Chicago, MLA 9, IEEE,
    OSCOLA, Bluebook, BibTeX, RIS for EndNote or Zotero (12/8/4)
  - recent work, "since 2023" or "2024–25" (8/4/4)
  - grey literature asked for: government and agency reports, guidance,
    theses, standards (6/6/2)
  - critiques and debates (6/4/2)
  - confirm or trace one work: the Sackett and Hardin references as the
    user wrote them, the weekend effect, interruptions and medication
    errors, weak ties, technical debt, the hungry judges (4/2/2)
  - reading lists, mostly books (4/4/0)
  - other: a patient leaflet, a table of validation papers, a timeline
    panel, an annotated bibliography (4/2/2)
  - non-English (8/4/2): Portuguese, Vietnamese, Czech, Greek, Japanese,
    Swedish, Hindi, Korean, Spanish, German, Turkish, French, Polish and
    Italian
- **Refusals.** Eleven answers cited nothing and pointed to databases or
  described studies without citing them: seven haiku (sessions 7, 11, 29,
  33, 37, 63, 89), three sonnet (36, 50, 60) and one opus (88). 116 of the
  120 answers say they were written from memory or ask for checking (a
  keyword count across the 15 languages).
- **References extracted.** 904: 292 from haiku, 418 from sonnet and 194
  from opus.

## Method

As [wild-5's](wild-5.md#method), with these differences:

1. **Extraction.** The same `claude -p --model sonnet` extractor. All 904
   raw strings are exact substrings of the answers. Session 110's answer is
   short: it corrects the user's Hardin reference and gives one citation,
   which is in the corpus.
2. **Lookup.** The same free sources, paced the same way, with the
   project's noreply address as the contact mailto: Crossref, Europe PMC
   (which carries PubMed), DataCite, arXiv, Open Library (search and ISBN)
   and doi.org content negotiation. Neither OpenAlex nor aspero was used.
   Europe PMC answered slowly and timed out under load, so the lookups ran
   eight at a time. A second full pass retried every call that had failed
   (the rest came from the cache); it changed one best match and no
   decision. Two arXiv IDs were also looked up at v1, to read a retitled
   paper's original title.
3. **Manual review.** Every reference the comparison didn't pass as
   correct (387) was reviewed by hand, with targeted extra lookups, and
   so was a random 70 of the 514 passes left (seed 20261125). The sample
   overturned three: a report matched to a journal article about it, a
   paper whose title was right but for its country (#464), and a DESI
   label carrying a sibling paper's arXiv ID (#589); a fourth, a WHO report
   matched to the Commission's Lancet article, was re-grounded on the
   report. A second pass read the 90 riskiest passes (label- or
   locator-led, low title similarity, Open Library matches, non-article
   records, short claims, no authors) and overturned two (Acts of
   Parliament matched to their Stationery Office prints, now unclear) and
   re-grounded six: five "label | arXiv ID" rows the comparison had matched
   to encyclopedia entries of the same name, and a book matched to a
   review of it. Three issue-only differences the comparison flagged were
   read and kept. One reviewer judged all 120 sessions.

   A second reviewer then audited the passes, since the random sample had
   found a one-word substitution (#464) that the title score let through.
   Every real-correct line (668) was checked strictly against its record:
   any cited title word the record lacks or replaces, and any year,
   journal, volume, issue, first page or identifier that disagrees, with
   the lines re-grounded by hand checked against the records their ids
   name. That flagged 78 lines and 25 re-grounded ones, all read by hand
   with the 44 books re-grounded on Open Library works (read with their
   editions); three were wrong: a rewritten subtitle Crossref
   doesn't carry (#466, Hirth's "effect of solar and wind power
   variability on their relative price" as "effect of wind and solar power
   on prices"), *ApJL* for a paper in *ApJ* (#587; its arXiv record has no
   journal), and Serianni's "lingua letteraria" as "lingua d'uso" (#673;
   Open Library's editions carry the subtitle). The rest were
   abbreviations, label rows, preprint matches and the like. A random 100
   of the 352 passes no one had read (seed 20261126), read in full,
   overturned none. Every hand decision is recorded with its evidence in
   `target/wild-6/parts/` (the audit in its p15 file), and the `why` of each
   line carries it.

   The automatic comparison was weak in the same places as in wild-5, and
   in two new ones:
   - of its 144 "fabricated?" proposals, 50 were real and correct, 6 real
     but flawed, one a chimera and 87 unclear (citations with no title,
     grey literature, court judgments and legislation); none was an
     invented work;
   - of its 196 "flawed?" proposals, 109 were real and correct:
     conference short names (FSE, ICSE, ISSTA, MSR), "Oxford UP" read as a
     journal, initialisms in titles (AUC, MRSA, SAE, FRB), `.e4` and
     article-number pages, journals under a later name;
   - identifier-led rows with no authors or year ("EAGLE | 2401.15077"),
     73 of them in two sonnet answers, were proposed as fabricated or
     matched to encyclopedia entries of the same name; they were judged
     by their arXiv IDs.

### Classes

As [wild-5's](wild-5.md#classes). How the rules were applied where this
corpus raised new cases:

- **Years of books.** A book cited one year after the only edition year
  any free source holds (often its copyright year) is `year`, with expect
  verified or plausible. wild-5 made a year one off real-correct only where
  Open Library holds both years. Six books fall here.
- **One word that changes the work.** A title with one word changed is
  real-correct, unless the word names a different work: "5-year results"
  as "2-year results", "in Poland" as "in Germany" are `title`, as was
  wild-5's "nine states" as "4 states".
- **Identifier-led rows.** "Label | arXiv ID" and "label: arXiv ID" rows
  are judged by the ID, as wild-3 judged identifier-led short citations:
  real-correct when the ID is the paper the label names. A retitled arXiv
  paper cited by its v1 title is real-correct.
- **Companion papers.** A citation carrying a companion paper's locator or
  identifier is flawed in that field, as wild-5's near-miss DOIs were. It
  is a chimera when the author list and year come from one paper and the
  title or topic and venue from a sibling.
- **Long author lists.** A name not on the record is `authors` wherever it
  stands, including the last author an APA 7 list gives after "…".
- **Not bibliographic records.** Court judgments, legislation, law review
  articles that no free source indexes, newspaper and blog pieces, web
  standards and databases are unclear. A blog post registered in Crossref
  is judged as a work.

| Class | Category, `expect` | n |
|---|---|---|
| real-correct | `w:real-correct`, real: true. Expect verified or plausible. | 665 |
| real-flawed:fields | `w:real-flawed:<fields>`, real: false. Expect per the policy table. | 81 |
| chimera | `w:chimera`, real: false. Expect plausible, mismatch or not_found. | 2 |
| fabricated | `a:wild-fabricated`. **None found.** | 0 |
| unclear | Excluded: 64 citations with no title or locator (authors, venue and year with a description; a trial or collaboration label; a prose mention); 33 grey literature (government, agency, inquiry, think-tank and engineering reports, NICE-style guidance not in the NCBI Bookshelf); 16 court judgments (UK, CJEU, ECtHR); 9 pieces of legislation; 9 newspaper, magazine, blog and encyclopedia pieces; 9 articles and chapters that no free source indexes; 6 law review articles; 4 editions or originals that no free source confirms; 3 standards and engineering bulletins; 2 theses; 1 database. | 156 |

## Results

| | haiku | sonnet | opus | all |
|---|---|---|---|---|
| references extracted | 292 | 418 | 194 | 904 |
| unclear (excluded) | 40 | 82 | 34 | 156 |
| judged | 252 | 336 | 160 | 748 |
| real-correct | 198 (78.6%) | 313 (93.2%) | 154 (96.3%) | 665 (88.9%) |
| real-flawed | 52 (20.6%) | 23 (6.8%) | 6 (3.8%) | 81 (10.8%) |
| chimera | 2 (0.8%) | 0 | 0 | 2 (0.3%) |
| fabricated | 0 | 0 | 0 | 0 |
| wrong beyond a soft field (not only an issue or last page) | 50 (19.8%) | 23 (6.8%) | 6 (3.8%) | 79 (10.6%) |

Every writer was wrong more often than in wild-5 (haiku 21.4% against
12.7%, sonnet 6.8% against 3.6%, opus 3.8% against 1.5%), which is what
the weighting toward error-prone requests was for. With 83 wrong
references, one wrong `verified` is 1.2 points.

Wrong rate (flawed, chimera or fabricated, as a share of judged
references) by slice. Each session has one request kind; the language and
domain slices cut across them.

| Slice | haiku | sonnet | opus |
|---|---|---|---|
| medicine/nursing/allied health | 28.4% (31/109) | 9.5% (11/116) | 3.0% (2/67) |
| other fields | 16.1% (23/143) | 5.5% (12/220) | 4.3% (4/93) |
| identifiers asked | 24.3% (17/70) | 9.8% (14/143) | 2.4% (1/41) |
| named style | 33.8% (23/68) | 5.8% (5/86) | 4.8% (2/42) |
| recent work | 33.3% (3/9) | 0.0% (0/42) | 0.0% (0/7) |
| grey literature asked | 0.0% (0/4) | 0.0% (0/9) | 0.0% (0/8) |
| critiques or debate | 11.4% (4/35) | 33.3% (1/3) | 7.1% (3/42) |
| confirm or trace one work | 16.7% (1/6) | 0.0% (0/1) | 0.0% (0/5) |
| reading lists | 7.1% (3/42) | 7.1% (3/42) | — |
| other requests | 16.7% (3/18) | 0.0% (0/10) | 0.0% (0/15) |
| non-English prompt | 21.9% (7/32) | 18.8% (3/16) | 8.3% (1/12) |

The fields wrong among flawed references. A reference can have several.

| Field | haiku | sonnet | opus |
|---|---|---|---|
| doi | 11 | 2 | 1 |
| title | 18 | 6 | 0 |
| authors | 7 | 9 | 2 |
| year | 5 | 5 | 2 |
| journal | 11 | 0 | 1 |
| volume | 5 | 0 | 0 |
| issue | 8 | 0 | 0 |
| pages (first) | 6 | 0 | 0 |
| last page | 1 | 0 | 0 |
| pmid | 1 | 1 | 0 |
| arxiv | 3 | 0 | 0 |

The prompts asked for identifiers far more often than wild-5's: 238
judged references carry a DOI, 10 a PMID and 88 an arXiv ID. Of the
references that carry a DOI, 17.2% of haiku's (11/64) give a wrong one,
sonnet's 1.7% (2/118) and opus's 1.8% (1/56). Eight cited DOIs don't
exist; the other six belong to other papers, five of them in the same
journal and year. Two PMIDs are other papers' (2/10), and three arXiv IDs
(3/88), all haiku's. By type, journal articles were wrong 14.8% of the
time (71/479), books 4.9% (7/143), conference papers 5 times in 26, and
preprints (0/72) and reports (0/22) never.

Haiku's hedges meant something: 39% of the references it marked as
uncertain were wrong (26/66), against 15% of those it didn't (28/186).

## Hallucination patterns

1. **Haiku is wrong one time in five, and most when asked for a style or
   for identifiers.** A third of its references in named-style answers
   were wrong (23/68). Its RIS answer for Zotero (session 87) gave three
   DOIs that are other papers in the same journal and year (#540); its
   APA 7 answer on mirror therapy (session 17) gave one paper the wrong
   co-authors, journal and locator (#107) and another a later year and a
   sibling journal with an invented volume and pages (#108).
2. **Near-miss DOIs are still haiku's.** Eleven of its 64 DOIs are wrong:
   seven don't exist (Ferber 2015, Robson 2001, Bell & Redelmeier 2001,
   Autor et al. 2016 and 2020, Veh et al. 2019, Kenny et al. 2021), and
   four are other papers (#385, and the three in #540's answer). Sonnet's
   two are Ehmann 2023 (a DOI that doesn't exist) and Legate et al. 2014
   (another *Language* paper); opus's one is Ardura Urquiaga et al. 2020
   (#456, another *Urban Studies* paper).
3. **Companion papers swap identifiers.** The three 2017 papers on
   FRB 121102 published together: haiku gave Marcote et al. the letter
   number and arXiv ID of Tendulkar et al. (#364) and Bassa et al. those
   of Marcote et al. (#366). Its "DESI 2024 III" carries the arXiv ID of
   DESI 2024 VI (#589).
4. **Two chimeras, both haiku's.** Khayambashi's 2016 *Am J Sports Med*
   author list and year with a rewritten title of his 2012 *JOSPT*
   patellofemoral trial (#48); and Innes et al.'s 2016 *Adv Dent Res*
   terminology paper, first author first, given its sibling's title,
   "consensus recommendations on carious tissue removal" (#146).
5. **Sonnet invents co-authors.** Nine of sonnet's 23 wrong references
   list a name the paper doesn't have: six in one answer on flaky tests
   (#397 among them), and three in APA 7 lists of 21 or more authors, two
   of them the last author given after "…" (#134). Opus did it twice (#105, #390).
6. **Titles rewritten, numbers and places changed.** "a cohort study of
   8,755 employees" became "107,000 air travellers" (#280), "rapidly fatal
   bowel ischaemia" "rapid onset intestinal obstruction" (#91), "can
   Ayurveda show the way forward?" "can Ayurvedomics change the game?"
   (#222), "5-year results" "2-year results" (#143), "in Poland" "in
   Germany" (#464), "variability on their relative price" "on prices"
   (#466), "lingua letteraria" "lingua d'uso" (#673). Haiku gave
   Senkov's *Refractory high-entropy alloys*
   a title from his later papers (#299).
7. **Venues moved to a sibling journal.** *Sci Adv* to *PNAS* (#592),
   *Ann Emerg Med* to *Ann Surg* (opus, #234), *JOSPT* to *Arch Phys Med
   Rehabil*, *J Dent Res* to *Caries Res*, *Eur J Clin Pharmacol* to *Br J
   Clin Pharmacol*, *Behav Res Ther* to *Community Dent Oral Epidemiol*,
   all haiku's, and *ApJ* to *ApJL* (#587). Some moved with a volume and pages of their own: *Otol
   Neurotol* 32(5):812 is an unrelated paper (#17); others gave *Invest
   Radiol* 55(8):544–555 (#186) and *Eur J Pain* 24(2):262–276 (#108).
8. **Opus errs least, and not in titles.** Its six wrong references are
   two invented co-authors, a sibling journal, a DOI of another paper and
   two years (#650 among them). In wild-5 all six of opus's errors were
   titles; here none is.
9. **No invented work.** Unlike wild-5, every judged reference refers to a
   real work.
10. **Identifiers asked of arXiv papers come back right.** Sonnet's two
    answers listing 73 papers by label and arXiv ID got every ID right;
    two cite the v1 titles of papers since retitled.
11. **Books remain the safe ground.** Seven of 143 judged books were
    wrong: five a year after the only edition free sources hold, one with a
    co-author who isn't on it (#267) and one with its subtitle rewritten
    (#673).

## aspero's results

**These measure the hard tail on purpose, and wild-6 more than most.** It
is weighted toward the requests and the writer that err most (identifiers
asked, named styles, recent work, grey literature; haiku in half the
sessions), so its wrong references are many and its real ones terse. It is
not typical input. With 83 wrong references, one wrong `verified` is 1.2
points.

The run was live, through `scripts/redteam.py` on 0.43.1, the release
wild-6 was held out from (no fix had seen it). It took two recorder
nights: on 2026-11-26 the free OpenAlex allowance ran out with 59 claims
capped, and on 2026-11-27 the run resumed from the tape and finished with
none capped. No claim panicked or failed internally. The 43 misses and
wrong verifieds were re-run live off the tape, with a fresh cache, the
same day: 42 gave the same verdict, and one came back `unverifiable` as
Europe PMC was slow that morning.

| Criterion | 0.43.1, held out | Target |
|---|---|---|
| Wrong → `verified` | 8/83 (9.6%) | ≤ 2% |
| Real → `mismatch` or `not_found` | 35/665 (5.3%) | ≤ 3% |
| Real → `verified` | 564/665 (84.8%) | ≥ 85% |
| Real → `verified` or `plausible` | 630/665 (94.7%) | ≥ 95% |
| Wrong → `plausible` naming the field that differs | 36/36 (100%) | ≥ 80% |
| … medicine/nursing/allied health: wrong → `verified` | 5/44 | |
| … medicine/nursing/allied health: real → `verified` | 216/248 (87.1%) | |
| … other fields: real → `verified` | 348/417 (83.5%) | |

Four criteria fail their targets, the real-citation ones narrowly. The
corpus has no invented works, nonexistent identifiers or retracted works.

By writer, wrong → `verified` was haiku 8/54, sonnet 0/23 and opus 0/6;
real → `verified` was haiku 169/198 (85.4%), sonnet 259/313 (82.7%) and
opus 136/154 (88.3%).

- **Wrong and verified (8, all haiku's).** Each matched the work the
  model meant, with one field wrong:
  - five titles reworded, everything else right: #14 (a cochlear-implant
    comparison retitled), #49 ("Additional strengthening …" for "The
    effect of additional strengthening …"), #130 ("… and birth outcomes"
    for "… and the risks of low birthweight and preterm birth"), #653 (a
    meta-analysis's title paraphrased) and #673 (a book's subtitle with
    one word changed);
  - two in the wrong journal: #145 and #592 (cited in PNAS, published in
    Science Advances);
  - one with a co-author changed: #267 (a workbook cited with Swinson for
    Craske and Barlow).
- **Real and missed (35: 11 `not_found`, 24 `mismatch`).** A first look:
  - **arXiv papers listed by a label beside their identifier, 18.** Two
    sonnet answers list sparse-autoencoder and speculative-decoding papers
    as "Gated SAEs: 2404.16014" or "Lookahead Decoding | 2402.02057", and
    a haiku answer cites "DESI DR2 BAO (arXiv:2503.14738)" (#590). The
    identifier is right; the label isn't the title, so the identifier is
    called another work's.
  - **A DOI with a dagger glued to its end, 3** (#404, #411, #414:
    "…00140†"), read as a DOI that doesn't exist.
  - **Books, 8,** not found or another work's (#92, #247, #265, #347,
    #487, #519, #677, #703).
  - **Grey literature and reports, 4** (#217, #218, #594, #600), and **2
    others**: #207, a WHO guideline beside an ISBN that doesn't resolve,
    and #597, an IPCC chapter matched to the whole report.
- **Accepted as another work (27).** Real references accepted whose
  matched record doesn't carry the cited DOI or PMID; as in wild-5, most
  are the same work under another identifier.

Against wild-5's held-out run on 0.43.0 (13.5%, 5.3%, 83.8%, 94.7%), the
real-citation figures are level or a point better on a corpus built to be
harder, and wrong → `verified` is lower, now measured on more than twice
as many wrong references. wild-6's misses are the next round's tuning
data; from that round on it no longer counts as held out.

## Notes

- **Instruction-like text.** No generated answer and no API response read
  in the review or the audit of passes contained instruction-like text.
- **Independence.** The prompts came from a naive subagent with no
  knowledge of aspero. The judging read only wild-3.md, wild-4.md,
  wild-5.md and wild-5's build scripts, and public sources: Crossref,
  Europe PMC, DataCite, arXiv, Open Library and doi.org (and, in the
  audit of passes, Semantic Scholar for one full title). Nobody involved
  read aspero's code or ran it, and OpenAlex wasn't used.
- **Line format.** As wild-5: each line carries `n`, `category`, `real`,
  `claim`, `expect`, `why`, `ids` (left out when there are none), `model`
  and `session` (1–120). `n` runs 1–748 in session order. Books carry
  `openlibrary:` work ids; NCBI Bookshelf guidelines carry `pmid:` ids;
  books confirmed only through a review of them carry none.
