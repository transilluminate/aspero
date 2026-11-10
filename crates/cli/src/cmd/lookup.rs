//! `aspero lookup`: works by identifier, falling through providers.

use std::io::IsTerminal;

use aspero_core::{FieldSet, IdError, SourceRecord, WorkId};
use aspero_engine::{Abstract, Engine, LookupResult, Outcome, Served};
use serde_json::{Value, json};

use super::{Failure, Out, merge_usage, print, print_line, read_stdin};
use crate::args::Format;
use crate::exit::Exit;
use crate::output;

pub async fn run(engine: &Engine, ids: Vec<String>, out: Out) -> Result<Exit, Failure> {
    let inputs = inputs(ids)?;
    let parsed: Vec<Result<WorkId, IdError>> = inputs.iter().map(|s| s.parse()).collect();
    let valid: Vec<WorkId> = parsed
        .iter()
        .filter_map(|p| p.as_ref().ok().cloned())
        .collect();
    let mut report = engine.lookup(&valid, FieldSet::DEFAULT).await;

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
    for (input, parsed) in inputs.iter().zip(&parsed) {
        let (item, item_exit) = match parsed {
            Err(e) => (
                json!({"input": input, "status": "invalid", "error": e.to_string()}),
                Exit::Usage,
            ),
            Ok(_) => {
                let (result, found) = results.next().expect("one result per valid id");
                let (mut item, exit) = item(input, result, out.full);
                if let Some(found) = found {
                    output::set_abstract(&mut item["work"], found.as_ref());
                }
                (item, exit)
            }
        };
        exit = exit.worst(item_exit);
        items.push(item);
    }

    match out.format {
        Format::Jsonl => items.iter().for_each(print_line),
        Format::Table => {
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

/// Arguments, or one identifier per stdin line when given none (or `-`).
/// Blank lines and `#` comments are skipped.
fn inputs(ids: Vec<String>) -> Result<Vec<String>, Failure> {
    if !(ids.is_empty() || ids == ["-"]) {
        return Ok(ids);
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
        .map(str::to_owned)
        .collect())
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
