//! `aspero identify`: which work a document is, from its first page.

use std::io::IsTerminal;
use std::path::PathBuf;

use aspero_engine::{Engine, Identified};
use serde_json::json;

use super::verify::{exit_for, verdict_text};
use super::{Failure, Out, print, read_stdin};
use crate::args::Format;
use crate::exit::Exit;
use crate::term::{self, Level, Paint};

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
        .identified
    else {
        match out.format {
            Format::Text | Format::Table => {
                let verdict = term::paint("NOT IDENTIFIED", Paint::Strong(Level::Warning));
                println!("{verdict}\n  nothing on the page is sure enough");
            }
            _ => print(&json!({"identified": false})),
        }
        return Ok(Exit::NotFound);
    };
    let v = &found.verification;
    let input = found.input();
    match out.format {
        Format::Text | Format::Table => {
            let verdict = term::paint("IDENTIFIED", Paint::Strong(Level::Good));
            let by = found.decided_by.as_str().replace('_', " ");
            println!("{verdict} by {by}");
            print!("{}", term::fill(&verdict_text(&input, v)));
        }
        _ => print(&item(&found, out.full)),
    }
    Ok(exit_for(v.verdict))
}

/// The JSON of a document identified: its verification, `identified`
/// and `decided_by`.
fn item(found: &Identified, full: bool) -> serde_json::Value {
    let input = found.input();
    let mut item = aspero_json::verification(json!({"input": input}), &found.verification, full);
    item["identified"] = json!(true);
    item["decided_by"] = json!(found.decided_by.as_str());
    item
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::testing::{Scripted, engine, pinned, work};

    /// What `identify --format json` prints for a first page whose DOI
    /// and title agree, lean and `--full`.
    #[tokio::test]
    async fn json_is_pinned() {
        let lecun = ["Yann LeCun", "Yoshua Bengio", "Geoffrey Hinton"];
        let engine = engine(vec![Scripted::new(
            "openalex",
            vec![work(
                &["10.1038/nature14539"],
                "Deep learning",
                &lecun,
                2015,
            )],
        )]);
        let page = "Deep learning\nYann LeCun, Yoshua Bengio & Geoffrey Hinton\n\
                    Nature 521, 436-444 (2015). doi:10.1038/nature14539\n";
        let found = engine
            .identify(page, None, Some("Deep learning"))
            .await
            .identified
            .expect("identified");
        let lines = format!("{}\n{}\n", item(&found, false), item(&found, true));
        pinned("identify/expected.jsonl", &lines);
    }
}
