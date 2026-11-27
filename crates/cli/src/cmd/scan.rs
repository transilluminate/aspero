//! `aspero scan`: the identifiers mentioned in documents, where, and as
//! written. Offline: nothing is looked up.

use std::io::IsTerminal;
use std::path::PathBuf;

use aspero_core::id::scan;
use serde_json::json;

use super::{Failure, Out, print, print_line, read_stdin};
use crate::args::Format;
use crate::exit::Exit;
use crate::grid::{Col, Grid};
use crate::term;

// Like every command: main hands over what it parsed.
#[allow(clippy::needless_pass_by_value)]
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
            // One file's name, given on the command line, needn't fill a
            // column.
            let files = mentions
                .iter()
                .map(|m| m["file"].as_str())
                .collect::<std::collections::BTreeSet<_>>()
                .len();
            let mut cols = vec![
                Col::cap("File", 30),
                Col::content("Line"),
                Col::content("Identifier"),
                Col::fill("Context"),
            ];
            if files < 2 {
                cols.remove(0);
            }
            let mut grid = Grid::new(cols);
            for m in &mentions {
                let mut row = vec![
                    m["file"].as_str().unwrap_or("").to_owned(),
                    format!("{}:{}", m["line"], m["column"]),
                    m["id"].as_str().unwrap_or("").to_owned(),
                    m["context"].as_str().unwrap_or("").to_owned(),
                ];
                if files < 2 {
                    row.remove(0);
                }
                grid.row(row);
            }
            if mentions.is_empty() {
                println!("no identifiers found");
            } else {
                println!("{grid}");
            }
        }
        Format::Text => {
            for m in &mentions {
                let line = format!(
                    "{}:{}:{}  {}  {}",
                    m["file"].as_str().unwrap_or(""),
                    m["line"],
                    m["column"],
                    m["id"].as_str().unwrap_or(""),
                    m["context"].as_str().unwrap_or("")
                );
                println!("{}", term::fill(&line));
            }
        }
        _ => print(&document(&mentions)),
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

/// The JSON document `scan --format json` prints.
fn document(mentions: &[serde_json::Value]) -> serde_json::Value {
    json!({ "mentions": mentions })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::testing::pinned;

    /// The mentions in a text, as `scan --format json` prints them: a DOI,
    /// one from a web address, one broken at a hyphen, an arXiv id and a
    /// PMID.
    #[test]
    fn json_is_pinned() {
        let text = "As shown (doi:10.1038/nature14539), and at\n\
                    https://doi.org/10.1145/3448250 and in 10.1093/nar/gkab-\n\
                    1112 too. See arXiv:1706.03762 and PMID: 12345678.\n";
        let mentions: Vec<_> = scan(text)
            .iter()
            .map(|m| aspero_json::mention("paper.txt", text, m))
            .collect();
        let doc = document(&mentions);
        pinned(
            "scan/expected.json",
            &(serde_json::to_string_pretty(&doc).unwrap() + "\n"),
        );
    }
}
