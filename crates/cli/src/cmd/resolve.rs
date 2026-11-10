//! `aspero resolve`: every candidate work for a citation, scored.

use aspero_engine::Engine;
use serde_json::{Value, json};

use super::claim::claims;
use super::verify::split;
use super::{Failure, Out, print, print_line};
use crate::args::{ClaimArgs, Format};
use crate::exit::Exit;
use crate::output;

pub async fn run(engine: &Engine, args: ClaimArgs, out: Out) -> Result<Exit, Failure> {
    let limit = args.limit;
    let (text, fields) = split(args);
    let mut exit = Exit::Ok;
    let mut docs = Vec::new();

    for (input, claim) in claims(text, fields)? {
        let claim = match claim {
            Ok(c) => c,
            Err(e) => {
                exit = exit.worst(Exit::Usage);
                docs.push(json!({"input": input, "error": e}));
                continue;
            }
        };
        let report = engine.resolve(&claim, limit).await;
        if report.candidates.is_empty() {
            exit = exit.worst(Exit::NotFound);
        }
        let candidates: Vec<Value> = report
            .candidates
            .iter()
            .map(|c| output::candidate(c, out.full))
            .collect();
        let errors: Vec<String> = report.errors.iter().map(ToString::to_string).collect();

        match out.format {
            Format::Table => {
                println!("{input}\n");
                for c in &report.candidates {
                    println!("{}\n", output::candidate_text(c, "  "));
                }
                if report.candidates.is_empty() {
                    println!("  no candidates\n");
                }
                for e in &errors {
                    eprintln!("warning: {e}");
                }
                if let Some(text) = output::usage_text(&report.usage) {
                    eprintln!("{text}");
                }
            }
            Format::Jsonl => candidates.iter().for_each(print_line),
            Format::Json | Format::Auto => docs.push(json!({
                "input": input,
                "candidates": candidates,
                "errors": errors,
                "usage": output::usage(&report.usage),
            })),
        }
    }

    if matches!(out.format, Format::Json | Format::Auto) {
        match docs.as_slice() {
            [single] => print(single),
            _ => print(&json!({"results": docs})),
        }
    }
    Ok(exit)
}
