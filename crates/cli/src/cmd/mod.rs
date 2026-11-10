//! One module per subcommand, plus the plumbing they share.
//!
//! A command returns `Ok(exit)` when it ran (even if some items were not
//! found: those outcomes are in its output) and `Err(Failure)` when it could
//! not run at all.

pub mod claim;
pub mod lookup;
pub mod resolve;
pub mod search;
pub mod verify;

use std::io::Read;

use aspero_engine::EngineError;
use serde_json::Value;

use crate::args::Format;
use crate::exit::Exit;

/// A command that couldn't run at all (as opposed to per-item outcomes).
#[derive(Debug)]
pub struct Failure {
    pub exit: Exit,
    pub message: String,
}

impl Failure {
    pub fn usage(message: impl Into<String>) -> Self {
        Failure {
            exit: Exit::Usage,
            message: message.into(),
        }
    }
}

impl From<EngineError> for Failure {
    fn from(e: EngineError) -> Self {
        Failure {
            exit: Exit::from_category(e.category()),
            message: e.to_string(),
        }
    }
}

/// Resolved output settings. `format` is never `Auto` here.
#[derive(Debug, Clone, Copy)]
pub struct Out {
    pub format: Format,
    pub full: bool,
}

pub fn read_stdin() -> Result<String, Failure> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| Failure::usage(format!("could not read stdin: {e}")))?;
    Ok(buf)
}

/// A whole JSON document, pretty-printed.
pub fn print(value: &Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("JSON values serialise")
    );
}

/// One JSON Lines record.
pub fn print_line(value: &Value) {
    println!("{value}");
}
