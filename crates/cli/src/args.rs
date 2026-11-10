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

Environment:
  OPENALEX_API_KEY  OpenAlex key ($1/day free budget; $0.10/day without)
  ASPERO_MAILTO     contact address for polite API pools (Crossref)";

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

    /// Match a messy citation (or identifier) to candidate works.
    #[command(after_help = "\
If the input parses as an identifier it is looked up. Otherwise it goes to
every provider that can search: citation matchers (Crossref) get the whole
reference, others a keyword search. Candidates are listed per provider with
that provider's score; scores are not comparable across providers.

Confidence scoring across providers is not implemented yet. Lookalikes exist
(re-deposits with the same title and authors), so check year and venue.

Examples:
  aspero resolve \"LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015\"
  aspero resolve \"Vaswani 2017 Attention is all you need\" --limit 3
  echo 'arXiv:1706.03762' | aspero resolve -")]
    Resolve {
        /// The citation; `-` reads all of stdin.
        text: String,
        /// Candidates per provider.
        #[arg(long, default_value_t = 3)]
        limit: u32,
    },
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
