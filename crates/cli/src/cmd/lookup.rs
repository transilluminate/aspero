//! `aspero lookup`: works by identifier, falling through providers.

use std::io::IsTerminal;

use aspero_engine::{Engine, Extras, InputLookup, Looked, LookupResult, describe_update};
use aspero_json::Status;
use serde_json::Value;

use super::{Failure, Out, lines, print, print_line, read_stdin};
use crate::args::Format;
use crate::exit::Exit;
use crate::output::{self, Level};

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
            let mut table =
                output::table(&["ID", "Status", "Year", "Author", "Title", "Free", "Via"]);
            let mut listed = Vec::new();
            for (item, lookup) in items.iter().zip(&report.items) {
                let looked = looked(lookup);
                table.add_row(row(item, looked.map(|l| &l.result)));
                let found = looked.and_then(|l| l.abstract_text.as_ref());
                if let (Some(found), Some(id)) = (found, item["id"].as_str()) {
                    listed.push((id.to_owned(), found));
                }
            }
            println!("{table}");
            for (id, found) in listed {
                println!("\n{id}\n{}", output::abstract_text(found.as_ref(), "  "));
            }
            if let Some(usage) = output::usage_text(&report.usage) {
                eprintln!("{usage}");
            }
        }
        Format::Text => {
            for (item, lookup) in items.iter().zip(&report.items) {
                let looked = looked(lookup);
                print!("{}", text(item, looked.map(|l| &l.result)));
                if let Some(found) = looked.and_then(|l| l.abstract_text.as_ref()) {
                    println!("{}", output::abstract_text(found.as_ref(), "  "));
                }
                println!();
            }
            if let Some(usage) = output::usage_text(&report.usage) {
                eprintln!("{usage}");
            }
        }
        Format::Bibtex | Format::Ris | Format::Cite => {
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
        _ => None,
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

/// One result as a table row.
fn row(item: &Value, result: Option<&LookupResult>) -> Vec<comfy_table::Cell> {
    use comfy_table::Cell;
    let label = item["id"]
        .as_str()
        .or(item["input"].as_str())
        .unwrap_or("?");
    let status = item["status"].as_str().unwrap_or("?").replace('_', " ");
    match result.and_then(|r| r.record.as_ref()) {
        Some(record) => {
            let w = &record.work;
            let via = match item["served"].as_str() {
                Some(served) => format!("{} ({served})", record.source),
                None => record.source.to_string(),
            };
            let (status_text, level) = match (w.retraction(), w.newer_version()) {
                (Some(r), _) => (format!("found\n{}", describe_update(r)), Level::Error),
                (None, Some(newer)) => (format!("found\nsuperseded by {newer}"), Level::Warning),
                (None, None) => (status, Level::Good),
            };
            vec![
                Cell::new(label),
                output::status_cell(&status_text, level),
                Cell::new(output::year(w)),
                Cell::new(output::first_author(w)),
                Cell::new(w.title.as_deref().unwrap_or("")),
                Cell::new(output::free_copy(w)),
                Cell::new(via),
            ]
        }
        None => {
            let level = match item["status"].as_str() {
                Some("not_found") | Some("invalid") => Level::Error,
                _ => Level::Warning,
            };
            let why = item["error"].as_str().unwrap_or("");
            vec![
                Cell::new(label),
                output::status_cell(&status, level),
                Cell::new(""),
                Cell::new(""),
                Cell::new(why),
                Cell::new(""),
                Cell::new(""),
            ]
        }
    }
}

/// One result as terminal text.
pub fn text(item: &Value, result: Option<&LookupResult>) -> String {
    let label = item["id"]
        .as_str()
        .or(item["input"].as_str())
        .unwrap_or("?");
    if let Some(record) = result.and_then(|r| r.record.as_ref()) {
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
        let conflicts: Vec<String> = item["disagreements"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|d| {
                let registry = d
                    .as_object()
                    .and_then(|o| o.keys().find(|k| !matches!(k.as_str(), "field" | "value")));
                let theirs = registry
                    .map(|k| d[k].as_str().unwrap_or("?"))
                    .unwrap_or("?");
                format!(
                    "{} {} (vs {theirs})",
                    d["field"].as_str().unwrap_or("?"),
                    d["value"].as_str().unwrap_or("?")
                )
            })
            .collect();
        let conflicts = match conflicts.is_empty() {
            true => String::new(),
            false => format!(
                "\n  ⚠ the DOI's registry disagrees: {}",
                conflicts.join("; ")
            ),
        };
        return format!(
            "{label}  found via {}{served}\n{}{latest}{conflicts}\n",
            record.source,
            output::work_text(record, "  ")
        );
    }
    let why = item["error"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| {
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
        });
    let status = item["status"].as_str().unwrap_or("?").replace('_', " ");
    format!("{label}  {status} ({why})\n")
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
                text(&item, looked(lookup).map(|l| &l.result)) + "\n"
            })
            .collect();
        pinned("lookup/expected.txt", &text);
    }
}
