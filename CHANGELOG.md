# Changelog

## Unreleased

- Newer versions: Crossref `new_version` notices (Cochrane reviews,
  `.pub6` → `.pub7`) and `is-replaced-by`/`is-preprint-of` relations
  become `new_version` updates. `lookup --full` follows the chain and adds
  `latest`; `verify` notes "superseded by …"; `check` warns.

## 0.3.1

- `verify` and `resolve` check the journal (abbreviations and initials
  included: N Engl J Med, NEJM), volume and first page, from claim fields
  or Vancouver/APA-style text. A wrong volume or page makes a match
  plausible, with the reason naming the right one; short references
  without a title ("Daum RS et al. N Engl J Med 2017;376:2545") verify.
  `checks` gains `container`, `volume` and `page`.
- Structured claims: JSON lines take `journal` (or `container`), `volume`,
  `page`, and a `key` echoed back untouched; results carry the `claim` as
  understood. `--journal`, `--volume`, `--page` flags.
- Plausible verdicts say which details differ.

## 0.3.0

- **Breaking (library):** `IdKind::ALL` and `Field::ALL` are slices, so
  future kinds and fields aren't breaking changes. (They were arrays whose
  length the new NBK kind and open-access field changed.)
- `open_access` on works: whether a free, legal copy exists, its PDF and
  page, licence, version and Unpaywall class (gold, diamond, hybrid,
  bronze, green, closed), from OpenAlex and Europe PMC. Lookup tables gain
  a "Free" column; text reports a "Free copy" line. `lookup --full` fills
  it from other providers when the first didn't say.
- `aspero scan`: every identifier mentioned in text or Markdown files, with
  line, column, the text as written and context, as JSON(L) or a table.
  Offline. `lookup -` reads its JSON lines and echoes each as `from`.
- NCBI Bookshelf IDs (`NBK526056`, Bookshelf URLs), looked up via Europe
  PMC: StatPearls chapters with title, series and date.
- Identifiers in claims and BibTeX URLs are found by the same scanner:
  PMCIDs, NBK IDs and labelled PMIDs now count; brackets inside DOIs stay.

## 0.2.0

- **Breaking (JSON):** `authors` is a list of objects in every mode, each
  with a `name` (and `orcid` when known); `--full` adds `given` and
  `family`. Lean output used to give plain strings.
- `update_to` on notices: what an erratum corrects or a retraction notice
  retracts (Crossref `update-to`). `lookup --full` asks Crossref for
  notices in both directions when another provider answered.
- Verification: generic titles with only a year stay plausible; replies,
  errata and comments no longer pass for the original; online and print
  years both match; ties go to the earliest published; `verify` stops
  early only on a match its identifiers or two providers vouch for.
- Identifiers pasted from prose lose wrapping brackets, quotes, asterisks
  and trailing punctuation; balanced brackets inside DOIs stay.
- Batch exit codes documented.

## 0.1.0

First release.

- `verify` citations (free text, fields, JSON lines, batches): verified,
  plausible, mismatch (wrong or nonexistent identifier, wrong year or
  authors, with the right work named), not found; retractions noted.
- `check` BibTeX/BibLaTeX bibliographies, CI-style: missing and wrong DOIs,
  year corrections, duplicates, retracted works; `--fail-on`.
- `lookup` DOIs, arXiv IDs, PMIDs, PMCIDs, ISBNs and their URLs; `search`
  one provider with year/type/open-access filters and cursor paging;
  `resolve` messy citations to scored candidates; `--abstract` everywhere.
- Providers: OpenAlex, Crossref, DataCite, doi.org, Europe PMC, Open
  Library. Lookups fall back provider by provider; independent searches
  run concurrently; per-provider pacing, retries and backoff.
- SQLite answer cache with stale-on-error; `cache stats|clear|path`.
- Tables on a terminal, JSON when piped (`--format table|text|json|jsonl`);
  `-v` shows every request.
