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

use aspero_engine::{CallOptions, Engine, SecretString, SqliteCache, StandardConfig, standard};
use clap::Parser;
use serde_json::json;

use crate::args::{Cli, Command, Format, Global};
use crate::cmd::{Failure, Out};
use crate::exit::Exit;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    reset_sigpipe();
    // A bare `aspero`: the overview, not clap's usage error.
    if std::env::args_os().len() == 1 {
        print!("{}", args::OVERVIEW);
        return ExitCode::SUCCESS;
    }
    let cli = Cli::parse();
    init_logging(cli.global.verbose);
    let format = match cli.global.format {
        Format::Auto if cli.global.style.is_some() => Format::Cite,
        Format::Auto if std::io::stdout().is_terminal() => Format::Table,
        Format::Auto => Format::Json,
        f => f,
    };
    let out = Out {
        format,
        full: cli.global.full,
        abstracts: cli.global.abstracts,
        style: cli.global.style.clone(),
        locale: cli.global.locale.clone(),
    };
    let exports = matches!(
        cli.command,
        Command::Lookup { .. } | Command::Search { .. } | Command::Verify { .. }
    );
    let result = match format.is_export() && !exports {
        true => Err(Failure::usage(
            "--format bibtex, ris and cite are for lookup, search and verify",
        )),
        false => run(cli, out.clone()).await,
    };
    match result {
        Ok(exit) => exit.code(),
        Err(failure) => {
            if format.is_human() || format.is_export() {
                eprintln!("error: {}", failure.message);
            } else {
                let category = failure.exit.as_str();
                eprintln!(
                    "{}",
                    json!({"error": {"category": category, "message": failure.message}})
                );
            }
            failure.exit.code()
        }
    }
}

/// Dies quietly on a closed stdout (`aspero scan big.txt | head -1`), as
/// other Unix tools do, instead of panicking in `println!`. Rust ignores
/// SIGPIPE by default.
#[cfg(unix)]
#[allow(unsafe_code)]
fn reset_sigpipe() {
    // SAFETY: called first thing in `main`, before any other threads exist.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn reset_sigpipe() {}

/// Warnings by default (a step of an analysis that failed, and why), but
/// not each failed request, which may be retried and is reported where it
/// matters; `-v`: aspero's own events (requests, cache, retries) at info;
/// `-vv`: everything at debug, including the HTTP stack. Always to stderr,
/// so stdout stays clean for pipes.
fn init_logging(verbosity: u8) {
    use tracing_subscriber::filter::{LevelFilter, Targets};
    use tracing_subscriber::prelude::*;

    let targets = match verbosity {
        0 => Targets::new()
            .with_target("aspero", LevelFilter::WARN)
            .with_target("aspero::http", LevelFilter::OFF),
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

/// The cache file, held to `max_bytes`. One that can't be opened is a
/// warning, not a failure: everything works without it, just slower and
/// costlier.
fn open_cache(max_bytes: Option<u64>) -> Option<Arc<SqliteCache>> {
    let opened = cache_path().map_err(|f| f.message).and_then(|path| {
        let cache = SqliteCache::open(&path).map_err(|e| format!("{e} ({})", path.display()))?;
        if let Err(e) = cache.set_max_bytes(max_bytes) {
            eprintln!("warning: couldn't hold the cache to its size limit: {e}");
        }
        Ok(cache)
    });
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
/// `file` paces requests across processes (parallel agents), and caches
/// answers unless `no_cache`.
fn engine(
    file: Option<Arc<SqliteCache>>,
    no_cache: bool,
    global: &Global,
) -> Result<Engine, Failure> {
    let mut config = StandardConfig::all(env("OPENALEX_API_KEY").map(SecretString::from));
    config.mailto = env("ASPERO_MAILTO");
    config.openalex_daily_cap = global.openalex_cap;
    config.cache_policy = global.cache_policy();
    config
        .disable(&global.without)
        .map_err(|e| Failure::usage(format!("--without: {e}")))?;
    config.pacing = file.clone().map(|c| c as _);
    config.cache = file.filter(|_| !no_cache).map(|c| c as _);
    standard(config).map_err(|e| Failure {
        exit: Exit::Internal,
        message: format!("could not set up HTTP client: {e}"),
    })
}

async fn run(cli: Cli, out: Out) -> Result<Exit, Failure> {
    match cli.command {
        Command::Cache { action } => {
            return cmd::cache::run(cache_path()?, action, cli.global.cache_max_bytes(), out);
        }
        Command::Scan { files } => return cmd::scan::run(files, out),
        Command::Styles { credits } => return cmd::styles::run(credits, out),
        _ => {}
    }
    let file = open_cache(cli.global.cache_max_bytes());
    let mut engine = engine(file, cli.global.no_cache, &cli.global)?;
    if cli.global.fresh {
        engine = engine.with_options(CallOptions::fresh());
    }
    match cli.command {
        Command::Lookup { ids } => cmd::lookup::run(&engine, ids, out).await,
        Command::Search { query, options } => cmd::search::run(&engine, query, options, out).await,
        Command::Resolve { claim } => cmd::resolve::run(&engine, claim, out).await,
        Command::Verify { claim } => cmd::verify::run(&engine, claim, out).await,
        Command::Identify {
            file,
            title,
            title_hint,
        } => cmd::identify::run(&engine, file, title, title_hint, out).await,
        Command::Check {
            path,
            fail_on,
            limit,
        } => cmd::check::run(&engine, path, fail_on, limit, out).await,
        Command::References { id, limit, cursor } => {
            cmd::graph::references(&engine, id, limit, cursor, out).await
        }
        Command::Analyse {
            inputs,
            kinds,
            limit,
            budget,
            estimate,
        } => cmd::graph::analyse(&engine, inputs, kinds, limit, budget, estimate, out).await,
        Command::Mcp { budget } => cmd::mcp::run(engine, budget).await,
        Command::Budget => cmd::budget::run(&engine, out).await,
        Command::Cache { .. } | Command::Scan { .. } | Command::Styles { .. } => {
            unreachable!("handled above")
        }
    }
}
