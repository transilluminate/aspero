# Changelog

Releases 0.10 to 0.19 are in [docs/changelog/0.1x.md](docs/changelog/0.1x.md),
and 0.1 to 0.9 in [docs/changelog/0.0x.md](docs/changelog/0.0x.md).

## Unreleased

Fixed:

- A book's chapter cited as a journal's article, its book named as the
  journal and an invented volume given as a journal's, without an issue
  ("Nagatomo, J. (2014). Life after Migration. Migration as Transnational
  Leisure, 33, 128–178."), is plausible, its volume named: a chapter is in
  no volume of that shape (holdout-6 `w:chapter-as-article`). A series'
  chapter at its real volume still verifies: labelled ("Lecture Notes in
  Computer Science, vol. 9351"), named as a series ("Advances in
  Experimental Social Psychology, 35, 53–152"), or named by its DOI. A
  chapter's record placed in its publisher's collection ("BRILL eBooks",
  OpenAlex's) is placed in its book when another record of it names the
  book, so the venue cited is checked whichever record is kept.
- A retracted article cited by its title, matched as well by its
  retraction notice titled the same (Taylor & Francis's are), verified as
  the notice, without saying the work was retracted (holdout-1 #721). The
  article itself is now the match, with its retraction; when only the
  notice is found, it carries the retraction, naming itself as the
  notice. A claim citing the notice ("Retraction of: …") is unchanged.

## 0.39.1

Fixed:

- Two panics in 0.39.0, found by fuzzing, both on text of other
  scripts where an ASCII form was expected: a bracketed year whose first
  four bytes split a character ("(200\u{ad}9)", a soft hyphen) after a
  byline and title, and a record's title ending in a dash and "edition"
  ("… — edition"). Either stopped a `verify` of the claim (as an
  `internal_error` since 0.39.0, not the whole batch).
- An abstract OpenAlex read off a publisher's landing page, its author
  list, affiliations and links and all (Wiley's, the ACP journals',
  "Volume 154, Issue S1 p. 3-31 … Search for more papers by this
  author"), is no abstract: the work has none from OpenAlex, and another
  source's is used if one has it. A page footer run into a real abstract
  ("…Received 18 September 1952DOI:https://doi.org/…©1952 American
  Physical Society") is cut off. About 1 in 1,000 of OpenAlex's
  abstracts on the red team's tape.

## 0.39.0

Changed:

- `aspero lookup`'s text says when a free copy is a page with no file
  link known ("Free copy (green): … (a page; no file link known)"): a
  repository's record may hold no file. `lookup --help` now says so for
  the JSON (`open_access` without `pdf_url`), and that a work's date is
  `issued` ("2016-07-04"; there is no separate `year`).
- A bug that panics while verifying one claim no longer stops the rest
  of a batch: that claim is `unverifiable`, with the new reason
  `internal_error` and the panic's message in `reason_text` (please
  report it), and the others are verified as usual. The CLI's
  `aspero verify` batches, the MCP `verify` tool and an embedder's batch
  call all get this. In Swift, `Reason` gains the case `internalError`,
  so a `switch` over it without `default` needs one more arm; a stored
  reason reads back as it was written.

Fixed:

- A cited identifier that is the work's, beside its authors and a year
  that isn't wrong, is plausible (the title to check) when the words
  where its title would stand are something else: a trial's label
  ("AReSVi-006 (GSK, RSVPreF3 OA): …"), a description, or a short form
  ("Hash Layers (Roller et al.) | 2106.04426"). It was a mismatch: the
  work's title, looked for in the text, wasn't there (wild-4, 20 rows).
- A cited identifier and the exact place it names (journal, volume and
  first page) that agree are plausible when the title or the byline
  differs ("Observation of ttH production" for "… tt¯H …"), unless both
  differ. A lookalike with another identifier or page (a talk on the same
  result) no longer outvotes them as the work "the citation matches".
- A title cited with the work's own identifier and nothing else
  ("MoE-Mamba: … . arXiv:2401.04081") verifies, as the identifier alone
  does. The identifier now counts as a second field when everything else
  cited agrees; it was held at plausible ("gives too little to confirm
  it"). A title alone, a book's say, still can't verify.
- A title found in free text only as a word or two is no full match unless
  the authors, an identifier, the journal or the place say which work it
  is: "Iceland" of "Iceland: A Peaceful Secession" inside "The History of
  Iceland", or a one-word title ("13th"). Such a work was reported as the
  closest, a mismatch ("its authors differs"); now nothing is found.
- A title's opening words followed by "…" or "..." ("Scaling Rectified
  Flow Transformers… (Stable Diffusion 3)") partly match the work's title
  instead of differing from it.
- A book cited, its chapter's record found: a chapter whose book is the
  title cited, cited without "In:", editors or a page, is part of the
  work and no longer verifies as it ("The Priests" for "The
  Christianization of Iceland: Priests, Power, and Social Change"), so
  the book's own record ranks first.
- A bare ISBN-13 in a reference, without "ISBN" before it, is read as
  the book's ISBN: 13 digits, hyphens allowed, starting 978 or 979 and
  with a valid check digit, as models write them in a table's cell
  ("… | 15th (2020) | 978-1260452310"). ISBN-10s are still read only
  when labelled. `scan`'s mentions are unchanged: this is read only
  when a reference is read as a claim.
- An NBER working paper's number is read as its DOI
  (`10.3386/w<number>`) when NBER is named beside it ("NBER Working
  Paper w31113", "NBER Working Paper No. 16449", "NBER WP 9804"), and
  the reference gives no DOI. A wrong number then finds the other paper
  it names, instead of the cited title verifying alone (wild-4 #723).
- A journal's article cited as a working paper ("… NBER Working
  Paper.", without a number) is another version of the one cited:
  plausible, as for a preprint, not verified. A record whose venue is
  a working-paper series is still not read as a preprint server's.
- The title searched for a free-text reference no citation matcher
  found leaves out more of what isn't its title: a table row's other
  cells (its first cell is the title), a byline and year before it
  ("WHO (2003) Kangaroo Mother Care …"), a name written out before a
  comma ("Einar Arnórsson, Réttarsaga Alþingis (1945)"), a note in
  brackets ending it ("(Pearson)", "(2004, in Icelandic)"; one in
  capitals, such as "(MERS)" or "(JNC 8)", stays), a place and
  publisher ("Atlanta: Scholars Press, 1998"), and a series' volume
  ("Writings from the Ancient World 2") after the book's own title; a
  link ("https://doi.org/…") no longer outranks the title as a
  sentence. Open Library finds none of these books by the longer
  query.
- A model-written list's furniture is no longer read as a reference's
  title or authors: a label opening it, a trial's or a model's name
  before the byline ("DAPA-HF: McMurray JJV, …", "BaSICS (Finfer S et
  al. …", "Aktueller Überblick: S. Pirandola et al. …"), and commentary
  after the volume and page it cites, in its sentence or a later table
  cell ("… 39(11):2296-303 | Cluster RCT in Danish players. …"). Such a
  reference at the work's place, with its authors, was plausible for a
  title it never gave; it is verified.
- A journal's series letter is no author's initial: "Physics Letters
  B", "Physical Review D" or "Journal of Materials Chemistry A" in a
  reference no longer reads as a family name and an initial, naming
  someone the record lacks; nor is a model's "Title:" field label a
  name.
- A record's title is compared without the markup and furniture a
  provider writes into it, where the citation doesn't give them too:
  tags (`<em>…</em>`), math markup (`$$\sqrt{s}=13\,\text {Te}\text
  {V} $$`) and an overbar ("tt¯H") read as the letters they mark; an
  edition (", 14th Edition"), a code in brackets ("(MPB-35)"), a
  library's statement of who wrote it ("/ Elaine N. Marieb, Katja
  Hoehn") and a book's series in brackets ("(Verso Classics, 10)") are
  left out. Such books and papers were found but only partly matched, or
  lost to another work (wild-4 #319, #343, #367, #369, #455, #540, #543,
  #804; wild-3 #761).
- Journals told in more of the ways they're named: OpenAlex's name
  doubled over a slash ("Physical review. D/Physical review. D.") is
  read as each of its names, so "PRD" is Physical Review D; one word
  cited that is the initials of a run of the journal's words ("PRD" of
  "Physical Review D: Particles, Fields, …") is no other journal; an
  acronym of four letters or more written with each word's first letter
  or two ("TREE" for Trends in Ecology & Evolution) is the journal; "AER:
  Papers & Proceedings" at the American Economic Review's own volume and
  page is that journal; and OpenAlex's name for ACL's proceedings
  ("Proceedings of the conference - Association for Computational
  Linguistics. Meeting") is ACL's (wild-4 #377, #382, #469, #485, #595,
  #748; wild-2 #790).
- A book's own title, as its reference's shape suggests, is searched
  beside a citation matcher's hit that shares only a fragment of it (a
  one-word review titled "Wilding", for "Wilding: The return of nature
  to a British farm"), when that hit is the only pointer and the
  reference has no identifier or volume and page to look it up by: one
  more title search for such references. A hit sharing exactly that
  title is searched first, though it came third behind hits sharing it
  and more (wild-4 #574).
- A DOI whose registry (Crossref) contradicts OpenAlex's record of it on
  two of date, volume and page now gets the registry's record (#183).
  OpenAlex sometimes merges two works into one: it has two corrections to
  one guideline, 10.1111/1471-0528.18258 and 10.1111/1471-0528.70298, as
  one work, with the second's DOI and the first's date, volume, page and
  PDF. `lookup --full` of the second now serves Crossref's record (2026,
  133, 1823), with none of the merged record's other identifiers, PDF or
  abstract, and still lists the `disagreements`. A lookup of the first no
  longer carries the second's DOI, nor PMIDs and PMCIDs the registry
  doesn't confirm. `verify` no longer merges such a record with the
  registry's: a citation of the 2026 correction verifies as 2026, not
  2025. Records that differ on one field only (an online-first year, an
  article number for a page) and other providers' records merge as
  before.
- A plain `lookup` (without `--full`) now does the same for a record that
  looks like two works merged into one, so `aspero lookup
  10.1111/1471-0528.70298` serves Crossref's record too (#183). OpenAlex's
  record looks merged when its locations name two PubMed records, or
  another DOI of the same publisher's, or when it was found by a DOI that
  isn't its own. A preprint's or a dataset's DOI beside the article's,
  and one DOI spelled two ways, don't count. Such a lookup costs one
  Crossref request more, and lists the `disagreements` it finds. About 2%
  of OpenAlex's DOI lookups look merged; the rest ask Crossref nothing, as
  before.

## 0.38.3

Fixed:

- A panic (index out of bounds) verifying free text whose cited title
  had only joining words before it ("The Deep learning. Nature. …"):
  looking back for a main title before it reached the first word. In
  0.38.0 to 0.38.2; one such claim stopped a whole `aspero verify`
  batch, and failed an embedder's call.

## 0.38.2

Added:

- An author's OpenAlex id (#174, an embedder's ask): `Person.openalex_id`
  ("A5001226970"), from OpenAlex's records, and carried onto the same
  author in another source's record of the same version (a shared DOI or
  PMID) when family name and first initial agree, so a Crossref-led work
  names it too. It is what an `authorId` search takes, for an author's
  works without an ORCID. The JSON gives it as `openalex_id`, after
  `orcid`. In Swift, `Author` gains `openalexId: String?`, last,
  defaulting to `nil`, so existing calls still compile. Works saved
  before this have none until fetched again.

## 0.38.1

Fixed:

- A short free-text title beside other authors, at the place of another
  work in the same journal and volume ("LeCun Y, Bengio Y, Hinton G. Deep
  learning. Nature. 2015;521…" against Littman's paper in Nature 521),
  graded that work's title `unknown`, as if the reference gave none; it
  is `mismatch` (#172). Beside the work's own authors a short label
  ("RESP score") is still no other title.

## 0.38.0

Added:

- The pages as written (#169, an embedder's ask): `Claim.pages` (the
  range, "762-774", or a page), beside `page`, the first page. The
  claim as read takes it from free text and BibTeX, in every style the
  first page is read from; the JSON's `read.pages`. Given without
  `page`, its first page counts as the page. In Swift, `Claim` gains
  `pages: String?`, last, defaulting to `nil`, so existing calls still
  compile.

Fixed:

- A free-text title that is another work's title as its subtitle, after
  a main title of its own ("Assessment of Clinical Criteria for Sepsis:
  For the Third International Consensus …" for Singer et al.'s "The
  Third International …"), graded that work's title `match`; it is
  `partial`, as the same title given as a field is (#168).

## 0.37.1

Added:

- Free text's title and authors in the claim as read (#169):
  `Verification.read.title` and `read.authors` (and the JSON's
  `read.title`, `read.authors`), as the citation writes them, not as the
  record has them. Where a work was found, the stretch of text that
  matched its title and the names before it ("Seymour CW", "Liu VX", …,
  those before "et al." included); where none was, what the common
  layouts tell (Vancouver, APA, Harvard, Chicago, a quoted title). Left
  empty where it can't be told: a wrong title is worse than none. On the
  red-team corpora, the layout's title is the found one 99.8% of the
  times it answers, and answers for 78% of citations. Verdicts don't
  change.

## 0.37.0

Added:

- `compare(claim:works:)` on the handle (#166): scores works the app
  already has against a claim, offline, on `verify`'s terms for its own
  candidates. Each work gets its per-field `checks` and `confidence`, in
  the order given, so a search's results can be shown beside a citation
  field by field. The engine's `compare` does the same for Rust callers.
  A work is read from its full `json`, or from its fields when that can't
  be read.

## 0.36.0

Added:

- A verification's reason as stored data (#163), for an app that keeps
  verifications and words their reasons when it shows them.
  `Verification.reasonJson` is stable, versioned JSON (the JSON
  contract's `reason`, with `"v": 1`); `reasonFromJson` reads it back as
  the typed `Reason`, and `workFromJson` reads a work's full JSON
  (`Work.json`) back as a `Work`. A code or version a reader doesn't
  know gives `nil`, never an error, so a stored reason always loads.
- A difference of kind (a reply or correction cited as the work) names
  the work's kind in the JSON's `reason.differs[].kind`
  (`expression_of_concern`, `reply`, …).

## 0.35.0

Changed (breaking):

- One name for a notice, and "note" kept free for what people write about
  a work (#160). A retraction, correction or newer version is a
  **notice** everywhere; a bibliography entry's malformed field is a
  **field problem**.
  - CLI JSON and MCP results: a work's `updates` is `notices`, and its
    `update_to` is `notice_for` (as the FFI already called them). A
    `check` finding of kind `updated` is kind `notice`, its detail under
    `notice` (was `update`). A verdict's English reason says
    `notice: RETRACTED …` (was `note:`).
  - FFI: `Verification.notes` is `notices`; `BibNote` is
    `BibFieldProblem`, and `BibEntry.notes` is `problems` (`BibProblem`
    stays). No new enum cases.
  - Rust: core's `Update`, `UpdateKind` and `UpdateTo` are `Notice`,
    `NoticeKind` and `NoticeFor`; `Work.updates` and `Work.update_to` are
    `notices` and `notice_for`; the engine's `Verification.notes` is
    `notices`, and `Finding::Updated { update }` is
    `Finding::Notice { notice }`. aspero-bib's `Note` is `FieldProblem`,
    `BibEntry.notes` is `problems`, and `Finding::Entry { note }` is
    `Finding::Entry { problem }`.
  - The cache is unchanged: it stores a work's notices under their old
    names, so caches read the same in versions before and after.

Added:

- Diagnostics: a `request` with `status=cancelled` says why, last (#156;
  DESIGN "Diagnostics events"): `why=deadline` (its attempt's deadline
  passed), `why=done` (its call no longer needed it: a verify found its
  verdict while other searches were still out) or `why=caller` (its
  caller stopped waiting). An embedder tuning deadlines can now tell an
  early verdict's cancels from a deadline's.

Changed:

- MCP: the tool list is about 38% smaller (#157): from ~46,000 to
  ~28,400 characters (about 11,500 to 7,100 tokens) at 0.34.0's seven
  tools, which every session pays for before its first call. The same
  tools, arguments and results; only how the schemas describe them:
  - a work, the session's spend and a call's usage are each an object
    whose description names its keys, rather than every key spelled
    out as a property, in each tool that returns one; a second copy
    (`latest`, `candidates`) points at the first;
  - `search_supports`' `query` points at `search`'s query tree rather
    than repeating its schema;
  - the query tree says what `phrase` means once, not on each term;
  - `sort` is a plain enum; input schemas drop `$schema` (2020-12 is
    MCP's default) and Rust's integer formats and int16 bounds.

  A test caps the list's size, so it doesn't grow back unnoticed.

## 0.34.0

Added:

- An id per engine call on its diagnostics events (#152; DESIGN
  "Diagnostics events"): `call=<id>`, last, on the `call` event, on
  each `request` made for it (cache answers, replays and paid requests
  finished detached included), on `retry`, and on the debug companions
  `request_url` and `call_input`. Ids are unique within the process,
  from one counter. Each claim's call in a `verify_batch` of more than
  one claim, and its prefetch, has an id of its own and `parent=<id>`,
  the batch's, so a batch reads as a tree; a check's verifies are still
  its parts, their requests carrying the check's id. The MCP `tool`
  event says `call=` of the engine call the tool made (host tools make
  none).
- FFI: `CallToken.id()`, the id of the call the token is passed to: an
  embedder can log the same number beside its call. A verify of more
  than one claim gives it as each claim's call's `parent`.
- Rust: `CallId`, an id chosen before an engine call
  (`CallId::new().scope(work)`), which the calls made in its scope take.

Changed (breaking for embedders):

- MCP: host tools' scopes are declared by the host, not enumerated by
  aspero (#153). A scope is a string: `sources` is aspero's own, for the
  engine tools; every other names a host tool's scope as the host chose
  it (`history.read`, `history.remove`, `notes.write`), at most 64 ASCII
  letters, digits, `.`, `_` and `-`, never `sources`. A key may list and
  call a host tool only if it holds exactly that tool's scope; none
  implies another. A malformed scope on a key fails `start` (and leaves
  the key out of `setKeys`), on a tool fails `start` or
  `hostToolsChanged()`; a scope no tool declares grants nothing. Errors
  name the scope string ("it lacks the history.write scope").
- FFI: `McpScope` is gone. `McpKey.scopes` and `McpCaller.scopes` are
  `[String]`, and `HostTool.scope` a `String`. The old cases become
  `"sources"` and the app's own names: `.readHistory` →
  `"history.read"`, `.writeHistory` → `"history.write"`,
  `.removeHistory` → `"history.remove"`, for example, on the keys and
  the tools alike.
- FFI: `HostTool` declares its hints, which were derived from the scope:
  `readOnly` (default false), `destructive` (default true) and
  `idempotent` (default false), MCP's own defaults, listed as the tool's
  annotations; a read-only tool is never listed as destructive. The old
  annotations were: reading `readOnly: true, destructive: false,
  idempotent: true`; writing `readOnly: false, destructive: false`;
  removing `readOnly: false` (`destructive` left on).
- Rust: `aspero_mcp::Scope` is gone for `aspero_mcp::scope` (`SOURCES`,
  `check_host`, `check_key`); `http::Key::scopes`, `Caller::scopes` and
  `http::HostTool::scope` are strings, and `http::HostTool` gains
  `read_only`, `destructive` and `idempotent`.

## 0.33.0

Added:

- Diagnostics events (#146; DESIGN "Diagnostics events", a contract):
  a `request` per provider request as it completes (provider, op,
  status or failure kind, ms, time waited for pacing, attempt, cache
  `miss`/`replayed`, cost; `detached=true` for a paid request finished
  after its caller left), a `request` per answer served from the cache
  (`cache=hit` or `stale`), a `call` per engine call (op, items,
  patience, providers asked, ms, outcome or `cancelled`), a `tool` per
  MCP tool call, engine or host (tool, or `unknown` for a name no tool
  has, key name, outcome, ms, cost), and `retry`. Info-level events
  hold counts, codes and times only; what was asked (queries, titles,
  identifiers, URLs) is only in `debug` companions (`request_url`,
  `call_input`, `tool_input`).
- FFI: `enableSystemLog(subsystem:level:)` sends them to Apple's unified
  log under the embedder's subsystem, by category (`net.<provider>`,
  `cache`, `budget`, `engine`, `mcp`), at `LogLevel.info` or `.debug`;
  debug lines are logged private, redacted outside a debug session. It
  installs the process's global `tracing` subscriber, so it returns
  false when called again, after another subscriber, or off Apple
  platforms.
- `CitedField::Identifier` (FFI `CitedField.identifier`, JSON `"field":
  "identifier"`): a claimed identifier that isn't the work's, beside
  another that is, with the work's of its kind as `actual` and the one
  claimed as `cited` (#148). A new enum case for exhaustive switches.

Changed:

- CLI `-v` no longer shows each request's URL, only its provider, op,
  status and times: URLs (which carry queries and identifiers) moved to
  `-vv`. Log targets are now `aspero::net` (was `aspero::http`,
  `aspero::retry`, `aspero::decode`), `aspero::engine` (was
  `aspero::graph`), `aspero::cache`, `aspero::budget` and `aspero::mcp`;
  warnings and info events give errors by kind, their text at debug. The
  default (no `-v`) now shows the network's warnings too: shared pacing
  failing, paid calls still finishing at exit.

Fixed:

- `--split` reads a tagged record as a reference of its own: a line
  starting a `{{cite …}}` template, an RIS record (`TY  -`) or an
  EndNote one (`%0`) starts a reference, and a template with its braces
  closed or an RIS record at `ER  -` ends one (#141). A template and a
  one-line RIS record pasted together were one reference, blending two
  works; RIS records written a tag per line, one after another, were
  one too.

Fixed (red team round 7, #148, from `bench/redteam/wild-3.jsonl`;
measured offline on round 6's tape: wild-3's wrong citations verified
fell from 25 of 122 to 8, and no correct citation of holdout-5, wild-2,
holdout-6 or wild-3 changed its verdict):

- Wrong citations `verify` verified, now plausible:
  - A work's DOI beside a PMID or PMCID that isn't the work's (another
    paper's, a few digits off: 17 of wild-3's 25): its identifiers partly
    agree, and the reason names the identifier. Not a mismatch, since a
    record's own PMID is sometimes wrong; a record without a PMID
    contradicts none, and one any source of the work gives agrees. DOIs
    and arXiv IDs aren't held to the work's this way (a work may have
    several).
  - Two claimed identifiers whose records are two works (the work's PMID
    beside another paper's DOI), unless the two records are one work's.
    Identifiers that all name another work are a mismatch, as before.
- A bare PMID standing as a Markdown table's cell beside a DOI ("… |
  10.1001/jamacardio.2016.0695 | 27438392 (verify)") is that PMID: the
  only cell of 7 or 8 digits, alone or with a bracketed note. Bare
  numbers in prose, in a row without a DOI or inside a cell are still
  none.

## 0.32.0

Added:

- MCP: a fourth scope, `RemoveHistory` (`remove_history`; FFI
  `McpScope.removeHistory`), for host tools that remove, untag or delete
  (#144). Checked like the others and not implied by `WriteHistory`, so
  a key may add without being able to remove; its tools are listed as
  destructive, and a key gaining or losing it hears `tools/list_changed`.

Fixed:

- A paid OpenAlex request already sent is finished and counted when its
  caller stops waiting (#139). A claim verified while its other searches
  were still out cancelled them mid-flight: OpenAlex charged for them,
  but their cost was missing from the usage, the credit spent here and
  the caps, and their answers from the cache and the tape (29% of a live
  run's OpenAlex requests; the balance fell 1.37× the costs counted).
  They now finish in the background, counted and cached as an awaited
  request is. Requests still waiting for a pacing slot are cancelled, as
  before, so the early stop still saves them. New `Engine::settle`
  waits, with a bound, for those finishing; the CLI waits up to 10 s
  before it exits, and `verify` and `check` add their usage to what they
  report.
- `verify` no longer panics on a reference holding a character that folds
  to two words, such as "½" ("1⁄2"): their spans overlapped, and reading
  the text between them sliced backwards.

Fixed (red team round 6, #126, from `bench/redteam/holdout-5.jsonl` and
`wild-2.jsonl`; measured offline on 0.31.0's tape: holdout-5's wrong
citations verified fell from 36 of 613 to 13; wild-2's correct citations
missed from 52 of 910 to 6, the rest of the change waiting on requests
the tape lacks):

- Correct citations `verify` missed:
  - A model-written reference whose only identifier is a bare arXiv
    number after a colon, as a table's cell or last ("Chan et al.,
    NegotiationToM: 2404.13627", "… | 1706.03741 | NeurIPS 2017") cites
    that arXiv ID. `scan` still reads bare numbers in prose as none.
  - An arXiv number's year and month ("1811.07871") is no cited year.
  - Titles' acronyms of their own words ("LLMs", "RLHF", "ToM") read as
    the words; a title's acronym ("Advanced RAG") is no byline.
  - Books: the title a reference cites is searched, not a matcher hit's
    run of a publisher's words sharing one of its words ("Ethiopian
    Public Health Association"), nor only a book review's title holding
    the whole citation; titles are read after bylines written "Family,
    Given," ("Vidal-Naquet, Pierre, La torture dans la République,
    Paris, Minuit, 1972") and after Harvard's "(2011)" without a stop.
  - An article found only reprinted later as a chapter of a collection,
    cited with its original year, is another version: plausible, not a
    mismatch.
  - A reference giving authors, year, journal and a bare DOI but no title
    is untitled: its DOI's words are no title.
  - A journal named with a volume and page counts though the title holds
    its word ("Physiotherapy, 40(8)").
- Wrong citations `verify` verified, now plausible:
  - Another meeting of the same society: an acronym fits a meeting's name
    only through two words or more and its furniture's initials ("MM" is
    not the Conference on Multimedia Retrieval), an acronym alone that a
    known conference can't make is another ("WWW '18" for CIKM), two
    acronyms that differ are two meetings, and a conference paper's
    container is its meeting's whatever its name. BibTeX's `booktitle`
    cites proceedings.
  - A title only partly the record's, words missing that aren't
    misspellings, unless the claimed identifier or the place agrees (a
    journal version's title cited for its preprint); a word replaced after
    the record's main title ("The Male Quixote").
  - Invented co-authors beside bylines in capitals ("JOHN R. JAENIKE") or
    of family names alone; names given for a record without a byline.
  - A journal's article cited as a chapter of an edited book named as its
    journal, its volume not cited; a conference paper cited at a volume
    and issue.
  - A supplement's roman page ("2009;18:ii5-ii8") is read with its volume.
  - IEEE's "Notice of Retraction:" is a marker, as "RETRACTED:" is.

## 0.31.0

Added:

- `Budget.creditSpentTodayHereUsd` (FFI, defaulted `nil`), JSON
  `credit_spent_today_here_usd` and a "$0.0100 here" figure in `aspero
  budget`: the prepaid credit charged today to calls made through aspero
  sharing this cache file, beside the key-wide `prepaidSpentTodayUsd`
  (#136). Engine: `Budget::credit_spent_today_here`.

Changed:

- The credit cap (`--openalex-credit-cap-usd`,
  `Config.openalexDailyCreditCapUsd`) counts the credit spent here, not
  everything on the key (#136): OpenAlex gives one key per account, so a
  researcher's scripts spending credit no longer stop aspero. Each paid
  call's cost counts once its response says the allowance is gone (the
  call that used it up, only the part past what was left), kept per day
  in the cache file (`credit_here`), shared by every process using it.
  The stop says "($0.26 of credit spent today through aspero here)", and
  `BudgetCap.spentUsd` for `.credit` is that figure; `spendableTodayUsd`
  counts credit left as the cap less it. The allowance cap still counts
  the whole key. A stop at the credit cap no longer asks `/rate-limit`
  first, since stale header balances no longer decide it. Library:
  `PrepaidDays` gains `add_credit_here` and `credit_here`.
- The FFI crate's records and the `Aspero` handle's methods are split into
  modules by domain. No Swift API changes, but 31 binding checksums do
  (uniffi hashes each item's module path): regenerate the Swift bindings
  with the library, as the xcframework does.

## 0.30.0

Changed:

- Developer: under the HTTP tape, identifiers are looked up one per
  request (`EngineBuilder::taped`, set by `StandardConfig::http_tape`), so
  each ID's answer, found or not, is a tape entry of its own and replays
  however later batches are made up (a code change moving which claims
  carry identifiers no longer turns every later batch into a miss). A
  batch an older tape holds whole still replays. Verdicts are the same;
  recording costs no more (OpenAlex's single-work lookups are free, its
  batch filters $0.0001 each) but sends more requests (DESIGN "HTTP
  tape").

Fixed (from a held-out red team 0.27.0's fixes never saw,
`bench/redteam/holdout-1.jsonl`: wrong citations verified fell from 29
of 351 to none, 229 of them answered, the rest at OpenAlex's daily cap;
real ones verified 351 of 382, from 343, and accepted 375, from 373):

- The journal is read in more styles: Nature's single page after a
  dotted name ("J. Org. Chem. 101, 6301 (1994)"), IEEE's comma after one
  ("Mach. Intell., vol. 56"), Chicago's issue and bracketed year
  ("Journal 94, no. 2 (2004): 317-321", which read the issue as the
  volume, and "“…” eClinicalMedicine (2010)."), a colon inside a name
  ("JAMA: The Journal of …"), the words after a quoted title in sentence
  case ("Revue historique"), and dotted names after a title in title
  case ("… by Nanoparticles. J. Am. Chem. Soc."), where each reading is
  weighed and the journal is clearly another only if every one is.
- A record in a journal whose type isn't given (Europe PMC's) is held to
  its journal like an article, structured claims included.
- A year or journal word inside a DOI or link ("10.1113/jphysiol.1974…",
  "10.1093/brain/…") is not the year or journal cited.
- A byline naming people as IEEE, MLA or BibTeX write them, none of them
  the work's authors, makes the authors different, not unknown; a
  co-author with initials of their own is no given name of the next.
- A BibTeX entry's title is compared as its title (not a booktitle that
  is a proceedings' name), and a quoted title counts however short.
- A book doesn't confirm a BibTeX `@article`'s journal; the original
  isn't verified for a correction of it cited ("Corrigendum to “…”"); a
  record without its page takes it from another record of the work in
  the same journal and volume.
- A title in capitals ("… WOMEN ON ORAL CONTRACEPTIVES …") no longer
  reads as names.

Fixed (from a second held-out red team, `bench/redteam/holdout-2.jsonl`,
replayed offline from the tape of round 2's run: wrong citations verified
fell from 39 of 399 to 2, 87 claims needing requests the tape lacks; real
ones mismatched or not found fell from 19 to 9 of 441):

- A conference paper cited at another conference (CVPR's at ICCV, KDD's
  at ICML) is plausible, the venue named: the major conferences are told
  by acronym or name, the record's by its proceedings or IEEE DOI. A
  preprint server's copy cited in a conference's proceedings can't
  confirm the conference: plausible, as a copy can't confirm a journal.
  Real NeurIPS or ICML citations whose only record is arXiv's are
  plausible too.
- A title with words swapped for others anywhere in it, several
  together ("Likelihood Determinants" for "Risk Factors", "A Controlled
  Study" for "A Randomized Trial"), in free text or a structured claim,
  is plausible at most; spelling, abbreviations, numbers spelt out and
  words broken at a line's end still match.
- More locators and journals are read: MLA's `77.4 (2005): 1321-1373`,
  Harvard's `pp.58-63` and `p.054433`, IEEE's `Art. no. 615`, prose's "the
  2007 … paper in Psychol Sci (…)", and dotted names after a title in
  title case ending in a short abbreviation (`… Creation. Psychol. Sci.
  5, 14`), each reading weighed.
- Invented co-authors beside a real first author are caught where the
  record gives a family name in capitals (`Seok-Bae LEE`), and next to
  Vancouver initials (`Rangel TR, Thalubrecht HL, …`).
- A journal word inside the work's title isn't the journal cited
  (`… monitoring autophagy. bioRxiv [Preprint]`), and a record OpenAlex
  types a preprint but in a journal PubMed indexes is that journal's
  article, so either cited as a preprint is plausible.
- A Chinese or Japanese title is a title given, however few spaces: one
  at another work's journal, volume and page is no longer verified.
- A BibTeX entry's arXiv `eprint` and `pmid` are identifiers claimed, so
  an unassigned arXiv ID there is checked.
- A record a provider gives for an identifier it doesn't list (OpenAlex
  answers a Zenodo version's DOI with the concept's record) carries it:
  the version's DOI no longer "does not exist".
- An inverted name in a record (`Lubrano, Emanuele`, DataCite's) is split
  as one; a claimed double-barrelled name an author's record shortens
  (`Hirschi-Santelmo` for Hirschi) and an organisation among a structured
  claim's authors are no strangers; BMJ's page `698.1` is page 698.

Fixed (from a third held-out red team, `bench/redteam/holdout-3.jsonl`,
replayed offline: wrong citations verified fell from 36 of 426 to none;
real ones verified 474 of 508, from 475, and accepted 490):

- A paper cited in any other meeting's proceedings is plausible, the
  venue named: proceedings' names are compared by what they are about,
  their furniture (Proceedings of the, ordinals, years, Annual,
  International, ACM/IEEE, SIG…, Conference on) set aside, and by their
  acronyms ("In … Conference on Computers and Accessibility (ASSETS
  '15)" for an HRI paper). A journal's article cited "In Proceedings of
  …" a meeting is another venue too. A record giving its pages but not
  its proceedings (OpenAlex's of ACM papers) takes them from another
  record of the same DOI, so the venue is checked whichever is kept.
- A book's chapter cited as a journal's article (a volume and issue, its
  book's title or a journal as the journal) is plausible, its volume
  named; nor does the book verify for it.
- An RIS or EndNote record or a citation template sent as a JSON claim's
  `text` keeps the fields it gives: an RIS record on several lines kept
  only its DOI (and verified by it whatever authors it named), or was
  refused as empty without one.
- A work named by its claimed identifier, whole title and place, cited
  with other authors, is plausible rather than a mismatch.
- A journal's words in a reworded title before it are the title's, not
  the journal's, so the title is compared; an invented co-author among
  names in a script without capitals (Arabic) is seen; a volume
  numbered like a year (`2007.0(52)`) is no year cited; prose naming a
  journal before its volume and pages (`showed in Cancer Epidemiology,
  38, 211–214`) is read; "Research Square" names a preprint server.
- Developer: OpenAlex's 404s answered on prepaid credit, past the daily
  allowance, are recorded on the HTTP tape (they read as a spent budget,
  so a red-team run on credit replayed its "not found" lookups as
  misses). The red team counts an ISBN-10 or a versioned DOI as the
  work's own identifier.

Fixed (from a fourth held-out red team, `bench/redteam/holdout-4.jsonl`,
and 394 references two language models wrote, `bench/redteam/wild-1.jsonl`,
replayed offline: wrong citations verified fell from 74 of 481 to 9, and
in wild-1 from 4 of 31 to 3; real ones verified 537 of 569, unchanged,
and in wild-1 309 of 363, from 305):

- A sister journal cited is another journal, plausible with the journal
  named: the family's parent for a sister's article ("JAMA" for JAMA
  Oncology, "The Lancet" for The Lancet Infectious Diseases, "Nature"
  for Nature Medicine) or a sister for the parent's ("JAMA Intern Med",
  "NEJM Evid", "JACC Heart Fail"). One name inside the other no longer
  makes them possibly the same; what each has of its own decides, an
  acronym's expansion, a series letter, a place, edition or descriptive
  subtitle aside. Abbreviations still match ("Lancet Infect Dis").
- A meeting of the same society cited is another venue: a society's
  acronym before its other meetings ("International AAAI Conference on
  Web and Social Media") isn't its conference's, and a venue cited by an
  acronym the record's name can't make (ICECS for the Midwest Symposium
  on Circuits and Systems) is another meeting. A copy in no venue (a
  repository's, a talk's) can't confirm a meeting cited.
- A journal's article cited by its preprint's DOI (Research Square,
  Preprints.org, bioRxiv, SSRN, …) with the journal's name and place is
  plausible, the server named; so is a preprint's record, its server DOI
  claimed, cited in a journal by name alone. An arXiv identifier beside
  a journal or venue still verifies.
- A journal's article cited as a chapter of an edited book ("In: …,
  editors. Handbook of …") is plausible, another container.
- A correction notice titled "Author Correction:" or "Publisher
  Correction:", typed a correction by its record, or titled "… –
  RETRACTION", is not the original it corrects; a claim citing one after
  its year ("(2015) 'Correction: …'") cites one.
- As models write them: a trial's label before the byline
  ("EMPEROR-Reduced (…): Packer M, …") is no invented author; BibTeX's
  accents (`Veli{\v{c}}kovi{\'c}`) are read; a record's garbled dash
  ("205???210") or title running on into a second sentence no longer
  differs; a page with a section's letter (`pp. R3–R20`, BMJ's `m1123`)
  is read; a journal's letters numbered in its volumes are the journal.
- A reference's own title (BibTeX's, a quoted one) is searched when the
  citation matchers' hits share only part of it, or only other words
  (a publisher's), with the reference.
- Developer: the red team compares identifiers however a corpus writes
  them (bare DOIs, `PMID:`), and prints a criterion with nothing to
  measure as n/a.

## 0.29.0

Added:

- Pages that aren't scholarly works (#119): `claimFromPage` tells a
  scholarly work's page (an identifier, citation tags, scholarly JSON-LD,
  a publisher's site, or no markup to tell) from a web page (a Wikipedia
  article, a news story, guidance), and a web page has no claim to
  verify. `PageClaim.kind` (`PageKind`: `work`, `webPage`) and
  `PageClaim.webPage` (`WebPage`: title, site, canonical address
  normalised, people as authors, published, updated, language, and its
  key, `url:` and the address), read for every page with a title.
  `webPageWork(page, accessed)` makes it a `Work` of type `webpage` that
  `export` writes as BibTeX `@misc` (`howpublished`, `note` with the
  site and access date), RIS `ELEC` (`Y2`, `LA`) and CSL-JSON `webpage`
  (`accessed`, `language`: a Zotero Web Page), and `cite` formats in any
  style. `Work.accessed` and `Work.language`; core `WorkType::WebPage`,
  `Work::accessed`, `Work::language`, `webpage::{PageKind, WebPage,
  normalise_url}`. `aspero verify --html FILE --url URL` shows a web
  page's record (exit 0) rather than verifying it, and writes it out
  with `--format bibtex|ris|csl-json|cite`, read on `--accessed DATE`
  (today by default).

Fixed:

- `claimFromPage` titled a page by its JSON-LD `headline` before its
  `name` (#118): Wikipedia's `headline` is the article's short
  description ("American actor and novelist (1930–2025)"). `name` comes
  first now, unless it isn't a title. A page title's suffix also goes
  when it is the name the page gives its own site (`og:site_name`,
  JSON-LD's website or publisher), even one holding a separator.
- A page's values lost anything that looked like a tag once decoded, so
  a title naming one ("&lt;meta&gt;: The metadata element") lost it. Only
  formatting markup (`<i>`, `<sub>`, `<span>`, JATS's, namespaced tags)
  is taken out now, escaped or not.

## 0.28.2

Added:

- `Budget.spendableTodayUsd`: what this engine's paid calls may still
  spend today under both caps, as `aspero budget` counts it
  (`Engine::spendable_today`), and `Budget.creditCapUsd`, the credit cap
  in force ($0.25 when unset), so callers needn't repeat the policy.

## 0.28.1

Added:

- Each MCP key's spend today (#113): `HttpServer::status` gains
  `KeyStatus.spent_today_usd`, `Status.spend_day` and `spend_resets_at`
  (FFI `McpKeyStatus.spentTodayUsd`, `McpStatus.spendDay`,
  `spendResetsAt`): the ledger's figure for the UTC day plus spend not yet
  recorded, the same the key's daily budget is checked against.
- What OpenAlex charges (#113), from `/rate-limit`'s `endpoint_costs_usd`
  (so with a key): `Usage.prices` and `Budget::prices` (a `Prices` of
  `singleton`, `list`, `search`, …), `Budget::search_price`; FFI
  `Budget.searchPriceUsd` and `Budget.prices` (`[BudgetPrice { endpoint,
  usd }]`). `Engine::spendable_today`: what paid calls may still spend
  today under the caps (the allowance left to its cap, then credit to the
  credit cap). `aspero budget` adds "searches $0.001 each, about 850 left
  today" (spendable over the search price, rounded down) and JSON
  `spendable_today_usd`, `search_price_usd`, `searches_left_today`,
  `prices_usd`.

## 0.28.0

Added:

- References pasted as a Wikipedia citation template (`{{cite journal
  |last1=… |title=… |journal=… |doi=…}}`, any `cite …` or `citation`), an
  RIS record (its lines run together on one, or not) or an EndNote record
  (`%0 Journal Article %A … %T …`) are read as their fields, a structured
  claim, wherever a reference is taken (`verify`'s arguments and stdin,
  MCP, the apps). A stdin line starting with `{{` was refused as invalid
  JSON; one starting with `{` that isn't meant as JSON is a reference
  now too.
- Developer: an HTTP record/replay tape (`ASPERO_HTTP_TAPE`,
  `ASPERO_HTTP_TAPE_MODE=replay-only`), on by default for `verify_bench`
  and `scripts/redteam.py`: each distinct request is paid for once and
  replayed free after, offline if asked (DESIGN "HTTP tape").
- A daily cap on OpenAlex prepaid credit (#110), beside the allowance's:
  `openalex_daily_credit_cap_usd` (engine `StandardConfig`,
  `EngineBuilder::daily_credit_cap`; FFI `Config.openalexDailyCreditCapUsd`,
  nil for the default; CLI `--openalex-credit-cap-usd` /
  `ASPERO_OPENALEX_CREDIT_CAP_USD`). Once the free allowance is all used
  (by anything using the key), paid calls go on, spending credit, while
  the credit spent today is under it; until then the allowance cap holds
  the rest of the allowance back. 0 never spends credit (DESIGN
  "Resilience").
- Which cap stopped a call: `Error::BudgetCapped` gains `which`
  (`CapKind::Allowance` or `Credit`), FFI `BudgetCap.capKind`
  (`BudgetCapKind.allowance` / `.credit`), with `capUsd`/`spentUsd` the
  cap reached and what's spent against it; a credit stop reads "stopped at
  today's credit cap of $1.00 ($1.02 of credit spent today)".
- Credit spent today and the credit's expiry on `Budget`:
  `prepaid_spent_today` / `prepaid_expires_at` (FFI
  `prepaidSpentTodayUsd`, `prepaidExpiresAt`; `aspero budget` JSON
  `prepaid_spent_today_usd`, `prepaid_expires_at`, and `allowance_cap_usd`,
  `credit_cap_usd`). OpenAlex doesn't report the spend: it's the day's
  first balance seen, kept in the cache file so every process and restart
  agrees, less the balance now, a top-up raising the start; a day first
  seen part-way through undercounts. `Usage.prepaid_expires_at` from
  `/rate-limit`, read as JavaScript prints a date (or ISO).

Changed:

- OpenAlex prepaid credit is now capped at $0.25 a day by default
  (before, it was spent freely once the allowance was gone, unless an
  allowance cap at or under the allowance stopped every paid call). Set
  `--openalex-credit-cap-usd` (or the config field) to allow more, or 0 to
  spend none. With a key, the first paid call of a process now asks
  `/rate-limit` (free) first, as an allowance cap already did. The
  benchmark and `scripts/redteam.py` set their own credit cap, $5.
- `aspero budget` says both caps in their terms: "free allowance $0.0498
  of $1.00 used today (cap $0.50), resets 00:00 UTC; credit $0.0300 spent
  today (cap $0.25), $4.97 left, expires 2027-02-14".

Fixed:

- Credit spent today no longer runs far past the truth ("$5.33 of credit
  spent today" for $0.34 spent): OpenAlex's servers answer with balances
  up to $0.30 apart, the stale ones higher, and each rise was taken for a
  top-up. A response's balance now only ever lowers today's figure; only
  `/rate-limit` (which says the credit bought) sets it either way or tells
  a top-up; and a stop at the credit cap is checked with a free
  `/rate-limit` first (at most once a minute). `Budget.prepaid_remaining`
  is that balance, not the last response's. The first `prepaid_day`
  table is dropped. `Usage.prepaid_bought` (OpenAlex
  `prepaid_balance_usd`).
- OpenAlex is no longer stopped until the reset ("usage quota
  exhausted") by a failing call (a per-second 429, a server fault) made
  once the day's allowance is used up while prepaid credit is left:
  OpenAlex spends the credit then, so only no credit left either (or its
  `not_enough_credits`) is a spent budget.

## 0.27.0

Added:

- `docs/accuracy.md`: what `verify` promises (a fabricated or miscited
  reference shouldn't come back verified; plausible is "check it") and
  what it can't check, the verdict policy, the red-team method (held-out
  and adversarial rounds), acceptance criteria and the release gate, the
  0.26.0 baseline, and how to reproduce it. Linked from the README and
  DESIGN.
- Issue forms: "Wrong verdict" (the citation, the verdict, the JSON
  output, the right work, the version and where it ran), "Bug" and
  "Feature"; `scripts/issue-to-case.py <n>` turns a wrong-verdict issue
  into a red-team case in `bench/redteam/reports.jsonl`.
- `verify --report`: after each result, a link to a new wrong-verdict
  issue with the form filled in (JSON: `report_url`), the output
  shortened to keep the link under 7,000 characters. Nothing is sent: the
  link is only printed.

Fixed (from the red team of #102: wrong citations verified fell from 31
of 152 to none, with real ones accepted 152 of 155, from 147):

- A citation naming co-authors a work doesn't have is no longer
  verified on its first author: it's plausible, its authors to check.
  Not for consortia, bylines cut short, or books.
- A title with words added ("Deep learning for medical image analysis"
  for "Deep learning"), a subtitle the work lacks or another one, or a
  word replaced ("Hydroxychloroquine in …" for "Dexamethasone in …") or
  words inserted ("… Septic Shock in Adults and Children (Sepsis-3)") is
  plausible at most. A dropped subtitle still matches, as does a subtitle
  added to a book's title (book records often leave it out).
- A year inside a title ("Cancer statistics, 2020") no longer stands in
  for the year cited, and is part of the title: the 2020 report cited as
  of 2018 with its volume and pages is plausible, the year named, not the
  2018 report nor "Colorectal cancer statistics, 2020". "CA Cancer J
  Clin" now matches "CA: A Cancer Journal for Clinicians".
- The cited journal is read in more styles: before the year ending a
  Vancouver reference ("… JAMA. 2020."), before its DOI link, IEEE's
  "vol." and "no.", Harvard's "'…', Chest." and "pp.", BibTeX's
  fields, prose ("… reported in Gastroenterology (1984)"), after a title
  ending in a capitalised word ("… Wuhan, China. Lancet."). A wrong journal there, alone or with the year one off, is
  plausible, not verified.
- Nature's single page ("Nature 530, 484 (2016)"), Harvard's pages and
  BibTeX's volume and pages are read, so wrong ones are caught.
- Another work's title at a work's journal, volume and page is a
  different title, not a missing one: plausible at most, never verified.
- A copy without journal, volume or page (a Zenodo deposit, a report)
  no longer verifies a citation of the journal article, nor outranks it.
- A preprint cited in a journal by name, and a journal article cited as
  a preprint, are other versions: plausible.
- Real works missed before are found or accepted: conference papers
  without DOIs (the title a reference suggests is searched when the
  citation matcher has none), a book found only reprinted in a reader,
  and a dataset whose DOI's record carries a later version's year.

## 0.26.1

Fixed (found by fuzzing, `fuzz/`):

- An `author_id` like an ORCID with extra hyphens (`0000-0002-18-5-0097`)
  panicked instead of being refused, in a query tree from the CLI, the
  MCP tools or the FFI.
- A .docx file inflating to gigabytes was read whole: parts are read up
  to 64 MB, a larger body unreadable.
- Each identifier kind's own parser (`WorkId::parse_as`, as a page's
  `citation_doi` is read) now cleans look-alikes (Unicode dashes,
  zero-width spaces) as `WorkId` parsing and `scan` do.
- A soft hyphen ending a DOI's line, the next not continuing it, is no
  longer read as a `-` ending the DOI.

## 0.26.0

Added:

- MCP activity as data beside its summary (#100): `http::Activity` and
  Swift's `McpActivity` have `error` (the kind, when it failed: Swift's
  new `McpErrorKind`, from `invalidArguments`, `notFound`, `forbidden`
  and `unsupported`, the agent's own doing, to `unavailable`,
  `throttled`, `auth` and the app's `timeout`, with `other` last for
  kinds added later), `errorMessage` (the whole message, without
  "error: "), `query` (what an engine tool was asked, as given: the
  keywords or the query tree in a line, `title:sepsis AND years:2018–`,
  identifiers or citations joined with "; ", at most 500 characters) and
  `count` (how many came back: works found, looked up, verified,
  references listed). An app tool's `query` and `count` are empty. The
  summary is as it was; the new fields default to `nil`, so Swift code
  building an `McpActivity` still compiles.

## 0.25.0

Added:

- A claim's site, the page it was read from (#98): `claimFromPage` and
  `verify --url` fill it from the address's host (`jamanetwork.com`),
  `site` in a JSON claim and `--site HOST` give it otherwise, and Swift's
  `Claim` has `site`. It's a hint of the publisher, never the journal nor
  a difference reported. For a claim without identifiers from a known
  publisher's site (JAMA Network, Oxford Academic, Nature, Elsevier's
  sites, Wiley, Springer and BMC, ACS and some 30 more, by registrable
  domain), Crossref is also asked for the title among the publisher's
  works (its DOI prefixes, free), and between candidates that match about
  as well the publisher's is chosen: a title shared from JAMA's page
  ("Treatment of Acute Ischemic Stroke: A Review") is now JAMA's review,
  not an older article of that title elsewhere. It never makes a match
  verified, never outranks a clearly better one, and with no candidate
  from the publisher changes nothing. When it decided, JSON says
  `chosen_by_site: true` and Swift's `Verification.chosenBySite`.

## 0.24.0

Added:

- A claim from a publisher's article page, for addresses that don't say
  which work they show (JAMA Network, Oxford Academic): `claimFromPage`
  (Swift) and `aspero verify --html FILE --url URL [--title TEXT]` read
  the page's markup, as a browser rendered it, offline: Highwire
  `citation_*` tags, PRISM, Dublin Core and schema.org JSON-LD, each field
  from the first that gives it, then `og:title` or `<title>` without the
  site's name (" | JAMA | JAMA Network"), then the title a browser gave it
  unless that's link text ("Full Text", "PDF"), and the address's
  identifiers. Every identifier found is claimed, so `verify` checks it,
  or `lookup` takes it. Without the markup, the address and the title
  alone. Swift's `PageClaim` holds the identifiers (`Mention`s), the
  `Claim` to verify and the `PageSource` that answered; JSON echoes
  `page_source`.

## 0.23.0

Added:

- Publishers' article addresses, pasted or dragged from a browser, are
  read as the work they show, by `scan` and so by `lookup`, `verify`,
  `identify`, MCP and the app: Nature's `nature.com/articles/…` pages as
  their DOI (`nature14539` is `10.1038/nature14539`), and ScienceDirect,
  Cell and The Lancet pages by Elsevier's PII, a new kind of identifier
  (`pii:S2772940026000032`). Crossref finds the DOI a PII belongs to (one
  free call), and the work is then looked up as that DOI, so results are
  the same as for the DOI. A PII Crossref doesn't know is not found.
- Each mention `scan` finds says whether it was read from a web address
  (`from_url` in JSON, only when it was; `fromUrl` in Swift), so the app
  can say a work was found from its address.

## 0.22.1

Changed (the CLI's tables and text; JSON and the other formats are as
they were):

- Tables fit the terminal: identifiers are never broken, titles get the
  width left, low-value columns drop or move under the title when narrow,
  and rows of several lines are ruled apart. Search tables no longer show
  the provider's raw score.
- Text wraps to the terminal with hanging indents (unwrapped when piped).
  Numbers have thousands separators, work types read in words ("journal
  article"), and `analyse` numbers its works from 1 and leads each line
  with the work.
- `verify` text gives the verdict, why (unless it simply matches) and each
  field's check as ✓/~/✗, without repeating the work; its table shows the
  work matched.
- `verify` and `resolve` with no citation read one per line from stdin
  when piped, as `lookup` does; on a terminal they say how to give one.
- Batch progress overwrites itself on stderr, only on a terminal. Colour
  is kept to verdicts and secondary lines; `NO_COLOR` turns it off.
- `lookup` says why an input wasn't looked up below the table.

## 0.22.0

Breaking: names now agree between Swift and JSON, and money says its
unit. Only names change; values and shapes stay as they were.

- Usage's cost: `cost` → `costUsd` (Swift), `cost` → `cost_usd` (JSON).
- Usage's remaining budget: `remaining` → `remainingUsd`, `remaining` →
  `remaining_usd`.
- Usage's daily allowance: `limit` → `limitUsd`, `limit` → `limit_usd`.
- Usage's prepaid balance: `prepaidRemaining` → `prepaidRemainingUsd`,
  `prepaid_remaining` → `prepaid_remaining_usd`.
- `Budget`'s fields in Swift: `remaining` → `remainingUsd`, `limit` →
  `limitUsd`, `prepaidRemaining` → `prepaidRemainingUsd`.
- `aspero budget`'s JSON: `limit` → `limit_usd`, `used` → `spent_usd`,
  `remaining` → `remaining_usd`, `prepaid_remaining` →
  `prepaid_remaining_usd`.
- A reached daily cap's figures: `BudgetCap.cap` → `capUsd`,
  `BudgetCap.used` → `spentUsd` (Swift only: JSON gives the cap as an
  error message).
- An analysis' estimate: `Estimate.usd` → `costUsd` (Swift), `usd` →
  `cost_usd` (JSON; Rust: `Estimate::cost_usd`).
- The OpenAlex daily cap: `Config.openalexDailyCap` →
  `openalexDailyCapUsd` (Swift), `StandardConfig::openalex_daily_cap` →
  `openalex_daily_cap_usd` (Rust), `--openalex-cap` → `--openalex-cap-usd`
  (the old flag still works) and `ASPERO_OPENALEX_CAP` →
  `ASPERO_OPENALEX_CAP_USD` (the old variable is still read when the new
  one isn't set).
- A search's cursor for the next page: `SearchResults.next` →
  `nextCursor` (JSON already `next_cursor`).
- How a result was served: `SearchResults.freshness` and
  `Lookup.freshness` → `served` (JSON already `served`; the enum is still
  `Freshness`).
- A lookup attempt's failure: `Attempt.message` → `error` (JSON already
  `error`).
- A work's deduplicating identifier: `Work.canonical` → `canonicalId`,
  `canonical` → `canonical_id`.
- A DOI found only with its trailing punctuation kept:
  `Lookup.withPunctuation` → `foundWithTrailingPunctuation`; JSON's
  `"note"` → `"found_with_trailing_punctuation": true` (Rust:
  `Looked::found_with_trailing_punctuation`).
- The work a verdict is about: `Verification.best` → `bestMatch`, `match`
  → `best_match` (a `check` entry's `match` too; Rust:
  `Verification::best_match`).
- Providers that failed: `Verification.failures` → `errors` (JSON already
  `errors`).
- The caller's key, echoed: `Verification.key` → `claimKey`, `key` →
  `claim_key` (claims are still written with `key`).
- Why, as data: `Verification.why` → `reason`, `why` → `reason` (Rust:
  `Verification::reason`).
- Why, in an English sentence for logs and agents: `Verification.reason`
  → `reasonText`, `reason` → `reason_text` (Rust:
  `Verification::reason_text`).
- `Analysis.notYet`, always empty, is removed from Swift.

## 0.21.0

Breaking:

- An analysis's `incomplete` lists every stop it met, not only the
  first: a JSON array, always present (`[]` when complete), at the top
  level and in each section; `[Incomplete]` in Swift. A sample of a
  much-cited seed (`capped`) is no longer hidden behind a budget stop or
  a failure.
- Removed from the library: the unused `Capabilities` flags `citations`,
  `references` and `recommend` and their `Provider` methods,
  `ArxivId::is_old_style`, `ReplayClient::then_fail` and
  `Analysis::not_yet` (the app's `notYet` stays, always empty).
  `Engine::identify` returns an `IdentifyReport` with its usage.

Added:

- The app's search results say which filters were ignored
  (`ignoredFilters`); usage gives a source's daily budget and prepaid
  balance; cache stats count cached citation links; `analyse` takes how
  many related works to list (`relatedLimit`, 20); a stop says which
  related-works stage stopped (`stage`) and which source failed.

Fixed:

- A search naming no source asks Europe PMC before DataCite, in the order
  the search sources are listed.
- Search refuses what it couldn't search faithfully, rather than
  widening or narrowing it: work types that would be filter syntax
  (`book,is_oa:true`), terms with no letters or digits, a journal beside
  other terms on Crossref (which only ranks by it), and leaving out on
  OpenAlex a journal naming more sources than a filter takes. `<`, `>`
  and `=` are dropped from searched words.
- Europe PMC and DataCite bracket a query tree beside an expert query,
  and escape backslashes in DOIs. A none of a none means an any. Sorting
  newest first no longer changes what a tree can search.
- MCP budgets hold: a failed `identify` charges what it spent; calls at
  once pass a session's or a key's budget by one call at most, and a key
  runs at most four paid calls at once; spend the ledger couldn't save is
  kept and saved later.
- The MCP server: stopping refuses new requests at once, and a stop cut
  short finishes when called again; a session opening as its key is
  revoked is closed; a key may hold 16 sessions (then HTTP 429); activity
  the app is far behind on is dropped, not queued without limit; an app
  tool that times out is reported as of unknown outcome, to check before
  retrying.
- Related works read at most 75 pages of citing works however little
  each seed is cited, and the estimate agrees; works of unknown citation
  count are read, not taken as uncited; pages keep to the budget by what
  OpenAlex charges.
- A seed is `not_found` only once that's settled: a failure or the budget
  stopping a second ask says so. A daily cap stops an analysis as
  `budget`, not `provider_failed` (the CLI no longer exits
  "unavailable"). Preprints and published versions linked only through a
  third record count as one work. A stale cached record's references
  aren't cached as fresh. A lookup of 101 works counts one paid request.
- Odd one out says `no_reference_data`, not `no_links`, where a seed of
  unknown references may cite the work.
- A failed lookup behind `references` or a search by citations names its
  source and any daily cap reached; a spent quota with no stated reset
  says when it is tried again (`until`).
- `--style Foo.CSL` is read as a file. Counts given the app saturate
  rather than wrap.

## 0.20.1

Fixed:

- Standard journal abbreviations that leave out or join words ("Proc Biol
  Sci", "Philos Trans R Soc Lond B Biol Sci", "Acta Crystallogr D Biol
  Crystallogr", "Eur J Cardiothorac Surg") are no longer taken for another
  journal, so a correct citation using one verifies. A subject word left
  out still means another journal ("Am J Med" isn't the American Journal
  of Emergency Medicine).
- An author's initials or a word of the title no longer count as the
  journal ("SR" for Scientific Reports, "mice" for Medicine), nor outweigh
  the journal a reference names.
- Structured claims compare the journal as free-text references do, so a
  `container` the rules can't recognise no longer lowers a correct
  citation; only one that's clearly another journal is a `mismatch`.
- An OpenAlex work whose main copy is in a repository (PubMed Central,
  arXiv) gives the journal it appeared in, not the repository; an
  `article` in a book series is a chapter.
- A merged work's cited-by count no longer depends on the order sources
  answer in.
- Another edition of a cited book stays `plausible` when the claim gives
  the publisher as its container; a chapter cited from a book no longer
  makes another edition of the book plausible.
- More organisations ("Public Health England", "World Bank", "Cancer
  Research UK", "Sepsis Task Force") are no longer split into given and
  family names in exports.

## 0.20.0

Fixed:

- A citation naming the wrong journal no longer verifies (#80). The
  journal a reference cites is read where it can be told (the words
  before the volume and page, Vancouver or APA), and a journal article
  cited in a journal that's clearly another, or a preprint cited by
  journal, volume and page, is `plausible` at most, with
  `checks.container` a `mismatch` and a `journal` difference whose `cited`
  is the journal as cited. Abbreviations, "The", spelling variants and
  names inside names ("Proc Natl Acad Sci U S A") aren't another journal;
  books, proceedings and references without a clear journal are left
  alone.
- `read` carries the journal read from free text (`container`).
- APA references without an issue ("Journal Name, 146, 106274.") give
  their volume and page.
- One work shows the same cited-by count, open access and date whichever
  source's record won the merge (#81). Open access is the more
  informative (a free copy with a PDF over `is_oa: false`); between
  records of one version (a shared DOI or PMID), the cited-by count is
  OpenAlex's when one has it, else the larger, and `issued` the earliest
  date (online first), the more precise of dates that overlap, with the
  other year kept in `other_years`. `verify` now agrees with `lookup`,
  which asks OpenAlex first.

Changed:

- Search hits, and a candidate's `found_by` sightings, leave `score` out
  when the source gives none (OpenAlex sorted by citations), rather than
  `null` (#81). The MCP `search` output schema's `score` is a number, and
  optional, as before.
