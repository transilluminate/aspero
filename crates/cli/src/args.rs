//! Command-line surface. Help text is part of the product: agents read it,
//! so every command carries examples.

use clap::{Args, Parser, Subcommand, ValueEnum};

const EXIT_CODES: &str = "\
Exit codes:
  0  success
  1  internal error
  2  invalid usage or input (bad identifier, unknown option)
  3  one or more works not found
  4  provider unavailable (network, timeout, server error)
  5  throttled (rate limit or spent usage budget)
  6  credentials rejected
  7  unsupported by the enabled providers
  8  unexpected provider response
  9  a citation does not check out (verify: mismatch or only plausible;
     check: an entry reached --fail-on)

With several inputs (a batch on stdin), the exit code is the most serious
problem among them; results for every input are still written.

Environment:
  OPENALEX_API_KEY  OpenAlex key ($1/day free budget; $0.10/day without)
  ASPERO_MAILTO     contact address for polite API pools (Crossref)
  ASPERO_CACHE_DIR  where to keep the answer cache (default: platform cache dir)

Answers are cached (records 7 days, not-found 1 day, searches 1 day) and
served stale if a provider fails; JSON marks them \"served\": \"cache\"/\"stale\".";

#[derive(Debug, Parser)]
#[command(
    name = "aspero",
    version,
    about = "Find, resolve and verify scholarly literature",
    long_about = "Find, resolve and verify scholarly literature via OpenAlex and Crossref.\n\n\
        Output is JSON when stdout is not a terminal, so agents can pipe it.",
    after_help = EXIT_CODES,
)]
pub struct Cli {
    #[command(flatten)]
    pub global: Global,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Args)]
pub struct Global {
    /// Output format. `auto`: tables on a terminal, JSON otherwise.
    #[arg(long, global = true, value_enum, default_value_t = Format::Auto)]
    pub format: Format,

    /// Include every known field, not just the lean default set. The
    /// JSON shape of works is in `aspero lookup --help`.
    #[arg(long, global = true)]
    pub full: bool,

    /// Log each provider request, cache hit and retry to stderr, timed from
    /// start, so concurrency and pacing are visible. `-vv` adds the HTTP
    /// stack's own debug output.
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Don't read or write the answer cache (its file still paces requests
    /// shared with other aspero processes).
    #[arg(long, global = true)]
    pub no_cache: bool,

    /// Include abstracts, looked up at other providers when the first one
    /// lacks it. JSON gives `abstract` (null if none exists) and
    /// `abstract_source`.
    #[arg(long = "abstract", global = true)]
    pub abstracts: bool,

    /// For `--format cite` (which it implies): a bundled style (`aspero
    /// styles` lists them) or a `.csl` file.
    #[arg(long, global = true, value_name = "ID|FILE")]
    pub style: Option<String>,

    /// The language of a style's terms ("et al."): `en-US` or `en-GB`. By
    /// default, the style's own.
    #[arg(long, global = true)]
    pub locale: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    Auto,
    Json,
    /// One JSON object per line (per work or per candidate).
    Jsonl,
    /// Aligned columns for list-shaped results; single results as text.
    Table,
    /// A readable report: each result in detail.
    Text,
    /// The works found as BibTeX entries (lookup, search, verify).
    Bibtex,
    /// The works found as RIS records (lookup, search, verify).
    Ris,
    /// The works found as a reference list in `--style` (lookup, search,
    /// verify).
    Cite,
}

impl Format {
    /// Meant for people (table or text) rather than programs.
    pub fn is_human(self) -> bool {
        matches!(self, Format::Table | Format::Text)
    }

    /// The works found, for a reference manager.
    pub fn is_export(self) -> bool {
        matches!(self, Format::Bibtex | Format::Ris | Format::Cite)
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Look up works by identifier.
    #[command(after_help = "\
Accepts DOIs, arXiv IDs, PMIDs, PMCIDs, NCBI Bookshelf IDs, ISBNs, OpenAlex
IDs and their URLs, bare or prefixed; also publisher URLs with a DOI in the
path (…/10.3389/fneur.2019.01343/full, page suffixes dropped):
  10.1038/nature14539   doi:10.1038/nature14539   https://doi.org/10.1038/nature14539
  arXiv:1706.03762v5    https://arxiv.org/abs/1706.03762
  pmid:26017442         PMC6716493   NBK526056   W2919115771

Bare digits are ambiguous; prefix them (pmid:, corpusid:).

JSON works:
  canonical    the work's key, the same whichever of its IDs found it (a
               DOI, else arXiv, PMID, PMCID, …): dedupe works on it
  authors      [{\"name\": \"Yann LeCun\", \"orcid\": \"...\"}, ...], the same
               shape with or without --full (which adds given and family)
  updates      notices about the work: [{\"kind\": \"retraction\", \"notice\":
               \"doi:...\", \"date\": \"...\", \"source\": \"...\"}]; kinds:
               retraction, withdrawal, expression_of_concern, correction,
               new_version (superseded; the notice is the newer work), …
               Rows sharing a `notice` are one event reported by several
               sources (or found via several identifiers): dedupe on it. A
               notice that is itself flagged names itself as `notice`.
               With --full, `date` is when the notice appeared (online or
               print, whichever first); `deposited` is the date the source
               gave, when later (often the issue's). Both come from the
               notice deposit and the registry's record of the notice, so
               they can differ from looking up the notice DOI itself when
               another provider answers that wrongly; --full then shows
               `disagreements` there.
  type         CSL types; notices themselves are \"erratum\" or \"retraction\"
  update_to    for a notice itself, the works it updates: [{\"kind\",
               \"work\", \"date\", \"source\"}]
  open_access  whether a free, legal copy exists: {\"is_oa\": true,
               \"status\": gold|diamond|hybrid|bronze|green|closed, \"pdf_url\",
               \"url\", \"license\", \"version\", \"source\"}; absent if unknown.
               \"closed\" means no free copy is known to the source (OpenAlex
               carries Unpaywall's data), not that none exists
  `lookup --full` also asks Crossref for notices in both directions, and
  other providers for open access when the first didn't say. A superseded
  work's result gains `latest`: the newest version, following the chain
  (Cochrane .pub5 → .pub6 → .pub7). A record that contradicts the DOI's
  registry (Crossref) on year, volume, page or title gains `disagreements`:
  [{\"field\": \"page\", \"value\": \"1541\", \"crossref\": \"1823\"}]; aggregators
  occasionally merge two works into one record.

Examples:
  aspero lookup 10.1038/nature14539 arXiv:1706.03762
  cat dois.txt | aspero lookup -        # one identifier per line
  aspero scan --format jsonl notes.md | aspero lookup -   # or JSON lines with an \"id\"
  aspero lookup --format jsonl W2919115771")]
    Lookup {
        /// Identifiers; `-` (or none, when piped) reads one per line from stdin.
        ids: Vec<String>,
    },

    /// Search one provider by keywords, author, title or journal.
    #[command(after_help = "\
Searches are not interchangeable across providers (rankings differ), so one
provider answers: --source, or the first that can.

Filters a provider can't apply are reported in `ignored_filters`, never
silently dropped. A provider that can't search by --author, --title,
--container or the --sort asked for fails instead. OpenAlex searches cost
$0.001 each, and a --container search one more.

--query takes the search as a JSON tree instead of keywords and fields: all,
any and none of terms (keywords, title, author, container, abstract,
author_id; \"phrase\": true for the words together), filters (years, type,
open_access, language, not_retracted, has_abstract) and relations (cites,
cited_by, any identifier: OpenAlex only). A provider that can't say the
tree fails and says why.

Examples:
  aspero search \"retrieval augmented generation\" --year 2020.. --limit 5
  aspero search transformer --source crossref --type article-journal
  aspero search --author lecun --title \"deep learning\" --sort most-cited
  aspero search \"protein folding\" --oa --cursor <next_cursor from last page>
  aspero search --query '{\"all\": [{\"title\": \"alphafold\"}, {\"not_retracted\": true}]}'
  aspero search --source europepmc --query '{\"any\": [{\"title\": \"sepsis\"}, {\"abstract\": \"septic shock\", \"phrase\": true}]}'
  aspero search --query '{\"all\": [{\"cites\": \"10.1038/nature14539\"}, {\"title\": \"survey\"}]}'")]
    Search {
        /// Keywords; may be left out when --author, --title, --container or
        /// --query is given.
        #[arg(required_unless_present_any = ["author", "title", "container", "tree"], default_value = "")]
        query: String,
        #[command(flatten)]
        options: SearchOptions,
    },

    /// Rank candidate works for a citation, with per-field checks.
    #[command(after_help = "\
The citation can be free text in any style, an identifier, structured
fields, or a mix. Candidates come from identifier lookups, Crossref's
citation matcher and OpenAlex title searches; records sharing an
identifier are merged. Each is scored on title, authors and year.

Use `verify` for a yes/no answer; `resolve` shows every candidate.

Examples:
  aspero resolve \"LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015\"
  aspero resolve --title \"Attention is all you need\" --author Vaswani --year 2017")]
    Resolve {
        #[command(flatten)]
        claim: ClaimArgs,
    },

    /// Check that a citation refers to a real work, and that its details agree.
    #[command(after_help = "\
Verdicts:
  verified      a matching work exists (and so do its claimed identifiers)
  plausible     a work partly matches; check the details          (exit 9)
  mismatch      the claimed DOI/arXiv ID doesn't exist or is another
                work's; `reason` names the work the citation matches (exit 9)
  not_found     no matching work                                   (exit 3)
  unverifiable  providers failed                                   (exit 4)

A wrong year is treated as a strong mismatch: re-deposits of famous papers
exist with the same title and authors but a different year.

Stops early when the claim's identifiers, or two providers, confirm a match;
otherwise OpenAlex title searches run too ($0.001 each, at most two per claim).

Batch input on stdin (`verify -`), one claim per line: a free-text
reference, or a JSON object of what you know (more reliable than text):
  {\"key\": \"pe.md:7\", \"title\": \"...\", \"authors\": [\"Meyer G\"], \"year\": 2014,
   \"journal\": \"N Engl J Med\", \"volume\": 370, \"page\": \"1402-11\", \"doi\": \"...\"}
  keys: text, title, authors, year, journal (or container), volume, page,
        doi, arxiv, id, ids, and key: anything, echoed back untouched
Unknown keys are refused. To paste a reference list as it is (numbered,
one per paragraph, wrapped), use --split instead. With --format jsonl, one result per line as
each finishes, in any order: match them up by `input` or your `key`. Each
also carries the `claim` as understood. Other formats keep input order.

The journal, volume and first page are checked too, from fields or from
the text (\"N Engl J Med 2017;376:2545\"). A different volume or page makes
a match plausible at best; it also tells an article from replies to it.

Examples:
  aspero verify \"Vaswani et al. 2017. Attention is all you need. arXiv:1706.03762\"
  aspero verify --title \"Deep learning\" --author LeCun --year 2015 --id 10.1038/nature14539
  aspero verify --title \"Fibrinolysis for patients with intermediate-risk pulmonary embolism\" --journal \"N Engl J Med\" --volume 370 --page 1402
  aspero verify --format jsonl - < citations.jsonl
  pbpaste | aspero verify --split -")]
    Verify {
        #[command(flatten)]
        claim: ClaimArgs,
    },

    /// Check every entry of a BibTeX/BibLaTeX, RIS or CSL-JSON file, or the
    /// Zotero or Mendeley citations in a Word document, CI-style.
    #[command(after_help = "\
The format is told from the contents; a .docx file's citations are read
from the fields Zotero and Mendeley keep in it. Each entry's title, authors, year,
journal, volume, pages and identifiers (DOI, arXiv, PMID, ISBN, and those
in URLs) are verified, then reported with fixes:
  error    retracted or withdrawn; not found; identifier wrong or
           nonexistent (with the right one); year off by more than one
  warning  expression of concern; only a plausible match; duplicate
           entries; providers failed
  info     add a DOI the entry lacks; year off by one (often preprint vs
           published, or online vs print); a published correction

Exits 9 if any entry reaches --fail-on (default: error), so it can gate a
pre-commit hook or CI job. JSON output has a summary and every finding.

Examples:
  aspero check refs.bib
  aspero check paper.docx
  aspero check --fail-on warning --format json thesis.bib > report.json
  cat refs.bib | aspero check -")]
    Check {
        /// The .bib file; `-` reads stdin.
        path: String,
        /// Lowest severity that makes the check fail (exit 9).
        #[arg(long, value_enum, default_value_t = FailOn::Error)]
        fail_on: FailOn,
        /// Results per provider search.
        #[arg(long, default_value_t = aspero_engine::SEARCH_LIMIT)]
        limit: u32,
    },

    /// List the identifiers mentioned in documents, without looking them up.
    #[command(after_help = "\
Finds DOIs, PMIDs, PMCIDs, NCBI Bookshelf IDs (NBK…), arXiv IDs and ISBNs
in text or Markdown: one record per mention, with file, line, column, the
identifier, the text as written and its line for context. Which mentions
matter is yours to decide; scan only says what is a valid identifier.
`id` is canonical (doi:10.1056/nejmoa1302097) and equals `lookup`'s `id`
for the same identifier: join results on it.

Cleaned like pasted input: wrapping brackets, quotes and emphasis and
trailing punctuation are dropped, brackets inside a DOI kept
(10.1016/S0140-6736(10)60835-5). Markdown links and tables are split.
Publisher URLs give their DOI without page suffixes (/full, /pdf, .pdf).
DOIs, PMC… and NBK… count anywhere; PMIDs, ISBNs and arXiv IDs only when
labelled (PMID: 123, ISBN 978-…), prefixed (arXiv:…) or as URLs. A DOI
or URL broken across lines, or spaced out after its punctuation as some
PDF copies are (https:// doi. org/ 10. …), is rejoined. Nothing is
fetched.

Examples:
  aspero scan notes/*.md
  aspero scan --format jsonl page.md | aspero lookup -    # results echo each mention as `from`
  pdftotext paper.pdf - | aspero scan -")]
    Scan {
        /// Text or Markdown files; `-` (or none, when piped) reads stdin.
        files: Vec<std::path::PathBuf>,
    },

    /// Which work a document is, from its first page's text.
    #[command(after_help = "\
A first page prints the paper's own DOI, but often others too (the
journal's, data, a related article, the first references), so none is
taken on trust. Each identifier on the page, those in the header and
footer first (at most six), is looked up, and its work must be confirmed:
by --title-hint (the largest type at the top of the page) or --title (the
file's title metadata, ignored when junk such as \"Microsoft Word -
draft3.docx\"), or by its own title and first author appearing on the page.
With none confirmed, the title and the top of the page are verified as a
citation. `decided_by` says which: identifier_and_title,
identifier_and_page, only_identifier (the page's one identifier, no title
to check) or title_search. Exits 3 when nothing is sure enough.

Examples:
  pdftotext -l 1 paper.pdf - | aspero identify --title \"$(pdfinfo paper.pdf | sed -n 's/^Title: *//p')\"
  aspero identify page1.txt --title-hint 'Deep learning'")]
    Identify {
        /// The first page as text; `-` (or none, when piped) reads stdin.
        file: Option<std::path::PathBuf>,
        /// The document's title metadata (ignored when junk).
        #[arg(long)]
        title: Option<String>,
        /// The largest type at the top of the page.
        #[arg(long)]
        title_hint: Option<String>,
    },

    /// Serve aspero's tools to an AI agent over the Model Context Protocol.
    #[command(after_help = "\
Speaks MCP on stdin and stdout (logs go to stderr), for an agent to launch.
With Claude Code, for example:
  claude mcp add aspero -- aspero mcp
  claude mcp add aspero -- aspero mcp --budget 0.5

Tools: search (a query tree, as `search --query` takes, or keywords),
search_supports, lookup, verify (one citation or a batch), identify (a
first page's text), cites and cited_by (works citing a work, and its
references). Each answers in the JSON shapes the other commands print, with
a short text summary; `full` gives every field.

Spending: OpenAlex searches cost $0.001 each (lookups are free). Each
session, one connection, may spend up to --budget (USD); once it has, tools
that may spend are refused with a budget error. Every result reports its
own usage and the session's spend so far.

Same environment (keys and contact address, as the agent passes them on),
cache and pacing as the other commands; -v logs requests to stderr. Exits
when stdin closes.")]
    Mcp {
        /// The most one session may spend, in USD.
        #[arg(long, value_name = "USD", default_value_t = aspero_mcp::DEFAULT_BUDGET_USD, value_parser = budget)]
        budget: f64,
    },

    /// Inspect or empty the answer cache.
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },

    /// List the bundled citation styles for `--format cite`.
    #[command(after_help = "\
Any independent CSL style also works: pass its .csl file to --style. Styles
from https://github.com/citation-style-language/styles are licensed under
CC BY-SA 3.0; --credits lists the bundled ones' authors.

Examples:
  aspero styles
  aspero lookup --style vancouver 10.1038/nature14539
  aspero verify --style apa --format cite - < citations.txt
  aspero lookup --style ~/styles/the-lancet.csl 10.1038/nature14539")]
    Styles {
        /// Print the bundled styles' license and authors.
        #[arg(long)]
        credits: bool,
    },
}

/// `--budget`: a positive number of dollars.
fn budget(s: &str) -> Result<f64, String> {
    match s.parse::<f64>() {
        Ok(usd) if usd.is_finite() && usd > 0.0 => Ok(usd),
        _ => Err(format!("{s:?} is not a positive amount of USD")),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SortBy {
    /// Best match first.
    Relevance,
    /// Most recently published first.
    Newest,
    MostCited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FailOn {
    Error,
    Warning,
    Info,
    Never,
}

#[derive(Debug, Subcommand)]
pub enum CacheAction {
    /// Show what the cache holds.
    Stats,
    /// Empty the cache.
    Clear,
    /// Print the cache file's path.
    Path,
}

/// A citation claim: free text and/or fields.
#[derive(Debug, Args)]
pub struct ClaimArgs {
    /// The citation as free text or an identifier; `-` reads one claim per
    /// stdin line.
    pub text: Option<String>,
    /// Claimed title.
    #[arg(long)]
    pub title: Option<String>,
    /// Claimed author (repeatable), e.g. "Vaswani, A." or "Ashish Vaswani".
    #[arg(long = "author")]
    pub authors: Vec<String>,
    /// Claimed publication year.
    #[arg(long)]
    pub year: Option<i16>,
    /// Claimed journal, full or abbreviated ("N Engl J Med").
    #[arg(long, visible_alias = "container")]
    pub journal: Option<String>,
    /// Claimed volume.
    #[arg(long)]
    pub volume: Option<String>,
    /// Claimed first page (or page range).
    #[arg(long, visible_alias = "pages")]
    pub page: Option<String>,
    /// Claimed identifier (repeatable): DOI, arXiv ID, PMID, URL...
    #[arg(long = "id")]
    pub ids: Vec<String>,
    /// Results per provider search.
    #[arg(long, default_value_t = aspero_engine::SEARCH_LIMIT)]
    pub limit: u32,
    /// Split the text (or stdin, with `-`) into references, as pasted from
    /// a reference list: numbered, bulleted, one per paragraph or line,
    /// wrapped lines rejoined.
    #[arg(long, conflicts_with_all = ["title", "authors", "year", "journal", "volume", "page", "ids"])]
    pub split: bool,
}

#[derive(Debug, Args)]
pub struct SearchOptions {
    /// Provider to search (e.g. openalex, crossref).
    #[arg(long)]
    pub source: Option<String>,
    /// Publication years: 2017, 2017..2020, 2017.., ..2020
    #[arg(long)]
    pub year: Option<String>,
    /// Work type as a CSL type, e.g. article-journal, paper-conference, article (preprint).
    #[arg(long = "type")]
    pub work_type: Option<String>,
    /// Only open-access works.
    #[arg(long)]
    pub oa: bool,
    /// An author's name.
    #[arg(long)]
    pub author: Option<String>,
    /// Words of the title.
    #[arg(long)]
    pub title: Option<String>,
    /// Where the work appeared: the journal, or the book for a chapter.
    #[arg(long, visible_alias = "journal")]
    pub container: Option<String>,
    /// The search as a JSON tree of all, any and none, instead of keywords,
    /// --author, --title and --container (see below).
    #[arg(long = "query", id = "tree", value_name = "JSON", conflicts_with_all = ["author", "title", "container"])]
    pub tree: Option<String>,
    /// Order of the hits.
    #[arg(long, value_enum, default_value_t = SortBy::Relevance)]
    pub sort: SortBy,
    /// Results per page.
    #[arg(long, default_value_t = 10)]
    pub limit: u32,
    /// Continue from a previous page's `next_cursor`, with the same
    /// `--source` (the printed `next page` command does both).
    #[arg(long)]
    pub cursor: Option<String>,
}
