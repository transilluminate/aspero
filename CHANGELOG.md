# Changelog

## Unreleased

Added:

- `Verification.read`: the claim as aspero read it, for a "cited vs
  found" view. Identifiers as written (a DOI cut short at a line break
  stays visible, with the readings tried as `alternatives`); from free
  text, the year (when it gives one), volume and first page
  (`Claim::as_read`). Title, authors and journal aren't parsed out of
  free text: they're scored by finding the work's own in it.
- `Difference.cited`: the citation's value for the field, as compared
  (the year nearest the work's, the volume or first page read, or the
  title, authors or journal given).
- JSON: `"read"` on each verification, `"cited"` in `why.differs`. FFI:
  `Verification.read: Claim?`, `Difference.cited: String?` (#46).

## 0.8.4

Fixed:

- An unnumbered reference whose authors fill its first line (ending
  "et al.") was split from its title line when that started like an
  author ("National, regional, and global trends…"). A surname's comma
  now needs a capital after it, and a reference so far without a year or
  identifier doesn't end there (#44).

## 0.8.3

Added:

- `Person::parts` (family and given names, split from a name written
  whole with the rules BibTeX export used, `None` for an organisation),
  `Person::initials` ("DV") and `Person::short`, the Vancouver form
  ("Laan DV"). FFI: `Author.family`, `Author.given` and `Author.short`;
  an `Author` passed back to `export` or `cite` with a family name keeps
  its parts (#41).

Fixed:

- A reference list stayed one reference when each DOI's end was wrapped
  onto a line of its own (`…/CIR.000000000` then `0000659`). A line that
  is one word of DOI-suffix characters with a digit, not a plain number
  or an identifier of its own, continues the link that ended the line
  before, so the reference ends in the whole DOI and `scan` reads it
  whole (#40).

## 0.8.2

Fixed:

- A DOI a PDF wrapped part-way through its suffix (`…/HJH.0b013e` then
  `3282f35c67`) was looked up as printed and reported as not existing,
  a red mismatch against the very work found. A DOI ending its line now
  offers the next line's first word joined to it as an alternative
  reading, `Claim::from_text` carries mentions' alternatives, and a cited
  DOI that doesn't exist but starts the found work's DOI is that DOI cut
  short (#39).
- An unnumbered reference list whose lines break with a lone `\r` or
  U+2028/U+2029 (as some PDF copies do) stayed one reference: those
  breaks are now newlines. Laid-out lines (`split_lines`) no longer start
  a reference at a line in lower case or closing a bracket, or after a
  word broken at a hyphen (#40).

## 0.8.1

Added:

- `WorkKind` (reply, correction, comment, retraction, expression of
  concern, recommendation): a `CitedField::Kind` difference's `kind`, the
  value `actual` words, so front ends can match on it exhaustively.
  `matching::derivative_work_kind` returns it. FFI: `Difference.kind:
  WorkKind?`.

## 0.8.0

Added:

- `identify`: which work a document is, from its first page's text, not
  the works it cites or links to. Each identifier on the page (header and
  footer first, at most six) is looked up and must be confirmed by the
  title given (the largest type on the page, or title metadata unless
  junk such as "Microsoft Word - draft3.docx") or by its own title and
  first author appearing on the page; with none confirmed, the top of the
  page is verified as a citation. It answers with the verification and
  `decided_by`: `identifier_and_title`, `identifier_and_page`,
  `only_identifier` or `title_search` (#24).
  - FFI: `identify(firstPage:, title:, titleHint:, patience:)` →
    `Identification? { decidedBy: IdentifiedBy, verification }`.
  - CLI: `aspero identify page.txt --title … --title-hint …` (exits 3
    when nothing is sure enough).

Fixed:

- `aspero scan --help` no longer says a DOI broken across lines isn't
  found.

## 0.7.0

Added:

- Citations without a title are found by where they appeared: running
  headers (`Injury, Int. J. Care Injured 47 (2016) 797–804`, `Nature 521,
  436–444 (2015)`) and short forms (`Injury. 2016;47(4):797-804.`).
  Journal, volume and first page agreeing verify; another page or volume
  is not found, not a guess. `verify` searches OpenAlex by volume, first
  page and year (±1) for them (`SearchQuery::place`, `Filter::Place`;
  found-by `place_search`) (#23).
- `Reason::Unconfirmed` (`"unconfirmed"`): the citation agrees with the
  work as far as it goes, but gives too little to confirm it. Before, this
  was `partly_matches` with no differences.
- `CitedField::Kind` (`"kind"`): the closest work is a reply, correction or
  commentary on the one cited; `actual` says which ("reply",
  "correction", "comment", …). `matching::derivative_kind` names it.

Changed:

- `partly_matches`, `closest` and `id_partly_matches` always list at least
  one difference. A closest work differing in nothing nameable is
  `not_found`. **FFI**: `Reason.unconfirmed` and `CitedField.kind` are new
  cases.
- A reference without a title (authors, journal, year and an identifier,
  say) no longer counts as citing a different title, and one naming no
  authors no longer counts as naming others ("authors mismatch" on a
  running header).

## 0.6.7

Fixed:

- `scan` found nothing in a DOI link that macOS Preview copies with
  spaces after its punctuation (`https:// doi. org/ 10. 1183/ 23120 541.
  00489- 2021`); such a link is now mended, and `verify` of it alone
  checks the DOI (#33).
- `verify` of that link was a mismatch against a placeholder record
  titled with a URL. A URL is no title, and a link's words (`https`,
  `doi`, `org`, …) no evidence of one; a year or journal agreeing alone
  no longer makes a work plausible either (#33).
- `split_references` gave up splitting references numbered inline when
  one of them had no year ("Defense Health Board … Falls Church, VA,
  n.d."). `n.d.`, "Accessed …" and "[cited …]" now count as a date, and
  one undated piece among four or more no longer stops the split (#35).
- Words inside a title no longer mark it a reply or correction
  ("differential response to corticosteroids", "Correction of
  hyponatraemia"): "response to", "comment on" and "correction" count
  only where a title starts with them.

## 0.6.6

Fixed:

- `split_references` read a list copied from a PDF onto one line ("…
  01159.x. [21] Finucane … [22] …", as Preview copies) as one reference.
  A line is now split before inline `[n]` markers in sequence (three or
  more, or two from the line's start), each after a full stop and before
  a capital, when every piece has a year or identifier; text before the
  first number is kept as a reference when dated (#31).
- A title of one or two words found inside free text no longer makes a
  work plausible by itself or with only a year: `verify "aspero 0.6.5 is
  in"` matched an encyclopedia entry titled "Aspero". It takes the
  authors, the journal, the volume and page, or an identifier (#29).

## 0.6.5

Fixed:

- Titles and journal names from Crossref (and OpenAlex, DataCite,
  Europe PMC) kept their HTML entities and JATS tags (`Anesthesia &amp;
  Analgesia`, `<i>E. coli</i>`), into citations, BibTeX and RIS. They
  are plain text now. Abstracts no longer gain a space where an inline
  tag was (`H<sub>2</sub>O` is `H2O`), and a `<` that opens no tag
  (`age < 65`) is kept (#27).

## 0.6.4

Fixed:

- `scan` found nothing in a DOI link broken across lines right after
  the DOI's prefix (`http://dx.doi.org/10.1017/` then
  `S1049023X15004653.`), as PDFs break them. A link ending in `/` at the
  end of a line now continues onto the next (#25).

## 0.6.3

Changed:

- `scan` is about 100× faster: ~1 GB/s on prose, ~200 MB/s on dense
  reference lists (was ~5.5 MB/s), with the same results, and no longer
  needs 16 bytes of memory per byte scanned. `split_references`, and
  parsing identifiers, are faster too.

Fixed:

- `scan` joined a link ending a line to the start of the next line
  when that was another link (`https://…`, `www.…`, `arXiv:…`, a DOI),
  the next reference's number (`18.`), a hyphenated word or a number.
  A link continues onto the next line only if that keeps it the same
  kind of identifier and changes it.

## 0.6.2

Added:

- Word documents: the citations Zotero and Mendeley (Desktop) keep in a
  .docx file's field codes, read as claims with their identifiers, each
  work once, from the body, footnotes and endnotes. `aspero check
  paper.docx`; FFI `readDocx(data)` → `DocxReferences { entries,
  manager, citations, text }`, `text` being the document's paragraphs
  for splitting its reference list when it has no fields. Mendeley
  Cite's content controls and EndNote's fields aren't read yet.
  (`aspero-bib`'s `docx` feature.)

## 0.6.1

Added:

- Citation styles, offline, by the hayagriva CSL processor, with eight
  styles from the official CSL repository bundled: APA 7th, Vancouver
  (NLM), Harvard (Cite Them Right), Chicago author-date, IEEE, Nature,
  AMA and MLA. Any independent `.csl` style can be used too. Styles and
  locales are CC BY-SA 3.0 (`crates/bib/styles/NOTICE.md`).
  - CLI: `--style <id|file.csl>` (implies `--format cite`) and
    `--locale en-GB` on `lookup`, `search` and `verify`; `aspero styles`
    lists the bundled ones, `--credits` prints their license and authors.
  - FFI: `citationStyles()`, `cite(works, style: StyleSource, format:
    CiteFormat, locale:)` → `[CitedEntry { index, label, text, inText }]`
    in the style's order (`.text` or `.html`), `styleCredits()`; throws
    `StyleError` (`unknownStyle`, `dependent(parent)`, `invalid`).
  - `aspero-bib`'s `styles` feature: `cite::{Style, cite, credits,
    BUNDLED}`.

## 0.6.0

Added:

- `check` reads RIS and CSL-JSON as well as BibTeX, telling them apart
  from the contents, and compares each entry's journal, volume and
  pages too (BibTeX's `journal`/`journaltitle`, `volume`, `pages`).
  PMIDs are read from BibTeX `pmid` or `eprinttype = {pubmed}`.
- `--format bibtex|ris` on `lookup`, `search` and `verify`: the works
  found, for a reference manager. `verify` writes the work each citation
  refers to (verified, or found under other identifiers than those
  cited) and names the rest on stderr. Whole names ("Yann LeCun") are
  split into family and given names; organisations stay whole.
- `check`'s findings about an entry's own fields carry `field`, `value`
  and `problem` (`not_a_doi`, `not_an_arxiv_id`, `not_an_isbn`,
  `not_a_pmid`).
- FFI: `readReferences(text)` reads a BibTeX, RIS or CSL-JSON file into
  `Bibliography { format, entries }`, each entry's `claim` ready to
  verify with `key` set to the entry key, and its `notes` as `BibNote {
  field, value, problem, message }`; it throws `ReadError.unrecognised`
  or `.invalid(format, line, message)`. `export(works, format)` writes
  works as BibTeX or RIS.

Changed:

- A citation whose identifier is the right work, with the same title and
  matching authors but a wrong year, is `plausible` /
  `id_partly_matches` naming the year, not `mismatch` / `id_disagrees`.
  `check` still reports a year off by more than one as an error.

Changed (breaking):

- `aspero-bib`: `BibError` is now `ReadError` (`Unrecognised`, or
  `Invalid { format, line, message }`); new `read`, `detect`,
  `parse_ris`, `parse_csl_json`, `bibtex`, `ris`, `cite_key`, `Keys`.
  `BibEntry::notes` are `Note { field, value, problem }`.

## 0.5.2

Faster:

- `verify` stops waiting for the remaining title and keyword searches as
  soon as a candidate verifies, instead of waiting for the slowest
  provider (Europe PMC's search can take 3–11 s). A citation whose DOI
  names another work now settles once two providers agree on the work
  it cites: 11–17 s → 3–5 s live; a citation without an identifier, 4–17 s
  → 2.5–4.5 s.

## 0.5.1

Added:

- `scan`, `split_references` and identifier parsing clean text from PDFs
  and word processors first: ligatures (`ﬁ`) expanded, soft hyphens and
  zero-width characters dropped (a soft hyphen at a line end marks a
  broken word), Unicode hyphens, dashes, quotes and spaces folded to
  ASCII. Spans still point into the text as given.
- A DOI broken across lines at a hyphen is kept as printed, and its
  reading without the hyphen is tried if the printed one doesn't exist:
  `Claim::alternatives`, `Reference::alternatives`,
  `Mention::alternative` (JSON `scan`: `alternative`; FFI:
  `alternativeIds` on `Claim` and `Reference`, `alternativeId` on
  `Mention`). `verify --split` passes them on.
- `split_lines` (FFI `splitLines`): splits a reference list's lines as
  laid out, using their indents (hanging or first-line) where the text
  alone can't tell; each reference gives its range of line indices.

Faster:

- `verify` no longer waits for citation matchers (Crossref's takes
  seconds) once the claim's identifiers alone verify it: a cold verify
  of a correct citation with its DOI takes about 0.4 s instead of 2–4 s.
- Lookups stop at a final "not found" (doi.org for DOIs): a DOI that
  doesn't exist is no longer searched for at Europe PMC, which took up
  to 5 s.
- Crossref gets one request in flight at a time without a contact
  address (its public pool refuses a second with 429), a few with one.
- A DOI's alternative reading is looked up alongside the printed one in
  `verify`'s first stage instead of before it.

Fixed:

- A lone numbered reference (`1. LeCun Y. …`) loses its number.
- `scan` rejoins a DOI or URL broken across lines (as in text from PDFs)
  by the rules `split_references` uses; the mention spans both lines.
  A link split mid-word is rejoined only before more of a link, never
  before a word or a year.

## 0.5.0

Breaking:

- A lookup that is offline and not in the cache has status `not_cached`
  (it was `failed`).
- `Verification` is `#[non_exhaustive]` and gains `why` and `notes`.
- FFI `verify` and `verify_each` take typed `Claim`s (text and/or fields,
  and a `key`) instead of strings.
- FFI calls return typed records instead of JSON strings: `lookup` →
  `Lookups`, `verify` → `Verifications`, `verify_each` hands the listener
  a `Verification` (with its claim's `index`) and returns usage, `scan` →
  `[Mention]`, `unavailable` → `[Unavailable]`. `AsperoError` gains
  `Cache`.

Added:

- `aspero verify --split` (and `resolve`) takes a pasted reference list
  (numbered, bulleted, one per paragraph or line, wrapped lines rejoined,
  DOIs and URLs broken by PDF extraction mended) and checks each
  reference; `split_references` in `aspero-core`, and
  `splitReferences` over FFI with UTF-16 ranges.
- `Engine::budgets()`: each provider's budget as its last response
  reported it, so it can be shown without spending any.
- FFI: `search`, `budgets`, `checkOpenalexKey` (one free lookup: valid,
  rejected or unreachable, and what's left), `cacheStats` and
  `clearCache`.
- Verdicts say why as data as well as in English: `Verification::why`
  (a `Reason`, with the fields that differ and their values) and `notes`
  (retraction, newer version). JSON adds `why: {code, differs, cited}`.
- Errors have a fine-grained `ErrorKind` (`rate_limited`,
  `budget_spent`, `key_rejected`, `not_cached`, …) beside their category.
  JSON adds `kind` to failed attempts and `error_kinds` to
  verifications.
- A verification's `errors` include failed lookups of its identifiers, not
  only failed searches.

Fixed:

- Labelled identifiers glued to a colon (`ISBN-13:979…`, `PMID:123`)
  are found by `scan` and parse as `scan` finds them.

## 0.4.1

- Blank claims (`""`, whitespace, or JSON with only blank fields) are
  `invalid` instead of being searched for.
- FFI: `lookup`, `verify` and `verify_each` take an optional `cancel:
  CallToken`; cancelling it aborts the call, which throws the new
  `AsperoError.Cancelled`. Swift `Task` cancellation doesn't reach Rust
  through UniFFI 0.32, so 0.4.0's claim that it did was wrong.
- FFI: `scan` mentions add `utf16: {start, end}` for `NSString` ranges.
- The xcframework is Apple silicon only and built for macOS 26.0
  (`UNIVERSAL=1` and `MACOSX_DEPLOYMENT_TARGET` for other builds),
  which also silences linker warnings about newer macOS versions.

## 0.4.0

Breaking:

- `verify --format jsonl` prints each result as it finishes, in any order
  (match by `input` or `key`); a slow claim no longer holds up the rest.
  Other formats keep input order. `Engine::verify_many` yields
  `(index, verification)` as each completes.
- The JSON contract moves to a new crate, `aspero-json`.

Added:

- A long-lived engine (for apps and parallel agents): lookups already in
  flight are shared rather than repeated; `CallOptions` set a caller's
  patience (`interactive()`: give up within seconds, `offline()`: cache
  only); a spent quota, rejected key or failing provider (circuit
  breaker) is skipped across calls, and `Engine::unavailable` says which
  and why.
- Parallel aspero processes (agents) share each provider's rate limit
  through the cache file, even with `--no-cache`.
- `aspero-ffi`: Swift bindings (UniFFI) for apps, answering in the same
  JSON, with results streamed as they finish; `AsperoFFI.xcframework.zip`
  and `aspero_ffi.swift` are attached to each release.
- `Engine::lookup_inputs`: identifiers as written, with `Extras`, so every
  front end answers as `aspero lookup` does.
- Help: where notice dates come from, and why they can differ from a plain
  lookup of the notice DOI.

## 0.3.4

- `canonical` on every work in JSON: one key per work whichever identifier
  found it (DOI, PMID or PMCID lookups of one work agree), for deduping.
- `lookup --full`: notices are dated by when they appeared (earliest of
  online and print), with the source's date kept as `deposited` when later;
  notices themselves are typed `erratum` or `retraction`; and a record
  that contradicts the DOI's registry (Crossref) gains `disagreements`, as
  when an aggregator merged two notices into one record. Contradicting
  records aren't used to date notices.

## 0.3.3

- Publisher URLs with a DOI in the path give that DOI, without landing-page
  suffixes (`/full`, `/abstract`, `/pdf`, `.pdf`, …): in `scan`, and as
  `lookup` input. doi.org URLs are taken as they are.
- A retraction or withdrawal notice that OpenAlex flags as retracted names
  itself as the `notice` (with `lookup --full`), so rows about one event
  share a `notice` to dedupe on.
- Help: `scan`'s `id` joins to `lookup`'s; `open_access` "closed" means no
  free copy is known to the source.

## 0.3.2

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
