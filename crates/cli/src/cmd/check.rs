//! `aspero check`: verify every entry of a .bib file and report fixes.

use std::io::IsTerminal;

use aspero_bib::BibEntry;
use aspero_engine::{BibliographyReport, CheckSummary, CheckedEntry, Engine, Severity};
use aspero_json::check::keys;

use super::{Failure, Out, print, print_line, read_stdin};
use crate::args::{FailOn, Format};
use crate::exit::Exit;
use crate::output::{self, Level};

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

    let show_progress = out.format.is_human() && std::io::stderr().is_terminal();
    let total = entries.iter().filter(|e| e.is_checkable()).count();
    let mut done = 0;
    let report = engine
        .check_entries(&entries, limit, |_| {
            done += 1;
            if show_progress {
                eprint!("\rchecking {done}/{total}…");
            }
        })
        .await;
    if show_progress {
        eprint!("\r\x1b[K");
    }

    let failed = report.entries.iter().any(|e| fails(e.severity(), fail_on));
    let keys = keys(&entries);
    let checked = || entries.iter().zip(&report.entries);
    match out.format {
        Format::Table => {
            let mut table = output::table(&["Status", "Key", "Verdict", "Findings"]);
            for (entry, c) in checked() {
                table.add_row(row(entry, c, &keys));
            }
            println!("{table}\n{}", summary_text(&path, &report.summary()));
            if let Some(text) = output::usage_text(&report.usage) {
                eprintln!("{text}");
            }
        }
        Format::Text => {
            print!("{}", text(&path, &entries, &report));
            if let Some(text) = output::usage_text(&report.usage) {
                eprintln!("{text}");
            }
        }
        Format::Jsonl => {
            for (entry, c) in checked() {
                print_line(&aspero_json::check::entry(entry, c, &keys, out.full));
            }
        }
        Format::Bibtex | Format::Ris | Format::Cite => unreachable!("rejected in main"),
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

/// Status, key, verdict, and one finding per line.
fn row(entry: &BibEntry, checked: &CheckedEntry, keys: &[&str]) -> Vec<comfy_table::Cell> {
    let status = match checked.severity() {
        None => output::status_cell("ok", Level::Good),
        Some(Severity::Info) => output::status_cell("info", Level::Info),
        Some(Severity::Warning) => output::status_cell("WARNING", Level::Warning),
        Some(_) => output::status_cell("ERROR", Level::Error),
    };
    let findings: Vec<String> = checked.findings.iter().map(|f| f.message(keys)).collect();
    vec![
        status,
        comfy_table::Cell::new(&entry.key),
        comfy_table::Cell::new(verdict(checked)),
        comfy_table::Cell::new(findings.join("\n")),
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
    let status = match checked.severity() {
        None => return format!("ok       {}\n", entry.key),
        Some(s) => s.as_str().to_uppercase(),
    };
    let mut out = format!("{status:<8} {}  ({})\n", entry.key, verdict(checked));
    for finding in &checked.findings {
        out.push_str(&format!("         - {}\n", finding.message(keys)));
    }
    out
}

fn summary_text(path: &str, s: &CheckSummary) -> String {
    format!(
        "{path}: {} entries, {} verified; {} with errors, {} with warnings, {} with suggestions, {} skipped",
        s.entries, s.verified, s.errors, s.warnings, s.suggestions, s.skipped
    )
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
        let lines: String = entries
            .iter()
            .zip(&report.entries)
            .map(|(e, c)| format!("{}\n", aspero_json::check::entry(e, c, &keys, true)))
            .collect();
        pinned("check/expected.jsonl", &lines);
        pinned("check/expected.txt", &text(path, &entries, &report));
    }
}
