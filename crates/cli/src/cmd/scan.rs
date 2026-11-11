//! `aspero scan`: the identifiers mentioned in documents, where, and as
//! written. Offline: nothing is looked up.

use std::io::IsTerminal;
use std::path::PathBuf;

use aspero_core::id::scan;
use serde_json::json;

use super::{Failure, Out, print, print_line, read_stdin};
use crate::args::Format;
use crate::exit::Exit;
use crate::output;

pub fn run(files: Vec<PathBuf>, out: Out) -> Result<Exit, Failure> {
    let mut mentions = Vec::new();
    for (name, text) in documents(files)? {
        for m in scan(&text) {
            mentions.push(aspero_json::mention(&name, &text, &m));
        }
    }
    match out.format {
        Format::Jsonl => mentions.iter().for_each(print_line),
        Format::Table => {
            let mut table = output::table(&["File", "Line", "Identifier", "Context"]);
            for m in &mentions {
                table.add_row(vec![
                    m["file"].as_str().unwrap_or("").to_owned(),
                    format!("{}:{}", m["line"], m["column"]),
                    m["id"].as_str().unwrap_or("").to_owned(),
                    m["context"].as_str().unwrap_or("").to_owned(),
                ]);
            }
            println!("{table}");
        }
        Format::Text => {
            for m in &mentions {
                println!(
                    "{}:{}:{}  {}  {}",
                    m["file"].as_str().unwrap_or(""),
                    m["line"],
                    m["column"],
                    m["id"].as_str().unwrap_or(""),
                    m["context"].as_str().unwrap_or("")
                );
            }
        }
        _ => print(&json!({ "mentions": mentions })),
    }
    Ok(Exit::Ok)
}

/// `(name, text)` per input: each file, or stdin as `-`.
fn documents(files: Vec<PathBuf>) -> Result<Vec<(String, String)>, Failure> {
    if files.is_empty() || files.iter().any(|f| f.as_os_str() == "-") {
        if files.len() > 1 {
            return Err(Failure::usage("`-` (stdin) can't be mixed with files"));
        }
        if std::io::stdin().is_terminal() {
            return Err(Failure::usage("no files given (pass them, or pipe text)"));
        }
        return Ok(vec![("-".to_owned(), read_stdin()?)]);
    }
    files
        .into_iter()
        .map(|path| {
            let bytes = std::fs::read(&path)
                .map_err(|e| Failure::usage(format!("could not read {}: {e}", path.display())))?;
            let text = String::from_utf8_lossy(&bytes).into_owned();
            Ok((path.display().to_string(), text))
        })
        .collect()
}
