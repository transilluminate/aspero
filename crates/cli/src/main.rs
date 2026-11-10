//! `aspero`: a thin command-line adapter over `aspero-engine`.
//!
//! Each subcommand lives in [`cmd`]; this file only parses arguments, builds
//! the engine, dispatches, and turns the outcome into an exit code.

mod args;
mod cmd;
mod exit;
mod output;

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use aspero_engine::{
    CrossrefConfig, DataciteConfig, DoiConfig, Engine, EuropePmcConfig, OpenAlexConfig,
    OpenLibraryConfig, SecretString, SqliteCache, StandardConfig, standard,
};
use clap::Parser;
use serde_json::json;

use crate::args::{Cli, Command, Format};
use crate::cmd::{Failure, Out};
use crate::exit::Exit;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    init_logging(cli.global.verbose);
    let format = match cli.global.format {
        Format::Auto if std::io::stdout().is_terminal() => Format::Table,
        Format::Auto => Format::Json,
        f => f,
    };
    let out = Out {
        format,
        full: cli.global.full,
        abstracts: cli.global.abstracts,
    };
    match run(cli, out).await {
        Ok(exit) => exit.code(),
        Err(failure) => {
            if format == Format::Table {
                eprintln!("error: {}", failure.message);
            } else {
                let category = format!("{:?}", failure.exit).to_lowercase();
                eprintln!(
                    "{}",
                    json!({"error": {"category": category, "message": failure.message}})
                );
            }
            failure.exit.code()
        }
    }
}

/// `-v`: aspero's own events (requests, cache, retries) at info; `-vv`:
/// everything at debug, including the HTTP stack. Always to stderr, so
/// stdout stays clean for pipes.
fn init_logging(verbosity: u8) {
    use tracing_subscriber::filter::{LevelFilter, Targets};
    use tracing_subscriber::prelude::*;

    let targets = match verbosity {
        0 => return,
        1 => Targets::new().with_target("aspero", LevelFilter::INFO),
        _ => Targets::new().with_default(LevelFilter::DEBUG),
    };
    let layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .with_timer(tracing_subscriber::fmt::time::Uptime::default())
        .with_target(verbosity > 1)
        .compact();
    tracing_subscriber::registry()
        .with(layer.with_filter(targets))
        .init();
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// `$ASPERO_CACHE_DIR/cache.sqlite`, or the platform cache directory
/// (macOS: `~/Library/Caches/aspero`).
fn cache_path() -> Result<PathBuf, Failure> {
    let dir = match env("ASPERO_CACHE_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => directories::ProjectDirs::from("", "", "aspero")
            .map(|d| d.cache_dir().to_path_buf())
            .ok_or_else(|| Failure {
                exit: Exit::Internal,
                message: "no home directory for the cache; set ASPERO_CACHE_DIR".into(),
            })?,
    };
    Ok(dir.join("cache.sqlite"))
}

/// The cache, unless disabled. A cache that can't be opened is a warning,
/// not a failure: everything works without it, just slower and costlier.
fn open_cache(disabled: bool) -> Option<Arc<SqliteCache>> {
    if disabled {
        return None;
    }
    let opened = cache_path()
        .map_err(|f| f.message)
        .and_then(|path| SqliteCache::open(&path).map_err(|e| format!("{e} ({})", path.display())));
    match opened {
        Ok(cache) => Some(Arc::new(cache)),
        Err(message) => {
            eprintln!("warning: running without a cache: {message}");
            None
        }
    }
}

/// Credentials and contact details come from the environment for now; the
/// config file and keyring (DESIGN §7) come later.
fn engine(cache: Option<Arc<SqliteCache>>) -> Result<Engine, Failure> {
    let mut config = StandardConfig::default();
    config.mailto = env("ASPERO_MAILTO");
    config.cache = cache.map(|c| c as _);
    let mut openalex = OpenAlexConfig::default();
    openalex.api_key = env("OPENALEX_API_KEY").map(SecretString::from);
    config.openalex = Some(openalex);
    config.crossref = Some(CrossrefConfig::default());
    config.datacite = Some(DataciteConfig::default());
    config.doi = Some(DoiConfig::default());
    config.europepmc = Some(EuropePmcConfig::default());
    config.openlibrary = Some(OpenLibraryConfig::default());
    standard(config).map_err(|e| Failure {
        exit: Exit::Internal,
        message: format!("could not set up HTTP client: {e}"),
    })
}

async fn run(cli: Cli, out: Out) -> Result<Exit, Failure> {
    if let Command::Cache { action } = cli.command {
        return cmd::cache::run(cache_path()?, action, out);
    }
    let engine = engine(open_cache(cli.global.no_cache))?;
    match cli.command {
        Command::Lookup { ids } => cmd::lookup::run(&engine, ids, out).await,
        Command::Search { query, options } => cmd::search::run(&engine, query, options, out).await,
        Command::Resolve { claim } => cmd::resolve::run(&engine, claim, out).await,
        Command::Verify { claim } => cmd::verify::run(&engine, claim, out).await,
        Command::Check {
            path,
            fail_on,
            limit,
        } => cmd::check::run(&engine, path, fail_on, limit, out).await,
        Command::Cache { .. } => unreachable!("handled above"),
    }
}
