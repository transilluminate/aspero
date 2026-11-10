//! `aspero`: a thin command-line adapter over `aspero-engine`.
//!
//! Each subcommand lives in [`cmd`]; this file only parses arguments, builds
//! the engine, dispatches, and turns the outcome into an exit code.

mod args;
mod cmd;
mod exit;
mod output;

use std::io::IsTerminal;
use std::process::ExitCode;

use aspero_engine::{
    CrossrefConfig, Engine, OpenAlexConfig, SecretString, StandardConfig, standard,
};
use clap::Parser;
use serde_json::json;

use crate::args::{Cli, Command, Format};
use crate::cmd::{Failure, Out};
use crate::exit::Exit;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let format = match cli.global.format {
        Format::Auto if std::io::stdout().is_terminal() => Format::Table,
        Format::Auto => Format::Json,
        f => f,
    };
    let out = Out {
        format,
        full: cli.global.full,
    };
    let result = match engine() {
        Ok(engine) => run(&engine, cli.command, out).await,
        Err(f) => Err(f),
    };
    match result {
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

/// Credentials and contact details come from the environment for now; the
/// config file and keyring (DESIGN §7) come later.
fn engine() -> Result<Engine, Failure> {
    let env = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    let mut config = StandardConfig::default();
    config.mailto = env("ASPERO_MAILTO");
    let mut openalex = OpenAlexConfig::default();
    openalex.api_key = env("OPENALEX_API_KEY").map(SecretString::from);
    config.openalex = Some(openalex);
    config.crossref = Some(CrossrefConfig::default());
    standard(config).map_err(|e| Failure {
        exit: Exit::Internal,
        message: format!("could not set up HTTP client: {e}"),
    })
}

async fn run(engine: &Engine, command: Command, out: Out) -> Result<Exit, Failure> {
    match command {
        Command::Lookup { ids } => cmd::lookup::run(engine, ids, out).await,
        Command::Search { query, options } => cmd::search::run(engine, query, options, out).await,
        Command::Resolve { text, limit } => cmd::resolve::run(engine, text, limit, out).await,
    }
}
