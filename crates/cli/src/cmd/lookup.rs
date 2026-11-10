//! `aspero lookup`: works by identifier, falling through providers.

use std::io::IsTerminal;

use aspero_core::{Doi, FieldSet, IdError, SourceRecord, WorkId};
use aspero_engine::{
    Abstract, Engine, LookupReport, LookupResult, Outcome, Served, describe_update,
};
use serde_json::{Value, json};

use super::{Failure, Out, merge_usage, print, print_line, read_stdin};
use crate::args::Format;
use crate::exit::Exit;
use crate::output::{self, Level};

pub async fn run(engine: &Engine, ids: Vec<String>, out: Out) -> Result<Exit, Failure> {
    let (inputs, from) = inputs(ids)?;
    let parsed: Vec<Result<WorkId, IdError>> = inputs.iter().map(|s| s.parse()).collect();
    let valid: Vec<WorkId> = parsed
        .iter()
        .filter_map(|p| p.as_ref().ok().cloned())
        .collect();
    let mut report = engine.lookup(&valid, FieldSet::DEFAULT).await;
    let retried = retry_with_punctuation(engine, &inputs, &parsed, &mut report).await;
    if out.full {
        engine.add_notices(&mut report).await;
        engine.add_open_access(&mut report).await;
    }

    // Abstracts for the works found, one batched call; None per result
    // when not asked for or not found.
    let mut abstracts: Vec<Option<Option<Abstract>>> = vec![None; report.results.len()];
    if out.abstracts {
        let found: Vec<(usize, &SourceRecord)> = report
            .results
            .iter()
            .enumerate()
            .filter_map(|(i, r)| Some((i, r.record.as_ref()?)))
            .collect();
        let records: Vec<&SourceRecord> = found.iter().map(|(_, r)| *r).collect();
        let fetched = engine.abstracts(&records).await;
        for ((i, _), a) in found.iter().zip(fetched.found) {
            abstracts[*i] = Some(a);
        }
        for (p, u) in fetched.usage {
            merge_usage(&mut report.usage, &p, u);
        }
    }
    let mut results = report.results.iter().zip(&abstracts);

    // Every input gets an item, in input order, including ones that didn't
    // parse: a request is never silently dropped.
    let mut exit = Exit::Ok;
    let mut items = Vec::new();
    for ((input, parsed), from) in inputs.iter().zip(&parsed).zip(from) {
        let (mut item, item_exit) = match parsed {
            Err(e) => (
                json!({"input": input, "status": "invalid", "error": e.to_string()}),
                Exit::Usage,
            ),
            Ok(_) => {
                let (result, found) = results.next().expect("one result per valid id");
                let (mut item, exit) = item(input, result, out.full);
                if retried.contains(&result.id) {
                    item["note"] = json!("found with the trailing punctuation kept");
                }
                if let Some(found) = found {
                    output::set_abstract(&mut item["work"], found.as_ref());
                }
                (item, exit)
            }
        };
        if let Some(from) = from {
            item["from"] = from;
        }
        exit = exit.worst(item_exit);
        items.push(item);
    }

    match out.format {
        Format::Jsonl => items.iter().for_each(print_line),
        Format::Table => {
            let mut table =
                output::table(&["ID", "Status", "Year", "Author", "Title", "Free", "Via"]);
            let mut listed = Vec::new();
            let mut abstracts = abstracts.iter();
            for (item, result) in items.iter().zip(per_input(&parsed, &report.results)) {
                table.add_row(row(item, result));
                let found = result
                    .and_then(|_| abstracts.next())
                    .and_then(Option::as_ref);
                if let (Some(found), Some(id)) = (found, item["id"].as_str()) {
                    listed.push((id.to_owned(), found.clone()));
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
            let mut abstracts = abstracts.iter();
            for (item, result) in items.iter().zip(per_input(&parsed, &report.results)) {
                print!("{}", text(item, result));
                let found = result
                    .and_then(|_| abstracts.next())
                    .and_then(Option::as_ref);
                if let Some(found) = found {
                    println!("{}", output::abstract_text(found.as_ref(), "  "));
                }
                println!();
            }
            if let Some(usage) = output::usage_text(&report.usage) {
                eprintln!("{usage}");
            }
        }
        Format::Json | Format::Auto => {
            print(&json!({"results": items, "usage": output::usage(&report.usage)}));
        }
    }
    Ok(exit)
}

/// DOIs may legally end in `.` (or `,;:`), which parsing strips as sentence
/// punctuation. For DOIs not found that way, ask once more with the
/// punctuation kept, in one batch; found ones replace their results.
/// Returns the identifiers that were found only on retry.
async fn retry_with_punctuation(
    engine: &Engine,
    inputs: &[String],
    parsed: &[Result<WorkId, IdError>],
    report: &mut LookupReport,
) -> Vec<WorkId> {
    let valid_inputs = inputs
        .iter()
        .zip(parsed)
        .filter(|(_, p)| p.is_ok())
        .map(|(i, _)| i);
    let retries: Vec<(usize, WorkId)> = valid_inputs
        .zip(&report.results)
        .enumerate()
        .filter(|(_, (_, r))| r.is_not_found())
        .filter_map(|(i, (input, _))| {
            Some((i, WorkId::Doi(Doi::with_trailing_punctuation(input)?)))
        })
        .collect();
    if retries.is_empty() {
        return Vec::new();
    }
    let ids: Vec<WorkId> = retries.iter().map(|(_, id)| id.clone()).collect();
    let second = engine.lookup(&ids, FieldSet::DEFAULT).await;
    for (p, u) in second.usage {
        merge_usage(&mut report.usage, &p, u);
    }
    let mut found = Vec::new();
    for ((i, _), result) in retries.into_iter().zip(second.results) {
        if result.record.is_some() {
            found.push(result.id.clone());
            report.results[i] = result;
        }
    }
    found
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
    Ok(read_stdin()?
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|line| match serde_json::from_str::<Value>(line) {
            Ok(object @ Value::Object(_)) => {
                let id = object["id"].as_str().unwrap_or(line).to_owned();
                (id, Some(object))
            }
            _ => (line.to_owned(), None),
        })
        .unzip())
}

/// The engine result for each input, `None` for inputs that didn't parse.
fn per_input<'a>(
    parsed: &'a [Result<WorkId, IdError>],
    results: &'a [LookupResult],
) -> impl Iterator<Item = Option<&'a LookupResult>> {
    let mut results = results.iter();
    parsed
        .iter()
        .map(move |p| p.as_ref().ok().and_then(|_| results.next()))
}

/// One result as JSON, with the exit code it implies. Attempts are included
/// when nothing was found (so the caller sees why) or with `--full`.
pub fn item(input: &str, result: &LookupResult, full: bool) -> (Value, Exit) {
    let mut item = json!({"input": input, "id": result.id});
    let exit = if let Some(record) = &result.record {
        item["status"] = json!("found");
        // Say when the answer didn't come live from the provider.
        let served = result
            .attempts
            .iter()
            .rfind(|a| matches!(a.outcome, Outcome::Found))
            .map(|a| a.served);
        if let Some(s) = served.filter(|s| *s != Served::Live) {
            item["served"] = json!(s.as_str());
        }
        item["work"] = output::work(record, full);
        Exit::Ok
    } else if result.attempts.is_empty() {
        item["status"] = json!("unsupported");
        item["error"] = json!(format!(
            "no enabled provider looks up {} identifiers",
            result.id.kind()
        ));
        Exit::Unsupported
    } else {
        let failures: Vec<_> = result
            .attempts
            .iter()
            .filter_map(|a| match &a.outcome {
                Outcome::Failed(e) => Some(e),
                _ => None,
            })
            .collect();
        let all_failed = failures.len() == result.attempts.len();
        item["status"] = json!(if all_failed { "failed" } else { "not_found" });
        if all_failed {
            Exit::from_category(failures[0].category())
        } else {
            Exit::NotFound
        }
    };
    if result.record.is_none() || full {
        let attempts: Vec<Value> = result
            .attempts
            .iter()
            .map(|a| match &a.outcome {
                Outcome::Found => json!({"provider": a.provider, "outcome": "found"}),
                Outcome::NotFound => json!({"provider": a.provider, "outcome": "not_found"}),
                Outcome::Failed(e) => {
                    json!({"provider": a.provider, "outcome": "failed", "error": e.to_string()})
                }
                _ => json!({"provider": a.provider, "outcome": "unknown"}),
            })
            .collect();
        item["attempts"] = json!(attempts);
    }
    (item, exit)
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
            let mut status_text = status;
            if let Some(r) = w.retraction() {
                status_text = format!("found\n{}", describe_update(r));
            }
            let level = if w.retraction().is_some() {
                Level::Error
            } else {
                Level::Good
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
        return format!(
            "{label}  found via {}{served}\n{}\n",
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
