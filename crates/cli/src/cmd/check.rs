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
fn entries(path: &str, source: &[u8]) -> Result<Vec<BibEntry>, Failure> {
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
    use std::sync::Arc;

    use aspero_core::{
        Capabilities, FieldSet, Hit, IdKind, Lookup, Page, PartialDate, Person, Provider,
        ProviderId, Raw, ResiliencePolicy, Response, Result, SearchQuery, SourceRecord, Update,
        UpdateKind, Usage, Work, WorkId,
    };
    use async_trait::async_trait;
    use jiff::Timestamp;

    use super::*;

    /// Lookups by identifier and searches by exact title over a few works,
    /// each call at a cost. Costs are binary fractions, so totals don't
    /// depend on the order calls finish in.
    struct Scripted {
        caps: Capabilities,
        works: Vec<SourceRecord>,
    }

    fn costing<T>(value: T, cost: f64) -> Response<T> {
        let mut usage = Usage::default();
        usage.cost = Some(cost);
        let mut response = Response::new(value);
        response.usage = Some(usage);
        response
    }

    #[async_trait]
    impl Provider for Scripted {
        fn id(&self) -> ProviderId {
            ProviderId::from_static("openalex")
        }

        fn capabilities(&self) -> &Capabilities {
            &self.caps
        }

        async fn lookup(&self, ids: &[WorkId], _: FieldSet) -> Result<Response<Vec<Lookup>>> {
            let lookups = ids
                .iter()
                .map(|id| Lookup {
                    id: id.clone(),
                    record: self.works.iter().find(|w| w.ids.contains(id)).cloned(),
                })
                .collect();
            Ok(costing(lookups, 0.0625))
        }

        async fn search(&self, query: &SearchQuery) -> Result<Response<Page>> {
            let title = |w: &SourceRecord| w.work.title.as_deref().map(str::to_lowercase);
            let hits = self
                .works
                .iter()
                .filter(|w| title(w) == Some(query.text.to_lowercase()))
                .map(|w| Hit::new(w.clone(), Some(100.0)))
                .collect();
            Ok(costing(Page::new(hits, None, None), 0.5))
        }
    }

    fn work(ids: &[&str], title: &str, authors: &[&str], year: i16) -> SourceRecord {
        let mut w = Work::default();
        w.title = Some(title.into());
        w.authors = authors.iter().map(|a| Person::literal(*a)).collect();
        w.issued = Some(PartialDate::from_year(year));
        SourceRecord::new(
            ProviderId::from_static("openalex"),
            ids.iter().map(|i| i.parse().unwrap()).collect(),
            w,
            FieldSet::ALL,
            Raw {
                media_type: "application/json".into(),
                body: "{}".into(),
            },
            Timestamp::UNIX_EPOCH,
        )
    }

    fn engine() -> Engine {
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
        let mut caps = Capabilities::default();
        caps.lookup_kinds = vec![IdKind::Doi, IdKind::Arxiv];
        caps.max_batch = 10;
        caps.fields = FieldSet::ALL;
        caps.search = true;
        Engine::builder()
            .provider(
                Arc::new(Scripted { caps, works }),
                ResiliencePolicy::default(),
                10,
            )
            .build()
    }

    /// What `check` prints, byte for byte: JSON (agents' contract), JSON
    /// Lines with `--full`, and text. `ASPERO_BLESS=1` rewrites the
    /// expected files after an intended change.
    #[tokio::test]
    async fn output_is_pinned() {
        let path = "refs.bib";
        let entries = entries(path, include_bytes!("../../tests/check/refs.bib")).unwrap();
        let report = engine().check_entries(&entries, 5, |_| {}).await;

        let json = aspero_json::check::report(path, &entries, &report, false);
        let json = serde_json::to_string_pretty(&json).unwrap() + "\n";
        let keys = keys(&entries);
        let lines: String = entries
            .iter()
            .zip(&report.entries)
            .map(|(e, c)| format!("{}\n", aspero_json::check::entry(e, c, &keys, true)))
            .collect();
        let text = text(path, &entries, &report);
        for (name, actual) in [
            ("expected.json", json),
            ("expected.jsonl", lines),
            ("expected.txt", text),
        ] {
            let file = format!("{}/tests/check/{name}", env!("CARGO_MANIFEST_DIR"));
            if std::env::var_os("ASPERO_BLESS").is_some() {
                std::fs::write(&file, &actual).unwrap();
            }
            let expected = std::fs::read_to_string(&file).unwrap();
            assert_eq!(actual, expected, "{name} changed");
        }
    }
}
