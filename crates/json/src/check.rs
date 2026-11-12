//! `check` results: a bibliography's entries with their findings.

use aspero_bib::BibEntry;
use aspero_engine::{BibliographyReport, CheckSummary, CheckedEntry, Finding, Severity, bare_doi};
use serde_json::{Value, json};

use crate::work;

/// The whole check: the file, a summary, every entry, and the usage.
pub fn report(path: &str, entries: &[BibEntry], report: &BibliographyReport, full: bool) -> Value {
    let keys = keys(entries);
    json!({
        "file": path,
        "summary": summary(&report.summary()),
        "entries": entries
            .iter()
            .zip(&report.entries)
            .map(|(e, checked)| entry(e, checked, &keys, full))
            .collect::<Vec<_>>(),
        "usage": work::usage(&report.usage),
    })
}

/// The entries' keys, which a duplicate's `of` indexes.
pub fn keys(entries: &[BibEntry]) -> Vec<&str> {
    entries.iter().map(|e| e.key.as_str()).collect()
}

/// One entry: key, type, verdict (`skipped` when not checked), its most
/// severe finding, every finding, and the work it matched.
pub fn entry(entry: &BibEntry, checked: &CheckedEntry, keys: &[&str], full: bool) -> Value {
    let mut doc = json!({
        "key": entry.key,
        "type": entry.entry_type,
        "verdict": checked.verdict().map_or("skipped", |v| v.as_str()),
        "severity": checked.severity().map(Severity::as_str),
        "findings": checked.findings.iter().map(|f| finding(f, keys)).collect::<Vec<_>>(),
    });
    if let Some(m) = checked.matched() {
        doc["match"] = work::candidate(m, full);
    }
    doc
}

/// One finding: kind, severity, message, and its details.
pub fn finding(f: &Finding, keys: &[&str]) -> Value {
    let mut out =
        json!({"kind": f.kind(), "severity": f.severity().as_str(), "message": f.message(keys)});
    let detail = match f {
        Finding::AddDoi { doi } => json!({"doi": bare_doi(doi)}),
        Finding::ReplaceId { claimed, with } => match with {
            Some(with) => json!({"claimed": claimed, "with": with}),
            None => json!({"claimed": claimed}),
        },
        Finding::YearDiffers { claimed, actual } => json!({"claimed": claimed, "actual": actual}),
        Finding::Duplicate { of } => json!({"of": keys.get(*of).copied().unwrap_or("?")}),
        Finding::Updated { update } => json!({"update": update}),
        Finding::Entry { note } => json!({
            "field": note.field,
            "value": note.value,
            "problem": note.problem.as_str(),
        }),
        _ => Value::Null,
    };
    if let Value::Object(detail) = detail {
        for (k, v) in detail {
            out[k] = v;
        }
    }
    out
}

/// Entries counted by outcome.
pub fn summary(s: &CheckSummary) -> Value {
    json!({
        "entries": s.entries,
        "verified": s.verified,
        "errors": s.errors,
        "warnings": s.warnings,
        "suggestions": s.suggestions,
        "skipped": s.skipped,
    })
}
