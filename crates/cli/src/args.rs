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
  9  a citation does not check out (verify: mismatch or only plausible)

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
    /// Output format. `auto`: table on a terminal, JSON otherwise.
    #[arg(long, global = true, value_enum, default_value_t = Format::Auto)]
    pub format: Format,

    /// Include every known field, not just the lean default set.
    #[arg(long, global = true)]
    pub full: bool,

    /// Don't read or write the answer cache.
    #[arg(long, global = true)]
    pub no_cache: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    Auto,
    Json,
    /// One JSON object per line (per work or per candidate).
    Jsonl,
    Table,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Look up works by identifier.
    #[command(after_help = "\
Accepts DOIs, arXiv IDs, PMIDs, OpenAlex IDs and their URLs, bare or prefixed:
  10.1038/nature14539   doi:10.1038/nature14539   https://doi.org/10.1038/nature14539
  arXiv:1706.03762v5    https://arxiv.org/abs/1706.03762
  pmid:26017442         W2919115771

Bare digits are ambiguous; prefix them (pmid:, corpusid:).

Examples:
  aspero lookup 10.1038/nature14539 arXiv:1706.03762
  cat dois.txt | aspero lookup -        # one identifier per line
  aspero lookup --format jsonl W2919115771")]
    Lookup {
        /// Identifiers; `-` (or none, when piped) reads one per line from stdin.
        ids: Vec<String>,
    },

    /// Search one provider by keywords.
    #[command(after_help = "\
Searches are not interchangeable across providers (rankings differ), so one
provider answers: --source, or the first that can.

Filters a provider can't apply are reported in `ignored_filters`, never
silently dropped. OpenAlex searches cost $0.001 each.

Examples:
  aspero search \"retrieval augmented generation\" --year 2020.. --limit 5
  aspero search transformer --source crossref --type article-journal
  aspero search \"protein folding\" --oa --cursor <next_cursor from last page>")]
    Search {
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

Stops as soon as a free source verifies the claim; OpenAlex title searches
($0.001 each, at most two per claim) run only when needed.

Batch input on stdin, one claim per line, as text or JSON:
  {\"title\": \"...\", \"authors\": [\"...\"], \"year\": 2017, \"doi\": \"...\"}
  JSON keys: text, title, authors, year, doi, arxiv, id, ids

Examples:
  aspero verify \"Vaswani et al. 2017. Attention is all you need. arXiv:1706.03762\"
  aspero verify --title \"Deep learning\" --author LeCun --year 2015 --id 10.1038/nature14539
  aspero verify - < citations.jsonl")]
    Verify {
        #[command(flatten)]
        claim: ClaimArgs,
    },

    /// Inspect or empty the answer cache.
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },
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
    /// Claimed identifier (repeatable): DOI, arXiv ID, PMID, URL...
    #[arg(long = "id")]
    pub ids: Vec<String>,
    /// Results per provider search.
    #[arg(long, default_value_t = 5)]
    pub limit: u32,
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
    /// Results per page.
    #[arg(long, default_value_t = 10)]
    pub limit: u32,
    /// Continue from a previous page's `next_cursor`.
    #[arg(long)]
    pub cursor: Option<String>,
}
