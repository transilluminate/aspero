//! `aspero resolve`: a messy citation (or identifier) to candidate works.
//!
//! Phase 0: candidates are listed per provider with that provider's score.
//! Cross-provider confidence scoring comes with `verify`.

use aspero_core::{FieldSet, SearchQuery, WorkId};
use aspero_engine::Engine;
use serde_json::json;

use super::{Failure, Out, lookup, print, print_line, read_stdin};
use crate::args::Format;
use crate::exit::Exit;
use crate::output;

const NOTE: &str = "Scores are per provider and not comparable. Cross-provider confidence is \
    not implemented yet; check year and venue for lookalikes.";

pub async fn run(engine: &Engine, text: String, limit: u32, out: Out) -> Result<Exit, Failure> {
    let text = if text == "-" { read_stdin()? } else { text };
    let text = text.trim().to_owned();
    if text.is_empty() {
        return Err(Failure::usage("nothing to resolve"));
    }
    match text.parse::<WorkId>() {
        Ok(id) => identifier(engine, &text, id, out).await,
        Err(_) => citation(engine, &text, limit, out).await,
    }
}

/// Identifiers short-circuit to a lookup.
async fn identifier(engine: &Engine, text: &str, id: WorkId, out: Out) -> Result<Exit, Failure> {
    let report = engine
        .lookup(std::slice::from_ref(&id), FieldSet::DEFAULT)
        .await;
    let result = &report.results[0];
    let (item, exit) = lookup::item(text, result, out.full);
    match out.format {
        Format::Table => println!("{}", lookup::text(&item, Some(result))),
        Format::Jsonl => print_line(&item),
        Format::Json | Format::Auto => print(&json!({
            "input": text,
            "kind": "identifier",
            "results": [item],
            "usage": output::usage(&report.usage),
        })),
    }
    Ok(exit)
}

/// Every searchable provider: citation matchers get the whole reference,
/// others a keyword search.
async fn citation(engine: &Engine, text: &str, limit: u32, out: Out) -> Result<Exit, Failure> {
    let mut candidates = Vec::new();
    let mut errors = Vec::new();
    let mut usage = Vec::new();
    let mut table = String::new();
    let mut exit = Exit::Ok;

    let searchable: Vec<_> = engine
        .providers()
        .filter(|p| p.capabilities().search)
        .map(|p| (p.id(), p.capabilities().citation_search))
        .collect();
    for (provider, is_matcher) in searchable {
        let (mut query, mode) = if is_matcher {
            (SearchQuery::citation(text), "citation match")
        } else {
            (SearchQuery::new(text), "keyword search")
        };
        query.page.limit = limit;
        match engine.search(Some(provider.as_str()), &query).await {
            Ok(report) => {
                table.push_str(&format!("{provider} ({mode}):\n"));
                for (i, hit) in report.page.hits.iter().enumerate() {
                    candidates.push(json!({
                        "provider": provider,
                        "rank": i + 1,
                        "score": hit.score,
                        "work": output::work(&hit.record, out.full),
                    }));
                    table.push_str(&output::hit_text(i + 1, hit));
                    table.push('\n');
                }
                table.push('\n');
                if let Some(u) = report.usage {
                    usage.push((provider.clone(), u));
                }
            }
            Err(e) => {
                let failure = Failure::from(e);
                exit = exit.worst(failure.exit);
                errors.push(json!({"provider": provider, "error": failure.message}));
                table.push_str(&format!("{provider}: {}\n\n", failure.message));
            }
        }
    }
    // Any candidates make the command a success; failed providers are listed
    // in `errors`.
    let exit = if candidates.is_empty() {
        exit.worst(Exit::NotFound)
    } else {
        Exit::Ok
    };

    match out.format {
        Format::Table => {
            print!("{table}");
            eprintln!("note: {NOTE}");
            if let Some(text) = output::usage_text(&usage) {
                eprintln!("{text}");
            }
        }
        Format::Jsonl => candidates.iter().for_each(print_line),
        Format::Json | Format::Auto => print(&json!({
            "input": text,
            "kind": "citation",
            "candidates": candidates,
            "errors": errors,
            "usage": output::usage(&usage),
            "note": NOTE,
        })),
    }
    Ok(exit)
}
