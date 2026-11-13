//! `aspero resolve`: every candidate work for a citation, scored.

use aspero_core::SourceRecord;
use aspero_engine::{Candidate, Engine, add_usage};
use serde_json::{Value, json};

use super::claim::claims;
use super::verify::split;
use super::{Failure, Out, merged, print, print_line};
use crate::args::{ClaimArgs, Format};
use crate::exit::Exit;
use crate::output;

pub async fn run(engine: &Engine, args: ClaimArgs, out: Out) -> Result<Exit, Failure> {
    let limit = args.limit;
    let split_list = args.split;
    let (text, fields) = split(args);
    let mut exit = Exit::Ok;
    let mut docs = Vec::new();

    for item in claims(text, fields, split_list)? {
        let echo = item.echo();
        let input = item.input;
        let claim = match item.claim {
            Ok(c) => c,
            Err(e) => {
                exit = exit.worst(Exit::Usage);
                docs.push(merged(echo, json!({"error": e})));
                continue;
            }
        };
        let mut report = engine.resolve(&claim, limit).await;
        if report.candidates.is_empty() {
            exit = exit.worst(Exit::NotFound);
        }
        let abstracts = if out.abstracts {
            // Search under every identifier merged into each candidate.
            let records: Vec<SourceRecord> = report
                .candidates
                .iter()
                .map(Candidate::merged_record)
                .collect();
            let fetched = engine.abstracts(&records.iter().collect::<Vec<_>>()).await;
            for (p, u) in &fetched.usage {
                add_usage(&mut report.usage, p, *u);
            }
            Some(fetched.found)
        } else {
            None
        };
        let found = |i: usize| abstracts.as_ref().map(|a| a[i].as_ref());
        let candidates: Vec<Value> = report
            .candidates
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let mut json = output::candidate(c, out.full);
                if let Some(a) = found(i) {
                    output::set_abstract(&mut json["work"], a);
                }
                json
            })
            .collect();
        let errors: Vec<String> = report.errors.iter().map(ToString::to_string).collect();

        match out.format {
            Format::Table => {
                println!("{input}");
                if report.candidates.is_empty() {
                    println!("  no candidates");
                } else {
                    let mut table = output::table(&[
                        "#", "Conf", "Checks", "Year", "Author", "Title", "ID", "Found by",
                    ]);
                    for (i, c) in report.candidates.iter().enumerate() {
                        table.add_row(output::candidate_row(i + 1, c));
                    }
                    println!("{table}");
                }
                for (i, c) in report.candidates.iter().enumerate() {
                    if let Some(a) = found(i) {
                        let id = output::main_id(&c.ids);
                        println!("\n{}. {id}\n{}", i + 1, output::abstract_text(a, "  "));
                    }
                }
                for e in &errors {
                    eprintln!("warning: {e}");
                }
                if let Some(text) = output::usage_text(&report.usage) {
                    eprintln!("{text}");
                }
            }
            Format::Text => {
                println!("{input}\n");
                for (i, c) in report.candidates.iter().enumerate() {
                    println!("{}", output::candidate_text(c, "  "));
                    if let Some(a) = found(i) {
                        println!("{}", output::abstract_text(a, "  "));
                    }
                    println!();
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
            Format::Bibtex | Format::Ris | Format::Cite => unreachable!("rejected in main"),
            Format::Json | Format::Auto => docs.push(merged(
                echo,
                json!({
                    "candidates": candidates,
                    "errors": errors,
                    "usage": output::usage(&report.usage),
                }),
            )),
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
