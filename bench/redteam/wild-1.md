# Wild corpus 1: references as language models write them

No longer held out: its failures shaped round 5 (see
[README.md](README.md#round-5-the-fourth-held-out-corpus-and-the-first-wild-one-2026-11-16-unreleased)),
so a later round needs a new one. The scorer now splits real from wrong
by `real` and compares identifiers however they're written (see "Running
it" below, written before both).

`wild-1.jsonl` holds 394 references that two language models wrote when
asked for literature reviews and reading lists, each checked by hand against
free bibliographic indexes. The other red-team corpora were made up to
resemble model errors. These are the errors the models actually made, in the
proportions they made them.

**Every reference here was written by a model and is test data.** A
`w:real-correct` entry points to a real work. The other categories (`w:real-flawed`,
`w:chimera`, `w:fabricated`) cite details or whole works that are wrong or
don't exist. Don't cite, index or attribute them to anyone.

## How it was generated (2026-11-16)

- **Models:** `claude-haiku-5-5` and `claude-sonnet-5-5`, run through the
  `claude` CLI with `-p --tools "" --strict-mcp-config
  --no-session-persistence --system-prompt "You are a helpful assistant."`.
  Each run was a fresh session with no tools, no MCP servers and no
  retrieval, so every reference came from the model's memory.
- **Sessions:** 42 in total, 21 per model, with one prompt per session
  (odd-numbered sessions went to haiku, even-numbered to sonnet). The
  prompts used four templates that users typically write:
  - "Write a 300-word literature review on *X* with 8 references in *style*"
  - "List 10 key papers on *X* with full citations"
  - "Give me citations supporting the claim that *X*"
  - "What are the landmark trials for *X*? Cite them."

  Styles requested were APA, Vancouver, Harvard, BibTeX, Chicago, an
  IEEE-style numbered list, and "none".
- **Topics:**
  - Medicine and nursing (20 sessions): sepsis fluids, statins in primary
    prevention, SGLT2 inhibitors and other heart failure treatment,
    delirium prevention, pressure injuries, antenatal steroids, falls,
    antimicrobial resistance, blood pressure targets, anticoagulation in
    atrial fibrillation, hand hygiene, CRISPR, nurse staffing, early ICU
    mobilisation.
  - CS and ML (8 sessions): transformer interpretability, federated
    learning privacy, RLHF, graph neural networks, diffusion models.
  - Social science and humanities (8 sessions): social media and adolescent
    mental health, Brexit economics, Byzantine iconography, minimum wage,
    the German Energiewende.
  - Physics and chemistry (6 sessions): cuprate superconductivity,
    perovskite solar cells, MOFs for CO2 capture, gravitational waves,
    topological insulators, battery SEI growth.
  - One prompt each was in Spanish (falls), German (Energiewende) and
    French (antimicrobial resistance).
- **Extraction:**
  - 416 candidate lines were taken from the 42 replies.
  - 13 lines that weren't references were dropped: notes, word counts, and
    the prose summaries one reply gave after its BibTeX.
  - 4 lines that named two works each were split into 7 references.
  - 1 exact duplicate was dropped.

  That leaves **405 references**: 189 from haiku and 216 from sonnet. Each
  claim keeps the text exactly as the model wrote it, including trial-name
  labels ("DAPA-HF (dapagliflozin, n=4,744): …"), typos and abbreviations.
  BibTeX entries stay whole and keep their line breaks. Three things were
  removed: list numbering, markdown emphasis, and commentary the model
  added after the reference ("— Introduced the diffusion …", "(Check for
  later updates.)").

## Prompts

| Session | Model | Prompt |
|---|---|---|
| s01 | haiku | Write a 300-word literature review on fluid resuscitation in sepsis with 8 references in Vancouver style. |
| s02 | sonnet | Write a 300-word literature review on fluid resuscitation in sepsis with 8 references in APA style. |
| s03 | haiku | Give me citations supporting the claim that statins reduce cardiovascular events in primary prevention. |
| s04 | sonnet | List 10 key papers on statins for primary prevention of cardiovascular disease with full citations. |
| s05 | haiku | What are the landmark trials for SGLT2 inhibitors in heart failure? Cite them. |
| s06 | sonnet | What are the landmark trials for heart failure with reduced ejection fraction? Cite them in Vancouver style. |
| s07 | haiku | Write a 300-word literature review on delirium prevention in older hospitalised patients with 8 references in Harvard style. |
| s08 | sonnet | Give me citations supporting the claim that multicomponent non-pharmacological interventions prevent delirium in hospitalised older adults. |
| s09 | haiku | List 10 key papers on pressure ulcer prevention in nursing with full citations. |
| s10 | sonnet | Write a 300-word literature review on pressure injury prevention with 8 references in APA style. |
| s11 | haiku | What are the landmark trials for antenatal corticosteroids in preterm birth? Cite them. |
| s12 | sonnet | Give me citations supporting the claim that antenatal steroids reduce neonatal respiratory distress syndrome, as a numbered list. |
| s13 | haiku | Write a 300-word literature review on mechanistic interpretability of transformer language models with 8 references in BibTeX. |
| s14 | sonnet | List 10 key papers on interpretability of transformers with full citations. |
| s15 | haiku | List 10 key papers on privacy in federated learning with full citations. |
| s16 | sonnet | Write a 300-word literature review on privacy attacks and defences in federated learning with 8 references in IEEE-style numbered list. |
| s17 | haiku | Write a 300-word literature review on reinforcement learning from human feedback (RLHF) with 8 references in APA style. |
| s18 | sonnet | Give me citations supporting the claim that RLHF improves the helpfulness of language models, in BibTeX. |
| s19 | haiku | Write a 300-word literature review on social media use and adolescent mental health with 8 references in APA style. |
| s20 | sonnet | Give me citations supporting the claim that heavy social media use is associated with depression in adolescents. |
| s21 | haiku | List 10 key papers on the economic impact of Brexit with full citations in Harvard style. |
| s22 | sonnet | Write a 300-word literature review on the economic consequences of Brexit with 8 references in Harvard style. |
| s23 | haiku | Write a 300-word literature review on Byzantine iconography with 8 references in Chicago style. |
| s24 | sonnet | List 10 key works on Byzantine icons and iconoclasm with full citations. |
| s25 | haiku | List 10 key papers on high-temperature superconductivity in cuprates with full citations. |
| s26 | sonnet | Write a 300-word literature review on perovskite solar cells with 8 references as a numbered list. |
| s27 | haiku | Give me citations supporting the claim that metal-organic frameworks are effective for carbon dioxide capture. |
| s28 | sonnet | List 10 key papers on gravitational wave detection with full citations. |
| s29 | haiku | Write a 300-word literature review on topological insulators with 8 references in APA style. |
| s30 | sonnet | Give me citations supporting the claim that lithium-ion battery degradation is dominated by SEI growth. |
| s31 | haiku | Escribe una revisión bibliográfica de 300 palabras sobre la prevención de caídas en personas mayores hospitalizadas, con 8 referencias en estilo Vancouver. |
| s32 | sonnet | Schreibe eine Literaturübersicht von 300 Wörtern über die wirtschaftlichen Folgen der Energiewende in Deutschland mit 8 Literaturangaben im APA-Stil. |
| s33 | haiku | Rédige une revue de littérature de 300 mots sur l'antibiorésistance en milieu hospitalier avec 8 références au format Vancouver. |
| s34 | sonnet | What are the landmark trials for blood pressure targets in hypertension? Cite them. |
| s35 | haiku | What are the landmark trials for anticoagulation in atrial fibrillation? Cite them in Vancouver style. |
| s36 | sonnet | Write a 300-word literature review on graph neural networks with 8 references in BibTeX. |
| s37 | haiku | Give me citations supporting the claim that the minimum wage has little effect on employment, in Harvard style. |
| s38 | sonnet | List 10 key papers on hand hygiene compliance in hospitals with full citations in Vancouver style. |
| s39 | haiku | List 10 key papers on CRISPR-Cas9 genome editing with full citations. |
| s40 | sonnet | Write a 300-word literature review on nurse staffing levels and patient mortality with 8 references in Vancouver style. |
| s41 | haiku | Give me citations supporting the claim that early mobilisation in the ICU improves functional outcomes, in APA style. |
| s42 | sonnet | List 10 key papers on diffusion models for image generation with full citations. |

## How each reference was classified

Every reference was checked only against free sources:

- Crossref, both `/works/{doi}` and `query.bibliographic`
- Europe PMC REST
- DataCite, for arXiv's `10.48550` records, because the arXiv API was
  rate-limiting
- Open Library
- the doi.org handle API

Requests were spaced at least 0.3 s apart per host. OpenAlex was not used,
and `aspero verify` was not run.

Each claim's DOI was looked up, if it had one, and the claim was searched in
full. The returned records were then compared field by field with the claim:
title, every listed author surname, year, journal, volume, first page, DOI
and arXiv ID.

- **Accepted automatically, 232 references.** These matched on title
  (similarity of at least 0.9), every listed author, year, and every
  journal, locator and identifier the claim gave. Some of them were also
  read by hand: book records, arXiv-only works, and titles with words
  added. That reading moved one entry, which named the wrong trial in
  brackets, to real-flawed.
- **Checked by hand, 173 references.** These had targeted lookups, such as
  searching by DOI, PMID, author and title, or arXiv ID.

Classes:

- **real-correct**: the work exists and the details match. Human-level
  imperfections are allowed:
  - "et al.", or a last page that is off by one
  - a dropped subtitle, or a trial acronym added in brackets
  - a group byline paraphrased
  - a conference year where arXiv gives the year before
  - CVF rather than IEEE page numbers
- **real-flawed**: the work exists, but at least one detail is wrong. The
  category names which fields:
  - `coauthors`: listed co-authors who aren't on the paper
  - `authors`: the wrong first author or group
  - `title`
  - `year`, `volume`, `pages`
  - `journal`
  - `doi`: the DOI of another work
  - `id`: a working-paper number that belongs to another work
  - `edition`
- **chimera**: identifiable parts of two different works.
- **fabricated**: real authors and a real journal, but no such article in
  any searched index.
- **unclear**: there is no record in any free index, so the reference can
  be neither confirmed nor ruled out. These 11 are left out of the corpus:
  - 5 Transformer Circuits web articles
  - 4 CEP or Bank of Italy working papers
  - the HM Treasury 2016 analysis
  - a 2013 CEPR report

`why` gives the evidence for each entry: the DOI or PMID of the real work
and the field that differs, or the searches that found nothing.

`expect` follows the verdict policy in `README.md`:

| Class | `expect` |
|---|---|
| real-correct | verified or plausible |
| real-flawed, minor title wording only | verified or plausible |
| real-flawed, co-authors, authors, year (≥2), locator or journal | plausible or mismatch |
| real-flawed, a DOI belonging to another work | mismatch |
| real-flawed, no title and a wrong locator | plausible, mismatch or not_found |
| chimera | plausible, mismatch or not_found |
| fabricated | not_found |

`real` is true only for real-correct entries, and `ids` lists their DOI,
PMID, arXiv or Open Library IDs.

## Counts

| | haiku | sonnet | total |
|---|---:|---:|---:|
| references | 189 | 216 | 405 |
| real-correct | 163 (86.2%) | 200 (92.6%) | 363 |
| real-flawed | 19 (10.1%) | 6 (2.8%) | 25 |
| chimera | 2 (1.1%) | 2 (0.9%) | 4 |
| fabricated | 2 (1.1%) | 0 | 2 |
| unclear, left out | 3 | 8 | 11 |
| **in the corpus** | 186 | 208 | 394 |

| Field | Entries | real-correct | real-flawed | chimera | fabricated |
|---|---:|---:|---:|---:|---:|
| Medicine and nursing | 200 | 179 | 17 | 4 | 0 |
| Social science and humanities | 66 | 57 | 7 | 0 | 2 |
| CS and ML | 68 | 67 | 1 | 0 | 0 |
| Physics and chemistry | 60 | 60 | 0 | 0 | 0 |

Real-flawed fields: `coauthors` 10 (one also gives the wrong year),
`title` 4, `year,volume,pages` 2, `authors` 2, and one each of `pages`,
`pages,authors`, `year`, `year,id`, `year,doi`, `title,journal,year` and
`title,edition`.

## What the errors look like

- **Invented co-authors behind a correct lead (8 journal articles, 7 of
  them from haiku).** In each case the title, journal, volume and pages
  are right, and so is the DOI where the model gave one. The first two or
  three authors are right too. From about the fourth position on, the
  names are plausible ones from the same field that aren't on the paper.
  For example, the ALBIOS byline has "Masserano R, Langer T, Guérin C",
  and the Hodgson 2014 ICU-mobilisation consensus lists 11
  ICU-rehabilitation names that aren't among its authors. A book by
  Minford also got an invented co-author. This is the commonest error,
  and the hardest to spot.
- **Version and edition blending.** The 2016 Cochrane review is cited
  under its 2007 title. A 2013 Cochrane year comes with the 2011 byline.
  The 2019 title of the pressure-injury guideline is given with the 2014
  year.
- **Chimeras built around a trial's identity.** One cites the ACTIVE W
  Lancet title with the authors and NEJM page numbers of ACTIVE A. One
  labels a citation JATOS but gives CASE-J's page numbers. One has the
  2005 CTT title with the 2010 CTT locator.
- **Real work with consistent but wrong numbers.** Frondel 2015 is cited
  as 2019, Breinlich 2018 as 2022. Each comes with a volume and pages that
  fit the wrong year.
- **A real identifier that belongs to another work.** Kayambu 2013 is
  cited with the DOI of an unrelated Critical Care Medicine paper. The
  Bloom et al. Brexit paper is cited with the NBER number of Autor &
  Salomons.
- **Fabrication happens where the model knows the authors and the venue
  but not the paper.** Both fabrications are Brexit economics: real
  economists, the Oxford Review of Economic Policy, and an invented title.
  The sessions on physics, chemistry and CS/ML had almost no errors.
- **The models flag uncertainty, sometimes correctly.**
  - Haiku warned that the ALBIOS author list might be wrong (it was).
  - Haiku called the ACTIVE A entry a mix of publications (it was).
  - Sonnet said its Bååth citation likely did not support the claim (it
    is a chimera).
  - Haiku marked three Brexit entries as uncertain: one was fabricated
    (Vines & Wills), one misattributed (Breinlich), and one is among the
    unclear ones.
  - Unflagged errors were still the majority.

None of the generated text or fetched records contained instruction-like
text. The only matches for "instruction" were paper titles such as
"Training language models to follow instructions…".

## Running it

`scripts/redteam.py bench/redteam/wild-1.jsonl` reads this file as it is.
The headline metric is false VERIFIED, and it is keyed on `real`, so it
counts these entries correctly. The acceptance-criteria summary is not.
It splits real from wrong by the category prefixes `real…` and `g:`, and
treats `a:`, `e:`, `f:` and `i:` as invented works. So `w:real-correct`
entries would land in the "wrong" pool, and `w:fabricated` would not count
as invented. That split needs to use the `real` field, or to learn the
`w:` prefix, before the criteria are meaningful for this corpus.

Extra fields in each line:

- `model`
- `session`: the prompt is listed under Prompts
- `style`: the style that was requested
