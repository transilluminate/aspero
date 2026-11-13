//! `aspero references` and `aspero analyse`: citation links.

use std::io::IsTerminal;

use aspero_bib::BibEntry;
use aspero_core::{Cursor, IdKind, SourceRecord, WorkId};
use aspero_engine::{
    Analysis, AnalysisKind, Engine, Incomplete, IncompleteKind, ReferencesPage, SeedProblem,
};
use serde_json::Value;

use super::{Failure, Out, lines, print, print_line, read_stdin, shell_quote};
use crate::args::{AnalysisArg, Format};
use crate::exit::Exit;
use crate::output;

pub async fn references(
    engine: &Engine,
    id: String,
    limit: u32,
    cursor: Option<String>,
    out: Out,
) -> Result<Exit, Failure> {
    let cursor = cursor.map(Cursor);
    let page = engine.references(&id, cursor.as_ref(), limit).await?;
    let doc = aspero_json::graph::references(&id, &page, out.full);
    match out.format {
        Format::Jsonl => {
            for item in doc["references"].as_array().into_iter().flatten() {
                print_line(item);
            }
        }
        Format::Table | Format::Text => {
            print!("{}", references_text(&page));
            if let Some(next) = &page.next_cursor {
                eprintln!(
                    "next page: aspero references {} --limit {limit} --cursor {}",
                    shell_quote(&id),
                    next.0
                );
            }
            if let Some(text) = output::usage_text(&page.usage) {
                eprintln!("{text}");
            }
        }
        Format::Bibtex | Format::Ris | Format::Cite => unreachable!("rejected in main"),
        Format::Json | Format::Auto => print(&doc),
    }
    Ok(Exit::Ok)
}

/// The work, its coverage, and each reference on a line.
fn references_text(page: &ReferencesPage) -> String {
    let title = page
        .record
        .as_ref()
        .and_then(|r| r.work.title.as_deref())
        .map(|t| format!(" \"{t}\""))
        .unwrap_or_default();
    let mut out = match page.total {
        Some(total) => format!("{}{title}: cites {total} works\n", page.work),
        None => format!(
            "{}{title}: references unknown (OpenAlex has no reference data for it)\n",
            page.work
        ),
    };
    for r in &page.references {
        let line = match &r.record {
            Some(record) => one_line(record),
            None => "(not found)".into(),
        };
        out.push_str(&format!("  {}  {line}\n", r.id));
    }
    out
}

/// "Title" — Author et al. 2015
fn one_line(record: &SourceRecord) -> String {
    let w = &record.work;
    let mut line = format!("\"{}\"", w.title.as_deref().unwrap_or("(untitled)"));
    let by = [output::first_author(w), output::year(w)]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if !by.is_empty() {
        line.push_str(&format!(" — {by}"));
    }
    line
}

/// The seeds of a bibliography: each entry's identifier (the one OpenAlex
/// is likeliest to know), its key, and the keys of entries without one.
struct FileSeeds {
    inputs: Vec<String>,
    keys: Vec<String>,
    skipped: Vec<String>,
}

fn file_seeds(entries: &[BibEntry]) -> FileSeeds {
    let rank = |id: &WorkId| match id.kind() {
        IdKind::Doi => 0,
        IdKind::Arxiv => 1,
        IdKind::Pmid => 2,
        IdKind::OpenAlex => 3,
        IdKind::Pmcid => 4,
        _ => 5,
    };
    let mut seeds = FileSeeds {
        inputs: Vec::new(),
        keys: Vec::new(),
        skipped: Vec::new(),
    };
    for entry in entries {
        match entry.claim.ids.iter().min_by_key(|id| rank(id)) {
            Some(id) => {
                seeds.inputs.push(id.to_string());
                seeds.keys.push(entry.key.clone());
            }
            None => seeds.skipped.push(entry.key.clone()),
        }
    }
    seeds
}

pub async fn analyse(
    engine: &Engine,
    inputs: Vec<String>,
    kinds: Vec<AnalysisArg>,
    limit: u32,
    budget: Option<f64>,
    estimate: bool,
    out: Out,
) -> Result<Exit, Failure> {
    if budget.is_some_and(|b| b.is_nan() || b < 0.0) {
        return Err(Failure::usage("--budget must be zero or more US dollars"));
    }
    let file = match inputs.as_slice() {
        [path] if path != "-" && std::path::Path::new(path).is_file() => Some(path.clone()),
        _ => None,
    };
    let seeds = match &file {
        Some(path) => {
            let source = std::fs::read(path)
                .map_err(|e| Failure::usage(format!("could not read {path}: {e}")))?;
            let entries = super::check::entries(path, &source)?;
            file_seeds(&entries)
        }
        None => FileSeeds {
            inputs: identifiers(inputs)?,
            keys: Vec::new(),
            skipped: Vec::new(),
        },
    };
    if seeds.inputs.is_empty() && seeds.skipped.is_empty() {
        return Err(Failure::usage("no works given"));
    }
    let kinds: Vec<AnalysisKind> = kinds
        .into_iter()
        .map(|k| match k {
            AnalysisArg::Retracted => AnalysisKind::RetractedReferences,
            AnalysisArg::OddOneOut => AnalysisKind::OddOneOut,
            AnalysisArg::ProbablyMissing => AnalysisKind::ProbablyMissing { limit },
            AnalysisArg::Related => AnalysisKind::Related,
        })
        .collect();
    let analysis = engine
        .analyse(&seeds.inputs, &kinds, budget, estimate)
        .await?;

    let mut doc = aspero_json::graph::analysis(&analysis, out.full);
    if file.is_some() {
        let keys: Vec<&str> = seeds.keys.iter().map(String::as_str).collect();
        let skipped: Vec<&str> = seeds.skipped.iter().map(String::as_str).collect();
        aspero_json::graph::file_seeds(&mut doc, &keys, &skipped);
    }
    match out.format {
        Format::Jsonl => print_line(&doc),
        Format::Table | Format::Text => {
            print!("{}", analysis_text(&analysis, &doc));
            if let Some(text) = output::usage_text(&analysis.usage) {
                eprintln!("{text}");
            }
        }
        Format::Bibtex | Format::Ris | Format::Cite => unreachable!("rejected in main"),
        Format::Json | Format::Auto => print(&doc),
    }
    Ok(exit_for(&analysis))
}

/// Arguments, or one identifier per stdin line when given none (or `-`).
fn identifiers(inputs: Vec<String>) -> Result<Vec<String>, Failure> {
    if !(inputs.is_empty() || inputs == ["-"]) {
        return Ok(inputs);
    }
    if std::io::stdin().is_terminal() {
        return Err(Failure::usage(
            "no works given (pass identifiers or a .bib file, or pipe one identifier per line)",
        ));
    }
    Ok(lines(&read_stdin()?).map(str::to_owned).collect())
}

/// The most serious problem among the seeds and analyses; a budget
/// reached is not one (it was asked for).
fn exit_for(a: &Analysis) -> Exit {
    let seeds = a.seeds.iter().filter_map(|s| s.problem).map(|p| match p {
        SeedProblem::InvalidId => Exit::Usage,
        SeedProblem::NotFound => Exit::NotFound,
        SeedProblem::UnsupportedId => Exit::Unsupported,
        SeedProblem::ProviderFailed => Exit::Unavailable,
        _ => Exit::Ok,
    });
    let stopped = [
        a.retracted_references.as_ref().and_then(|s| s.incomplete),
        a.odd_one_out.as_ref().and_then(|s| s.incomplete),
        a.probably_missing.as_ref().and_then(|s| s.incomplete),
    ];
    let failed = stopped
        .into_iter()
        .flatten()
        .filter(|i| i.kind == IncompleteKind::ProviderFailed)
        .map(|_| Exit::Unavailable);
    seeds.chain(failed).fold(Exit::Ok, Exit::worst)
}

/// 4300 → "4,300".
fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "stopped at the budget: 1,200 of 4,300 references checked".
fn stopped(i: &Incomplete, what: &str) -> String {
    let why = match i.kind {
        IncompleteKind::Budget => "stopped at the budget",
        IncompleteKind::ProviderFailed => "OpenAlex failed",
        _ => "capped",
    };
    format!(
        "  ⚠ {why}: {} of {} {what}\n",
        thousands(i.done),
        thousands(i.total)
    )
}

/// The analysis as a readable report.
fn analysis_text(a: &Analysis, doc: &Value) -> String {
    let label = |i: usize| {
        let seed = &doc["seeds"][i];
        seed["key"]
            .as_str()
            .or(seed["input"].as_str())
            .unwrap_or("?")
            .to_owned()
    };
    let resolved = a.seeds.iter().filter(|s| s.record.is_some()).count();
    let mut out = format!(
        "{} works, {resolved} found at OpenAlex, {} with known references\n",
        doc["seeds"].as_array().map_or(0, Vec::len),
        a.with_references
    );
    for (i, seed) in doc["seeds"].as_array().into_iter().flatten().enumerate() {
        let detail = match seed["status"].as_str() {
            Some("resolved") => {
                let refs = match &seed["references"] {
                    Value::Number(n) if n.as_u64() == Some(1) => "1 reference".into(),
                    Value::Number(n) => format!("{n} references"),
                    _ => "references unknown".into(),
                };
                let same = seed["same_as"]
                    .as_u64()
                    .map(|f| format!(", same work as [{f}]"))
                    .unwrap_or_default();
                let record = a.seeds.get(i).and_then(|s| s.record.as_ref());
                format!(
                    "{}  {refs}{same}  {}",
                    seed["id"].as_str().unwrap_or("?"),
                    record.map(one_line).unwrap_or_default()
                )
            }
            Some("skipped") => "skipped: no identifier".into(),
            _ => format!(
                "{} ({})",
                seed["reason"].as_str().unwrap_or("?").replace('_', " "),
                seed["error"].as_str().unwrap_or("")
            ),
        };
        out.push_str(&format!("  [{i}] {}  {detail}\n", label(i)));
    }

    if let Some(found) = &a.retracted_references {
        out.push_str("\nRetracted references\n");
        if found.items.is_empty() {
            out.push_str("  none found\n");
        }
        for r in &found.items {
            out.push_str(&format!(
                "  [{}] {} cites {}  {} (retracted, says {})\n",
                r.seed,
                label(r.seed),
                r.cites
                    .ids
                    .first()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                one_line(&r.cites),
                r.source
            ));
        }
        if let Some(i) = &found.incomplete {
            out.push_str(&stopped(i, "references checked"));
        }
    }
    if let Some(found) = &a.odd_one_out {
        out.push_str("\nOdd ones out\n");
        if found.items.is_empty() {
            out.push_str("  none: every work is linked to another\n");
        }
        for o in &found.items {
            let why = match o.reason.as_str() {
                "no_links" => "cites no other work of the set, none cites it, shares no references",
                _ => "its references are unknown, and no other work of the set cites it",
            };
            out.push_str(&format!("  [{}] {}: {why}\n", o.seed, label(o.seed)));
        }
    }
    if let Some(found) = &a.probably_missing {
        out.push_str(&format!(
            "\nProbably missing (cited by k of the {} works with known references)\n",
            a.with_references
        ));
        if found.items.is_empty() {
            out.push_str("  none: no work outside the set is cited by two of it\n");
        }
        for m in &found.items {
            let by: Vec<String> = m.cited_by.iter().map(|i| format!("[{i}]")).collect();
            let work = m.record.as_ref().map(one_line).unwrap_or_default();
            out.push_str(&format!(
                "  {}  {}  {work}  cited by {}\n",
                m.count(),
                m.id,
                by.join(" ")
            ));
        }
        if let Some(i) = &found.incomplete {
            out.push_str(&stopped(i, "works looked up"));
        }
    }
    for kind in &a.not_yet {
        out.push_str(&format!("\n{}: not yet\n", kind.as_str().replace('_', " ")));
    }
    if let Some(i) = &a.incomplete {
        out.push_str(&format!("\nSeeds{}", stopped(i, "looked up")));
    }
    if let Some(e) = &a.estimate {
        let floor = if e.exact { "" } else { " at least" };
        out.push_str(&format!(
            "\nEstimate:{floor} {} paid requests, ${:.4}\n",
            thousands(e.requests),
            e.usd
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use aspero_core::{Update, UpdateKind};

    use super::*;
    use crate::cmd::testing::{Scripted, engine, pinned, work};

    fn cites(mut record: SourceRecord, ids: &[&str]) -> SourceRecord {
        record.references = Some(ids.iter().map(|i| i.parse().unwrap()).collect());
        record
    }

    fn works() -> Vec<SourceRecord> {
        let mut retracted = work(
            &["W40", "10.1234/retracted"],
            "A retracted study",
            &["Jane Doe"],
            2019,
        );
        let mut retraction = Update::new(UpdateKind::Retraction);
        retraction.source = Some("openalex".into());
        retracted.work.updates = vec![retraction];
        vec![
            cites(
                work(
                    &["W1", "10.1234/a"],
                    "Fluids in sepsis",
                    &["Ann Smith", "Bo Chen"],
                    2020,
                ),
                &["W10", "W11", "W40"],
            ),
            cites(
                work(
                    &["W2", "10.1234/b"],
                    "Vasopressors in sepsis",
                    &["Bo Chen"],
                    2021,
                ),
                &["W1", "W10", "W11"],
            ),
            work(
                &["W3", "10.1234/book"],
                "A book on sepsis",
                &["Cy Dee"],
                2001,
            ),
            cites(
                work(&["W4", "10.1234/knitting"], "Knitting", &["Di Wool"], 2022),
                &["W99"],
            ),
            work(
                &["W10"],
                "Early goal-directed therapy",
                &["Ed Rivers"],
                2001,
            ),
            work(&["W11"], "Surviving sepsis", &["Fay Dell"], 2016),
            retracted,
        ]
    }

    /// What `references` and `analyse` print, byte for byte: JSON (the
    /// contract) and text.
    #[tokio::test]
    async fn output_is_pinned() {
        let engine = engine(vec![Scripted::new("openalex", works())]);
        let page = engine.references("10.1234/a", None, 2).await.unwrap();
        let json = aspero_json::graph::references("10.1234/a", &page, false);
        pinned(
            "graph/references.json",
            &(serde_json::to_string_pretty(&json).unwrap() + "\n"),
        );
        pinned("graph/references.txt", &references_text(&page));

        let inputs: Vec<String> = [
            "10.1234/a",
            "10.1234/b",
            "10.1234/book",
            "10.1234/knitting",
            "10.1234/missing",
            "isbn:9780262033848",
        ]
        .map(String::from)
        .to_vec();
        let kinds = [
            AnalysisKind::RetractedReferences,
            AnalysisKind::OddOneOut,
            AnalysisKind::ProbablyMissing { limit: 5 },
            AnalysisKind::Related,
        ];
        let analysis = engine.analyse(&inputs, &kinds, None, false).await.unwrap();
        let json = aspero_json::graph::analysis(&analysis, false);
        pinned(
            "graph/analysis.json",
            &(serde_json::to_string_pretty(&json).unwrap() + "\n"),
        );
        pinned("graph/analysis.txt", &analysis_text(&analysis, &json));
        assert_eq!(exit_for(&analysis), Exit::Unsupported);

        let estimate = engine.analyse(&inputs, &kinds, None, true).await.unwrap();
        let json = aspero_json::graph::analysis(&estimate, false);
        pinned(
            "graph/estimate.json",
            &(serde_json::to_string_pretty(&json).unwrap() + "\n"),
        );
    }

    #[test]
    fn numbers_read_in_thousands() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(4300), "4,300");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }
}
