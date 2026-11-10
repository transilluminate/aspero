//! `aspero check`: verify every entry of a .bib file and report fixes.

use std::io::IsTerminal;

use aspero_bib::BibEntry;
use aspero_engine::{Checked, Engine, Finding, Severity, Verdict, describe_update};
use serde_json::{Value, json};

use super::{Failure, Out, print, print_line, read_stdin};
use crate::args::{FailOn, Format};
use crate::exit::Exit;
use crate::output;

pub async fn run(
    engine: &Engine,
    path: String,
    fail_on: FailOn,
    limit: u32,
    out: Out,
) -> Result<Exit, Failure> {
    let source = if path == "-" {
        read_stdin()?
    } else {
        std::fs::read_to_string(&path)
            .map_err(|e| Failure::usage(format!("could not read {path}: {e}")))?
    };
    let entries = aspero_bib::parse(&source).map_err(|e| Failure::usage(format!("{path}: {e}")))?;
    if entries.is_empty() {
        return Err(Failure::usage(format!("{path}: no entries")));
    }

    let checkable: Vec<&BibEntry> = entries.iter().filter(|e| e.is_checkable()).collect();
    let claims: Vec<_> = checkable.iter().map(|e| e.claim.clone()).collect();
    let show_progress = out.format == Format::Table && std::io::stderr().is_terminal();
    let total = claims.len();
    let checked = engine
        .check(&claims, limit, |i| {
            if show_progress {
                eprint!("\rchecking {}/{total}…", i + 1);
            }
        })
        .await;
    if show_progress {
        eprint!("\r\x1b[K");
    }

    // Reports in file order, skipped entries included.
    let mut by_entry = checked.iter();
    let keys: Vec<&str> = checkable.iter().map(|e| e.key.as_str()).collect();
    let mut reports = Vec::new();
    for entry in &entries {
        let checked = entry
            .is_checkable()
            .then(|| by_entry.next().expect("one per checkable"));
        reports.push(Report::new(entry, checked, &keys));
    }

    let summary = Summary::of(&reports);
    let failed = reports.iter().any(|r| fails(r.severity, fail_on));
    let usage = checked
        .iter()
        .flat_map(|c| &c.verification.report.usage)
        .fold(Vec::new(), |mut acc, (p, u)| {
            super::merge_usage(&mut acc, p, *u);
            acc
        });

    match out.format {
        Format::Table => {
            for report in &reports {
                print!("{}", report.text());
            }
            println!("\n{}", summary.text(&path));
            if let Some(text) = output::usage_text(&usage) {
                eprintln!("{text}");
            }
        }
        Format::Jsonl => reports.iter().for_each(|r| print_line(&r.json(out.full))),
        Format::Json | Format::Auto => print(&json!({
            "file": path,
            "summary": summary.json(),
            "entries": reports.iter().map(|r| r.json(out.full)).collect::<Vec<_>>(),
            "usage": output::usage(&usage),
        })),
    }
    Ok(if failed { Exit::Unverified } else { Exit::Ok })
}

fn fails(severity: Option<Severity>, fail_on: FailOn) -> bool {
    let threshold = match fail_on {
        FailOn::Error => Severity::Error,
        FailOn::Warning => Severity::Warning,
        FailOn::Info => Severity::Info,
        FailOn::Never => return false,
    };
    severity.is_some_and(|s| s >= threshold)
}

/// One finding, ready to show.
struct Line {
    kind: &'static str,
    severity: Severity,
    message: String,
    detail: Value,
}

struct Report<'a> {
    entry: &'a BibEntry,
    checked: Option<&'a Checked>,
    lines: Vec<Line>,
    severity: Option<Severity>,
}

impl<'a> Report<'a> {
    fn new(entry: &'a BibEntry, checked: Option<&'a Checked>, keys: &[&str]) -> Self {
        let mut lines: Vec<Line> = entry
            .notes
            .iter()
            .map(|note| Line {
                kind: "entry",
                severity: Severity::Warning,
                message: note.clone(),
                detail: Value::Null,
            })
            .collect();
        match checked {
            None => lines.push(Line {
                kind: "skipped",
                severity: Severity::Warning,
                message: "nothing to check: no title or identifier".into(),
                detail: Value::Null,
            }),
            Some(c) => lines.extend(c.findings.iter().map(|f| line(f, keys))),
        }
        let severity = lines.iter().map(|l| l.severity).max();
        Report {
            entry,
            checked,
            lines,
            severity,
        }
    }

    fn verdict(&self) -> &'static str {
        self.checked
            .map_or("skipped", |c| c.verification.verdict.as_str())
    }

    fn text(&self) -> String {
        let status = match self.severity {
            None => return format!("ok       {}\n", self.entry.key),
            Some(s) => s.as_str().to_uppercase(),
        };
        let mut out = format!(
            "{status:<8} {}  ({})\n",
            self.entry.key,
            self.verdict().replace('_', " ")
        );
        for line in &self.lines {
            out.push_str(&format!("         - {}\n", line.message));
        }
        out
    }

    fn json(&self, full: bool) -> Value {
        let findings: Vec<Value> = self
            .lines
            .iter()
            .map(|l| {
                let mut f =
                    json!({"kind": l.kind, "severity": l.severity.as_str(), "message": l.message});
                if let Value::Object(detail) = &l.detail {
                    for (k, v) in detail {
                        f[k] = v.clone();
                    }
                }
                f
            })
            .collect();
        let mut doc = json!({
            "key": self.entry.key,
            "type": self.entry.entry_type,
            "verdict": self.verdict(),
            "severity": self.severity.map(Severity::as_str),
            "findings": findings,
        });
        if let Some(m) = self.checked.and_then(Checked::matched) {
            doc["match"] = output::candidate(m, full);
        }
        doc
    }
}

fn line(finding: &Finding, keys: &[&str]) -> Line {
    let (message, detail) = match finding {
        Finding::Verdict { reason, .. } => (reason.clone(), Value::Null),
        Finding::AddDoi { doi } => {
            let bare = doi.to_string().trim_start_matches("doi:").to_owned();
            (format!("add doi = {{{bare}}}"), json!({"doi": bare}))
        }
        Finding::ReplaceId { claimed, with } => match with {
            Some(with) => (
                format!("replace {claimed} with {with}"),
                json!({"claimed": claimed, "with": with}),
            ),
            None => (
                format!("{claimed} is not this work's identifier"),
                json!({"claimed": claimed}),
            ),
        },
        Finding::YearDiffers { claimed, actual } => (
            if (claimed - actual).abs() == 1 {
                format!(
                    "year {claimed} → {actual} (often preprint vs published, or online vs print)"
                )
            } else {
                format!("year {claimed} → {actual}")
            },
            json!({"claimed": claimed, "actual": actual}),
        ),
        Finding::Duplicate { of } => {
            let key = keys.get(*of).copied().unwrap_or("?");
            (format!("duplicate of {key}"), json!({"of": key}))
        }
        Finding::Updated { update } => (describe_update(update), json!({"update": update})),
        _ => (format!("{finding:?}"), Value::Null),
    };
    Line {
        kind: finding.kind(),
        severity: finding.severity(),
        message,
        detail,
    }
}

#[derive(Default)]
struct Summary {
    entries: usize,
    verified: usize,
    errors: usize,
    warnings: usize,
    infos: usize,
    skipped: usize,
}

impl Summary {
    fn of(reports: &[Report]) -> Self {
        let mut s = Summary {
            entries: reports.len(),
            ..Summary::default()
        };
        for r in reports {
            match r.checked {
                None => s.skipped += 1,
                Some(c) if c.verification.verdict == Verdict::Verified => s.verified += 1,
                Some(_) => {}
            }
            match r.severity {
                Some(Severity::Error) => s.errors += 1,
                Some(Severity::Warning) => s.warnings += 1,
                Some(Severity::Info) => s.infos += 1,
                _ => {}
            }
        }
        s
    }

    fn text(&self, path: &str) -> String {
        format!(
            "{path}: {} entries, {} verified; {} with errors, {} with warnings, {} with suggestions, {} skipped",
            self.entries, self.verified, self.errors, self.warnings, self.infos, self.skipped
        )
    }

    fn json(&self) -> Value {
        json!({
            "entries": self.entries,
            "verified": self.verified,
            "errors": self.errors,
            "warnings": self.warnings,
            "suggestions": self.infos,
            "skipped": self.skipped,
        })
    }
}
