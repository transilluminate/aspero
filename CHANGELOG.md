# Changelog

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
