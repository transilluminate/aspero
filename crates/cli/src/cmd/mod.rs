//! One module per subcommand, plus the plumbing they share.
//!
//! A command returns `Ok(exit)` when it ran (even if some items were not
//! found: those outcomes are in its output) and `Err(Failure)` when it could
//! not run at all.

pub mod cache;
pub mod check;
pub mod claim;
pub mod identify;
pub mod lookup;
pub mod resolve;
pub mod scan;
pub mod search;
pub mod styles;
#[cfg(test)]
mod testing;
pub mod verify;

use std::io::Read;

use aspero_engine::EngineError;
pub use aspero_json::merged;
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
#[derive(Debug, Clone)]
pub struct Out {
    pub format: Format,
    pub full: bool,
    pub abstracts: bool,
    /// For `--format cite`: a bundled style's id, or a `.csl` file.
    pub style: Option<String>,
    pub locale: Option<String>,
}

pub fn read_stdin() -> Result<String, Failure> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| Failure::usage(format!("could not read stdin: {e}")))?;
    Ok(buf)
}

/// The lines of `text` worth reading, trimmed: blank lines and `#`
/// comments skipped.
pub fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
}

/// Quotes `s` for a POSIX shell, leaving plain words as they are.
pub fn shell_quote(s: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "-_./:=+,@%".contains(c);
    if !s.is_empty() && s.chars().all(plain) {
        s.to_owned()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
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
