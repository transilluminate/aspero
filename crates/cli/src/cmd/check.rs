//! `aspero check`: verify every entry of a .bib file and report fixes.

use std::fmt::Write as _;
use std::io::IsTerminal;

use aspero_bib::BibEntry;
use aspero_engine::{BibliographyReport, CheckSummary, CheckedEntry, Engine, Severity};
use aspero_json::check::keys;

use super::{Failure, Out, print, print_line, read_stdin};
use crate::args::{FailOn, Format};
use crate::exit::Exit;
use crate::grid::{self, Col, Grid, Text};
use crate::output::{self, Level};
use crate::term::{self, Paint, Progress};

pub async fn run(
    engine: &Engine,
    path: String,
    fail_on: FailOn,
    limit: u32,
    out: Out,
) -> Result<Exit, Failure> {
    let source = if path == "-" {
        if std::io::stdin().is_terminal() {
            return Err(Failure::usage(
                "no file given (pass one, or pipe a .bib file)",
            ));
        }
        read_stdin()?.into_bytes()
    } else {
        std::fs::read(&path).map_err(|e| Failure::usage(format!("could not read {path}: {e}")))?
    };
    let entries = entries(&path, &source)?;
    if entries.is_empty() {
        return Err(Failure::usage(format!("{path}: no entries")));
    }

    let total = entries.iter().filter(|e| e.is_checkable()).count();
    let mut progress = Progress::new("entries", if out.format.is_human() { total } else { 0 });
    progress.show(0);
    let mut done = 0;
    let report = engine
        .check_entries(&entries, limit, |_| {
            done += 1;
            progress.show(done);
        })
        .await;
    progress.clear();

    let failed = report.entries.iter().any(|e| fails(e.severity(), fail_on));
    let keys = keys(&entries);
    let checked = || entries.iter().zip(&report.entries);
    match out.format {
        Format::Table => {
            let mut grid = Grid::new([
                Col::content("Status"),
                Col::cap("Key", 24),
                Col::content("Verdict"),
                Col::fill("Findings"),
            ]);
            for (entry, c) in checked() {
                grid.row(row(entry, c, &keys));
            }
            println!("{grid}");
            println!("{}", term::fill(&summary_text(&path, &report.summary())));
            output::eprint_usage(&report.usage);
        }
        Format::Text => {
            print!("{}", term::fill(&text(&path, &entries, &report)));
            output::eprint_usage(&report.usage);
        }
        Format::Jsonl => {
            for (entry, c) in checked() {
                print_line(&aspero_json::check::entry(entry, c, &keys, out.full));
            }
        }
        Format::Bibtex | Format::Ris | Format::CslJson | Format::Cite => {
            unreachable!("rejected in main")
        }
        Format::Json | Format::Auto => print(&aspero_json::check::report(
            &path, &entries, &report, out.full,
        )),
    }
    Ok(if failed { Exit::Unverified } else { Exit::Ok })
}

/// A bibliography file's entries, or a Word document's citations (a zip
/// file, as .docx files are).
pub fn entries(path: &str, source: &[u8]) -> Result<Vec<BibEntry>, Failure> {
    if source.starts_with(b"PK") {
        let read = aspero_bib::docx::read_docx(source)
            .map_err(|_| Failure::usage(format!("{path}: not a .docx file")))?;
        if read.entries.is_empty() {
            return Err(Failure::usage(format!(
                "{path}: no Zotero or Mendeley citations; to check its reference list, \
                 paste it into `aspero verify --split -`"
            )));
        }
        return Ok(read.entries);
    }
    let text =
        std::str::from_utf8(source).map_err(|_| Failure::usage(format!("{path}: not text")))?;
    let (_, entries) =
        aspero_bib::read(text).map_err(|e| Failure::usage(format!("{path}: {e}")))?;
    Ok(entries)
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

/// The verdict as shown: `skipped` when there was nothing to check.
fn verdict(checked: &CheckedEntry) -> String {
    checked
        .verdict()
        .map_or("skipped", |v| v.as_str())
        .replace('_', " ")
}

/// How much attention an entry needs: problems in capitals, to stand out
/// without colour.
fn status(checked: &CheckedEntry) -> (&'static str, Level) {
    match checked.severity() {
        None => ("ok", Level::Good),
        Some(Severity::Info) => ("info", Level::Info),
        Some(Severity::Warning) => ("WARNING", Level::Warning),
        Some(_) => ("ERROR", Level::Error),
    }
}

/// Status, key, verdict, and the findings, each marked when there are
/// several (a long one wraps).
fn row(entry: &BibEntry, checked: &CheckedEntry, keys: &[&str]) -> Vec<Text> {
    let (status, level) = status(checked);
    let findings: Vec<String> = checked.findings.iter().map(|f| f.message(keys)).collect();
    let findings = match findings.as_slice() {
        [one] => one.clone(),
        many => many
            .iter()
            .map(|f| format!("• {f}"))
            .collect::<Vec<_>>()
            .join("\n"),
    };
    vec![
        grid::status(status, level),
        Text::from(&entry.key),
        Text::from(verdict(checked)),
        Text::from(findings),
    ]
}

/// The whole check as text: each entry, then the summary.
fn text(path: &str, entries: &[BibEntry], report: &BibliographyReport) -> String {
    let keys = keys(entries);
    let entries: String = entries
        .iter()
        .zip(&report.entries)
        .map(|(entry, c)| entry_text(entry, c, &keys))
        .collect();
    format!("{entries}\n{}\n", summary_text(path, &report.summary()))
}

fn entry_text(entry: &BibEntry, checked: &CheckedEntry, keys: &[&str]) -> String {
    let (status, level) = status(checked);
    // Every status in capitals, but a quiet ok.
    let status = match checked.severity() {
        None => status.to_owned(),
        Some(_) => status.to_uppercase(),
    };
    let status = term::paint(&format!("{status:<8}"), Paint::Strong(level));
    if checked.severity().is_none() {
        return format!("{status} {}\n", entry.key);
    }
    let mut out = format!("{status} {}  ({})\n", entry.key, verdict(checked));
    for finding in &checked.findings {
        let _ = writeln!(out, "         - {}", finding.message(keys));
    }
    out
}

/// "refs.bib: 8 entries, 4 verified; 4 with errors, 1 skipped": counts
/// that are zero left out.
fn summary_text(path: &str, s: &CheckSummary) -> String {
    let entries = match s.entries {
        1 => "1 entry".to_owned(),
        n => format!("{n} entries"),
    };
    let problems: Vec<String> = [
        (s.errors, "with errors"),
        (s.warnings, "with warnings"),
        (s.suggestions, "with suggestions"),
        (s.skipped, "skipped"),
    ]
    .into_iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, what)| format!("{n} {what}"))
    .collect();
    let problems = if problems.is_empty() {
        String::new()
    } else {
        format!("; {}", problems.join(", "))
    };
    format!("{path}: {entries}, {} verified{problems}", s.verified)
}
#[cfg(test)]
mod tests {
    use aspero_core::{Update, UpdateKind};

    use super::*;
    use crate::cmd::testing::{Scripted, engine, pinned, work};

    /// What `check` prints, byte for byte: JSON (agents' contract), JSON
    /// Lines with `--full`, and text.
    #[tokio::test]
    async fn output_is_pinned() {
        let lecun = ["Yann LeCun", "Yoshua Bengio", "Geoffrey Hinton"];
        let mut retracted = work(
            &["10.1234/retracted"],
            "A retracted study",
            &["Jane Doe"],
            2019,
        );
        let mut retraction = Update::new(UpdateKind::Retraction);
        retraction.notice = Some("10.1234/notice".parse().unwrap());
        retracted.work.updates = vec![retraction];
        let works = vec![
            work(&["10.1038/nature14539"], "Deep learning", &lecun, 2015),
            work(&["10.1145/3448250"], "Deep learning for AI", &lecun, 2021),
            work(
                &[
                    "W2626778328",
                    "arXiv:1706.03762",
                    "10.48550/arXiv.1706.03762",
                ],
                "Attention Is All You Need",
                &["Ashish Vaswani", "Noam Shazeer", "Niki Parmar"],
                2017,
            ),
            retracted,
        ];
        let engine = engine(vec![Scripted::new("openalex", works)]);

        let path = "refs.bib";
        let entries = entries(path, include_bytes!("../../tests/check/refs.bib")).unwrap();
        let report = engine.check_entries(&entries, 5, |_| {}).await;

        let json = aspero_json::check::report(path, &entries, &report, false);
        pinned(
            "check/expected.json",
            &(serde_json::to_string_pretty(&json).unwrap() + "\n"),
        );
        let keys = keys(&entries);
        let mut lines = String::new();
        for (e, c) in entries.iter().zip(&report.entries) {
            let _ = writeln!(lines, "{}", aspero_json::check::entry(e, c, &keys, true));
        }
        pinned("check/expected.jsonl", &lines);
        pinned("check/expected.txt", &text(path, &entries, &report));
    }
}
