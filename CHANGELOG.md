# Changelog

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
