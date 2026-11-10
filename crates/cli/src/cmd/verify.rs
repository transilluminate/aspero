//! `aspero verify`: does a citation refer to a real work, with matching
//! details?

use aspero_core::{ProviderId, Usage};
use aspero_engine::{Engine, Verdict, Verification};
use serde_json::{Value, json};

use super::claim::{ClaimInput, claims};
use super::{Failure, Out, print, print_line};
use crate::args::{ClaimArgs, Format};
use crate::exit::Exit;
use crate::output;

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
    // warm, claims are then verified (and streamed) one by one cheaply.
    if claims.len() > 1 {
        let valid: Vec<_> = claims
            .iter()
            .filter_map(|(_, c)| c.as_ref().ok().cloned())
            .collect();
        for (p, u) in engine.prefetch(&valid).await.usage {
            merge_usage(&mut usage, &p, u);
        }
    }
    for (input, claim) in claims {
        let (item, item_exit, text) = match claim {
            Err(e) => (
                json!({"input": input, "verdict": "invalid", "reason": e}),
                Exit::Usage,
                format!("INVALID  {input}\n  {e}\n"),
            ),
            Ok(claim) => {
                let v = engine.verify(&claim, limit).await;
                for (p, u) in &v.report.usage {
                    merge_usage(&mut usage, p, *u);
                }
                (
                    item(&input, &v, out.full),
                    exit_for(v.verdict),
                    verdict_text(&input, &v),
                )
            }
        };
        exit = exit.worst(item_exit);
        // Stream line-oriented formats so long batches show progress.
        match out.format {
            Format::Jsonl => print_line(&item),
            Format::Table => println!("{text}"),
            _ => items.push(item),
        }
    }

    match out.format {
        Format::Table => {
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

fn merge_usage(totals: &mut Vec<(ProviderId, Usage)>, provider: &ProviderId, usage: Usage) {
    match totals.iter_mut().find(|(p, _)| p == provider) {
        Some((_, total)) => *total = total.then(usage),
        None => totals.push((provider.clone(), usage)),
    }
}
