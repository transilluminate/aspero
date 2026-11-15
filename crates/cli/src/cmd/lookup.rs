//! `aspero lookup`: works by identifier, falling through providers.

use std::io::IsTerminal;

use aspero_engine::{Engine, Extras, InputLookup, Looked, LookupResult, describe_update};
use aspero_json::Status;
use serde_json::Value;

use super::{Failure, Out, lines, print, print_line, read_stdin};
use crate::args::Format;
use crate::exit::Exit;
use crate::grid::{self, Col, Grid, Text};
use crate::output::{self, Level};
use crate::term::{self, Paint};

pub async fn run(engine: &Engine, ids: Vec<String>, out: Out) -> Result<Exit, Failure> {
    let (inputs, from) = inputs(ids)?;
    let mut extras = Extras::default();
    extras.full = out.full;
    extras.abstracts = out.abstracts;
    let report = engine.lookup_inputs(&inputs, extras).await;

    // Every input gets an item, in input order, including ones that didn't
    // parse: a request is never silently dropped.
    let mut exit = Exit::Ok;
    let mut items = Vec::new();
    for ((input, lookup), from) in inputs.iter().zip(&report.items).zip(from) {
        let mut item = aspero_json::input_lookup(input, lookup, out.full);
        exit = exit.worst(match looked(lookup) {
            None => Exit::Usage,
            Some(l) => exit_for(&l.result),
        });
        if let Some(from) = from {
            item["from"] = from;
        }
        items.push(item);
    }

    match out.format {
        Format::Jsonl => items.iter().for_each(print_line),
        Format::Table => {
            // Too narrow for them all, the source goes first, then the
            // identifier moves under the title, then the free copy and
            // the author go.
            let mut grid = Grid::new([
                Col::content("ID").folds(2, "Title"),
                Col::content("Status"),
                Col::cap("Author", 20).drops(4),
                Col::content("Year"),
                Col::fill("Title").or_when_folded("Work"),
                Col::content("Free copy").drops(3),
                Col::content("Via").drops(1),
            ]);
            let mut listed = Vec::new();
            let mut notes = Vec::new();
            for (item, lookup) in items.iter().zip(&report.items) {
                let looked = looked(lookup);
                grid.row(row(item, looked.map(|l| &l.result)));
                let found = looked.and_then(|l| l.abstract_text.as_ref());
                if let (Some(found), Some(id)) = (found, item["id"].as_str()) {
                    listed.push((id.to_owned(), found));
                }
                if let Some(note) = note(item) {
                    notes.push(format!("{}: {note}", label(item)));
                }
            }
            println!("{grid}");
            // Why an input wasn't looked up, below the table, not squeezed
            // into a column.
            for note in notes {
                println!("{}", term::fill(&note));
            }
            for (id, found) in listed {
                let text = output::abstract_text(found.as_ref(), "  ");
                println!("\n{id}\n{}", term::fill(&text));
            }
            output::eprint_usage(&report.usage);
        }
        Format::Text => {
            for (item, lookup) in items.iter().zip(&report.items) {
                let looked = looked(lookup);
                print!("{}", term::fill(&text(item, looked)));
                if let Some(found) = looked.and_then(|l| l.abstract_text.as_ref()) {
                    println!(
                        "{}",
                        term::fill(&output::abstract_text(found.as_ref(), "  "))
                    );
                }
                println!();
            }
            output::eprint_usage(&report.usage);
        }
        Format::Bibtex | Format::Ris | Format::CslJson | Format::Cite => {
            let mut export = output::Exporter::new(&out)?;
            for (input, lookup) in inputs.iter().zip(&report.items) {
                match looked(lookup).and_then(|l| l.result.record.as_ref()) {
                    Some(r) => {
                        let entry = export.entry(&r.work, &r.ids);
                        if !entry.is_empty() {
                            println!("{entry}");
                        }
                    }
                    None => eprintln!("skipped {input}: not found"),
                }
            }
            print!("{}", export.finish());
        }
        Format::Json | Format::Auto => {
            print(&serde_json::json!({"results": items, "usage": output::usage(&report.usage)}));
        }
    }
    Ok(exit)
}

fn looked(lookup: &InputLookup) -> Option<&Looked> {
    match lookup {
        InputLookup::Looked(l) => Some(l),
        InputLookup::Invalid(_) => None,
    }
}

/// Arguments, or one identifier per stdin line when given none (or `-`).
/// Blank lines and `#` comments are skipped. A JSON object line (as from
/// `aspero scan`) gives its `id`, and is echoed back as the result's
/// `from`. Returns the inputs and, per input, that object.
fn inputs(ids: Vec<String>) -> Result<(Vec<String>, Vec<Option<Value>>), Failure> {
    if !(ids.is_empty() || ids == ["-"]) {
        let from = vec![None; ids.len()];
        return Ok((ids, from));
    }
    if std::io::stdin().is_terminal() {
        return Err(Failure::usage(
            "no identifiers given (pass them, or pipe one per line)",
        ));
    }
    Ok(lines(&read_stdin()?)
        .map(|line| match serde_json::from_str::<Value>(line) {
            Ok(object @ Value::Object(_)) => {
                let id = object["id"].as_str().unwrap_or(line).to_owned();
                (id, Some(object))
            }
            _ => (line.to_owned(), None),
        })
        .unzip())
}

/// The exit code a result implies.
fn exit_for(result: &LookupResult) -> Exit {
    match Status::of(result) {
        Status::Found => Exit::Ok,
        Status::Unsupported => Exit::Unsupported,
        Status::Failed(category) => Exit::from_category(category),
        Status::NotCached => Exit::Unavailable,
        _ => Exit::NotFound,
    }
}

/// The identifier looked up, or the input that isn't one.
fn label(item: &Value) -> &str {
    item["id"]
        .as_str()
        .or(item["input"].as_str())
        .unwrap_or("?")
}

/// "found", "not found", …, and how much attention it needs.
fn status(item: &Value) -> (String, Level) {
    let level = match item["status"].as_str() {
        Some("found") => Level::Good,
        Some("not_found" | "invalid") => Level::Error,
        _ => Level::Warning,
    };
    let status = item["status"].as_str().unwrap_or("?").replace('_', " ");
    (status, level)
}

/// Why an input wasn't found, when that's more than "not found": invalid,
/// unsupported, or the providers failed.
fn note(item: &Value) -> Option<String> {
    match item["status"].as_str() {
        Some("found" | "not_found") => None,
        _ => Some(why(item)),
    }
}

/// One result as a table row.
fn row(item: &Value, result: Option<&LookupResult>) -> Vec<Text> {
    let label = label(item);
    let (status, level) = status(item);
    let Some(record) = result.and_then(|r| r.record.as_ref()) else {
        return vec![Text::from(label), grid::status(status, level)];
    };
    let w = &record.work;
    let via = match item["served"].as_str() {
        Some(served) => format!("{} ({served})", record.source),
        None => record.source.to_string(),
    };
    let mut title = w.title.clone().unwrap_or_default();
    let status = match (w.retraction(), w.newer_version()) {
        (Some(r), _) => {
            title = format!("{title}\n⚠ {}", describe_update(r));
            grid::status("retracted", Level::Error)
        }
        (None, Some(newer)) => {
            title = format!("{title}\n⚠ superseded by {newer}");
            grid::status("superseded", Level::Warning)
        }
        (None, None) => grid::status(status, level),
    };
    vec![
        Text::from(label),
        status,
        Text::from(output::author_cell(w)),
        Text::from(output::year(w)),
        Text::from(title),
        Text::from(output::free_copy(w)),
        Text::from(via),
    ]
}

/// What each provider said, or the input's error.
fn why(item: &Value) -> String {
    if let Some(error) = item["error"].as_str() {
        return error.to_owned();
    }
    item["attempts"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|a| {
            let detail = a["error"]
                .as_str()
                .or(a["outcome"].as_str())
                .unwrap_or("?")
                .replace('_', " ");
            format!("{}: {detail}", a["provider"].as_str().unwrap_or("?"))
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// One result as terminal text.
pub fn text(item: &Value, looked: Option<&Looked>) -> String {
    let label = label(item);
    let (status, level) = status(item);
    let status = term::paint(&status, Paint::Strong(level));
    if let Some(looked) = looked
        && let Some(record) = &looked.result.record
    {
        let served = item["served"]
            .as_str()
            .map(|s| format!(" ({s})"))
            .unwrap_or_default();
        let latest = match &item["latest"] {
            Value::Null => String::new(),
            l => format!(
                "\n  latest version: {} \"{}\" ({})",
                l["ids"][0].as_str().unwrap_or("?"),
                l["title"].as_str().unwrap_or("(untitled)"),
                l["issued"].as_str().unwrap_or("?"),
            ),
        };
        let conflicts: Vec<String> = looked
            .disagreements
            .iter()
            .map(|d| format!("{} {} (vs {})", d.field, d.value, d.registry_value))
            .collect();
        let conflicts = if conflicts.is_empty() {
            String::new()
        } else {
            format!(
                "\n  ⚠ the DOI's registry disagrees: {}",
                conflicts.join("; ")
            )
        };
        return format!(
            "{label}  {status} via {}{served}\n{}{latest}{conflicts}\n",
            record.source,
            output::work_text(record, "  ")
        );
    }
    format!("{label}  {status} ({})\n", why(item))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::testing::{Scripted, engine, pinned, work};

    /// What `lookup --full` prints as text, byte for byte: found (with the
    /// registry disagreeing), not found, unsupported and invalid.
    #[tokio::test]
    async fn text_is_pinned() {
        let lecun = ["Yann LeCun", "Yoshua Bengio", "Geoffrey Hinton"];
        let mut late = work(&["10.1038/nature14539"], "Deep learning", &lecun, 2016);
        late.work.volume = Some("521".into());
        let mut registered = work(&["10.1038/nature14539"], "Deep learning", &lecun, 2015);
        registered.work.volume = Some("522".into());
        let engine = engine(vec![
            Scripted::new("openalex", vec![late]),
            Scripted::new("crossref", vec![registered]).registry(),
        ]);
        let inputs = [
            "10.1038/nature14539",
            "10.1234/missing",
            "PMID:12345678",
            "not an identifier",
        ]
        .map(String::from);
        let mut extras = Extras::default();
        extras.full = true;
        let report = engine.lookup_inputs(&inputs, extras).await;
        let text: String = inputs
            .iter()
            .zip(&report.items)
            .map(|(input, lookup)| {
                let item = aspero_json::input_lookup(input, lookup, true);
                text(&item, looked(lookup)) + "\n"
            })
            .collect();
        pinned("lookup/expected.txt", &text);
    }
}
