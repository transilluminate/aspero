//! `aspero identify`: which work a document is, from its first page.

use std::io::IsTerminal;
use std::path::PathBuf;

use aspero_engine::Engine;
use serde_json::json;

use super::verify::{exit_for, verdict_text};
use super::{Failure, Out, print, read_stdin};
use crate::args::Format;
use crate::exit::Exit;

pub async fn run(
    engine: &Engine,
    file: Option<PathBuf>,
    title: Option<String>,
    title_hint: Option<String>,
    out: Out,
) -> Result<Exit, Failure> {
    let page = match file.filter(|f| f.as_os_str() != "-") {
        Some(path) => {
            let bytes = std::fs::read(&path)
                .map_err(|e| Failure::usage(format!("could not read {}: {e}", path.display())))?;
            String::from_utf8_lossy(&bytes).into_owned()
        }
        None if std::io::stdin().is_terminal() => {
            return Err(Failure::usage(
                "no file given (pass one, or pipe the page's text)",
            ));
        }
        None => read_stdin()?,
    };
    let Some(found) = engine
        .identify(&page, title.as_deref(), title_hint.as_deref())
        .await
    else {
        match out.format {
            Format::Text | Format::Table => {
                println!("NOT IDENTIFIED\n  nothing on the page is sure enough")
            }
            _ => print(&json!({"identified": false})),
        }
        return Ok(Exit::NotFound);
    };
    let v = &found.verification;
    let input = found.input();
    match out.format {
        Format::Text | Format::Table => {
            println!(
                "IDENTIFIED by {}",
                found.decided_by.as_str().replace('_', " ")
            );
            print!("{}", verdict_text(&input, v));
        }
        _ => {
            let mut item = aspero_json::verification(json!({"input": input}), v, out.full);
            item["identified"] = json!(true);
            item["decided_by"] = json!(found.decided_by.as_str());
            print(&item);
        }
    }
    Ok(exit_for(v.verdict))
}
