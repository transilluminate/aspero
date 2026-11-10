//! `aspero scan`: the identifiers mentioned in documents, where, and as
//! written. Offline: nothing is looked up.

use std::io::IsTerminal;
use std::path::PathBuf;

use aspero_core::id::{Mention, scan};
use serde_json::{Value, json};

use super::{Failure, Out, print, print_line, read_stdin};
use crate::args::Format;
use crate::exit::Exit;
use crate::output;

/// Characters of context shown either side of a mention in a long line.
const CONTEXT: usize = 60;

pub fn run(files: Vec<PathBuf>, out: Out) -> Result<Exit, Failure> {
    let mut mentions = Vec::new();
    for (name, text) in documents(files)? {
        for m in scan(&text) {
            mentions.push(item(&name, &text, &m));
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

fn item(file: &str, text: &str, m: &Mention) -> Value {
    json!({
        "file": file,
        "line": m.line,
        "column": m.column,
        "id": m.id,
        "kind": m.id.kind().prefix(),
        "text": &text[m.span.clone()],
        "context": context(text, m),
    })
}

/// The mention's line, trimmed, cut to a window around it when long.
fn context(text: &str, m: &Mention) -> String {
    let line_start = text[..m.span.start].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[m.span.end..]
        .find('\n')
        .map_or(text.len(), |i| m.span.end + i);
    let before: Vec<char> = text[line_start..m.span.start].chars().collect();
    let after: Vec<char> = text[m.span.end..line_end].chars().collect();
    let cut_before = before.len() > CONTEXT;
    let cut_after = after.len() > CONTEXT;
    let before: String = before[before.len().saturating_sub(CONTEXT)..]
        .iter()
        .collect();
    let after: String = after[..after.len().min(CONTEXT)].iter().collect();
    format!(
        "{}{}{}{}{}",
        if cut_before { "…" } else { "" },
        before.trim_start(),
        &text[m.span.clone()],
        after.trim_end(),
        if cut_after { "…" } else { "" },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mentions_carry_where_and_how_they_were_written() {
        let text = "# Notes\n  PE trials: PEITHO (doi:10.1056/NEJMoa1302097).\r\n";
        let m = &scan(text)[0];
        assert_eq!(
            item("vault/pe.md", text, m),
            json!({
                "file": "vault/pe.md",
                "line": 2,
                "column": 22,
                "id": "doi:10.1056/nejmoa1302097",
                "kind": "doi",
                "text": "doi:10.1056/NEJMoa1302097",
                "context": "PE trials: PEITHO (doi:10.1056/NEJMoa1302097).",
            })
        );
    }

    #[test]
    fn long_lines_are_cut_around_the_mention() {
        let text = format!("{} PMC6716493 {}", "a".repeat(100), "b".repeat(100));
        let m = &scan(&text)[0];
        let c = context(&text, m);
        assert!(c.starts_with('…') && c.ends_with('…'), "{c}");
        assert!(c.contains(" PMC6716493 "));
        assert_eq!(c.chars().count(), 2 * CONTEXT + "PMC6716493".len() + 2);
    }
}
