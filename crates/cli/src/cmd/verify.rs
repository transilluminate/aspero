//! `aspero verify`: does a citation refer to a real work, with matching
//! details?

use aspero_core::{ProviderId, Usage};
use aspero_engine::{Engine, Verdict, Verification};
use serde_json::{Value, json};

use super::claim::{ClaimInput, claims};
use std::io::IsTerminal;

use futures::StreamExt;

use super::{Failure, Out, merge_usage, print, print_line};
use crate::args::{ClaimArgs, Format};
use crate::exit::Exit;
use crate::output::{self, Level};

pub async fn run(engine: &Engine, args: ClaimArgs, out: Out) -> Result<Exit, Failure> {
    let limit = args.limit;
    let (text, fields) = split(args);
    let claims = claims(text, fields)?;
    if claims.is_empty() {
        return Err(Failure::usage("no claims given"));
    }

    let mut exit = Exit::Ok;
    let mut usage: Vec<(ProviderId, Usage)> = Vec::new();
    let mut items = Vec::new();

    // One batched lookup of every claimed identifier first; with the cache
    // warm, claims are then verified several at a time, streamed in order.
    let valid: Vec<_> = claims
        .iter()
        .filter_map(|(_, c)| c.as_ref().ok().cloned())
        .collect();
    if valid.len() > 1 {
        for (p, u) in engine.prefetch(&valid).await.usage {
            merge_usage(&mut usage, &p, u);
        }
    }
    let mut verified = std::pin::pin!(engine.verify_many(&valid, limit));
    let (mut rows, mut texts) = (Vec::new(), Vec::new());
    let total = claims.len();
    let show_progress = out.format == Format::Table && total > 1 && std::io::stderr().is_terminal();
    for (input, claim) in claims {
        let (item, item_exit, text) = match claim {
            Err(e) => (
                json!({"input": input, "verdict": "invalid", "reason": e}),
                Exit::Usage,
                format!("INVALID  {input}\n  {e}\n"),
            ),
            Ok(_) => {
                let v = verified
                    .next()
                    .await
                    .expect("one verification per valid claim");
                for (p, u) in &v.report.usage {
                    merge_usage(&mut usage, p, *u);
                }
                let mut item = item(&input, &v, out.full);
                let mut text = verdict_text(&input, &v);
                if out.abstracts
                    && let Some(best) = v.best.map(|i| &v.report.candidates[i])
                {
                    // Search under every identifier merged into the match.
                    let mut record = best.record.clone();
                    record.ids = best.ids.clone();
                    let fetched = engine.abstracts(&[&record]).await;
                    for (p, u) in &fetched.usage {
                        merge_usage(&mut usage, p, *u);
                    }
                    let found = fetched.found.into_iter().next().flatten();
                    output::set_abstract(&mut item["match"]["work"], found.as_ref());
                    text.push_str(&output::abstract_text(found.as_ref(), "  "));
                    text.push('\n');
                }
                (item, exit_for(v.verdict), text)
            }
        };
        exit = exit.worst(item_exit);
        // Stream line-oriented formats so long batches show progress.
        match out.format {
            Format::Jsonl => print_line(&item),
            Format::Text => println!("{text}"),
            Format::Table => {
                rows.push(row(&item));
                texts.push(text);
                if show_progress {
                    eprint!("\rverified {}/{total}…", rows.len());
                }
            }
            _ => items.push(item),
        }
    }
    if show_progress {
        eprint!("\r\x1b[K");
    }

    match out.format {
        // One claim is detail, not rows: show it as text.
        Format::Table if texts.len() == 1 => {
            println!("{}", texts[0]);
            if let Some(text) = output::usage_text(&usage) {
                eprintln!("{text}");
            }
        }
        Format::Table => {
            let mut table = output::table(&["Verdict", "Conf", "Citation", "Reason"]);
            for row in rows {
                table.add_row(row);
            }
            println!("{table}");
            if let Some(text) = output::usage_text(&usage) {
                eprintln!("{text}");
            }
        }
        Format::Text => {
            if let Some(text) = output::usage_text(&usage) {
                eprintln!("{text}");
            }
        }
        Format::Jsonl => {}
        Format::Json | Format::Auto => {
            print(&json!({"results": items, "usage": output::usage(&usage)}));
        }
    }
    Ok(exit)
}

/// Positional text and the field flags.
pub fn split(args: ClaimArgs) -> (Option<String>, ClaimInput) {
    let fields = ClaimInput {
        title: args.title,
        authors: args.authors,
        year: args.year,
        ids: args.ids,
        ..ClaimInput::default()
    };
    (args.text, fields)
}

/// One verification as a table row: verdict, confidence, citation, reason.
fn row(item: &Value) -> Vec<comfy_table::Cell> {
    use comfy_table::Cell;
    let verdict = item["verdict"].as_str().unwrap_or("?");
    let level = match verdict {
        "verified" => Level::Good,
        "plausible" | "unverifiable" => Level::Warning,
        _ => Level::Error,
    };
    let confidence = item["match"]["confidence"]
        .as_f64()
        .map(|c| format!("{c:.2}"))
        .unwrap_or_default();
    vec![
        output::status_cell(&verdict.replace('_', " ").to_uppercase(), level),
        Cell::new(confidence),
        Cell::new(item["input"].as_str().unwrap_or("")),
        Cell::new(item["reason"].as_str().unwrap_or("")),
    ]
}

fn exit_for(verdict: Verdict) -> Exit {
    match verdict {
        Verdict::Verified => Exit::Ok,
        Verdict::NotFound => Exit::NotFound,
        Verdict::Unverifiable => Exit::Unavailable,
        _ => Exit::Unverified,
    }
}

fn item(input: &str, v: &Verification, full: bool) -> Value {
    let mut item = json!({
        "input": input,
        "verdict": v.verdict.as_str(),
        "reason": v.reason,
    });
    if let Some(best) = v.best {
        item["match"] = output::candidate(&v.report.candidates[best], full);
    }
    if !v.report.errors.is_empty() {
        let errors: Vec<String> = v.report.errors.iter().map(ToString::to_string).collect();
        item["errors"] = json!(errors);
    }
    if full {
        let all: Vec<Value> = v
            .report
            .candidates
            .iter()
            .map(|c| output::candidate(c, true))
            .collect();
        item["candidates"] = json!(all);
    }
    item
}

fn verdict_text(input: &str, v: &Verification) -> String {
    let mut lines = vec![
        format!(
            "{}  {input}",
            v.verdict.as_str().to_uppercase().replace('_', " ")
        ),
        format!("  {}", v.reason),
    ];
    if let Some(best) = v.best {
        lines.push(output::candidate_text(&v.report.candidates[best], "  "));
    }
    for e in &v.report.errors {
        lines.push(format!("  warning: {e}"));
    }
    lines.join("\n") + "\n"
}
