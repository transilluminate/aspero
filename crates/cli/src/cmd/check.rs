//! `aspero check`: verify every entry of a .bib file and report fixes.

use std::io::IsTerminal;

use aspero_bib::BibEntry;
use aspero_core::{ProviderId, Usage};
use aspero_engine::{Checked, Engine, Finding, Severity, Verdict, describe_update};
use serde_json::{Value, json};

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
    let (checked, usage) = check(engine, &entries, limit, |_| {
        done += 1;
        if show_progress {
            eprint!("\rchecking {done}/{total}…");
        }
    })
    .await;
    if show_progress {
        eprint!("\r\x1b[K");
    }

    let reports = reports(&entries, &checked);
    let summary = Summary::of(&reports);
    let failed = reports.iter().any(|r| fails(r.severity, fail_on));

    match out.format {
        Format::Table => {
            let mut table = output::table(&["Status", "Key", "Verdict", "Findings"]);
            for report in &reports {
                table.add_row(report.row());
            }
            println!("{table}\n{}", summary.text(&path));
            if let Some(text) = output::usage_text(&usage) {
                eprintln!("{text}");
            }
        }
        Format::Text => {
            print!("{}", text(&path, &reports, &summary));
            if let Some(text) = output::usage_text(&usage) {
                eprintln!("{text}");
            }
        }
        Format::Jsonl => reports.iter().for_each(|r| print_line(&r.json(out.full))),
        Format::Bibtex | Format::Ris | Format::Cite => unreachable!("rejected in main"),
        Format::Json | Format::Auto => {
            print(&document(&path, &reports, &summary, &usage, out.full))
        }
    }
    Ok(if failed { Exit::Unverified } else { Exit::Ok })
}

/// The whole check as one JSON document.
fn document(
    path: &str,
    reports: &[Report],
    summary: &Summary,
    usage: &[(ProviderId, Usage)],
    full: bool,
) -> Value {
    json!({
        "file": path,
        "summary": summary.json(),
        "entries": reports.iter().map(|r| r.json(full)).collect::<Vec<_>>(),
        "usage": output::usage(usage),
    })
}

/// The whole check as text: each entry, then the summary.
fn text(path: &str, reports: &[Report], summary: &Summary) -> String {
    let entries: String = reports.iter().map(Report::text).collect();
    format!("{entries}\n{}\n", summary.text(path))
}

/// Checks the checkable entries, in order, and what that cost.
async fn check(
    engine: &Engine,
    entries: &[BibEntry],
    limit: u32,
    progress: impl FnMut(usize),
) -> (Vec<Checked>, Vec<(ProviderId, Usage)>) {
    let claims: Vec<_> = entries
        .iter()
        .filter(|e| e.is_checkable())
        .map(|e| e.claim.clone())
        .collect();
    let checked = engine.check(&claims, limit, progress).await;
    let usage = checked
        .iter()
        .flat_map(|c| &c.verification.report.usage)
        .fold(Vec::new(), |mut acc, (p, u)| {
            super::merge_usage(&mut acc, p, *u);
            acc
        });
    (checked, usage)
}

/// Reports in file order, skipped entries included.
fn reports<'a>(entries: &'a [BibEntry], checked: &'a [Checked]) -> Vec<Report<'a>> {
    let keys: Vec<&str> = entries
        .iter()
        .filter(|e| e.is_checkable())
        .map(|e| e.key.as_str())
        .collect();
    let mut by_entry = checked.iter();
    entries
        .iter()
        .map(|entry| {
            let checked = entry
                .is_checkable()
                .then(|| by_entry.next().expect("one per checkable"));
            Report::new(entry, checked, &keys)
        })
        .collect()
}

/// A bibliography file's entries,or a Word document's citations (a zip
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
                message: note.to_string(),
                detail: json!({
                    "field": note.field,
                    "value": note.value,
                    "problem": note.problem.as_str(),
                }),
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

    /// Status, key, verdict, and one finding per line.
    fn row(&self) -> Vec<comfy_table::Cell> {
        let status = match self.severity {
            None => output::status_cell("ok", Level::Good),
            Some(Severity::Info) => output::status_cell("info", Level::Info),
            Some(Severity::Warning) => output::status_cell("WARNING", Level::Warning),
            Some(_) => output::status_cell("ERROR", Level::Error),
        };
        let findings: Vec<&str> = self.lines.iter().map(|l| l.message.as_str()).collect();
        vec![
            status,
            comfy_table::Cell::new(&self.entry.key),
            comfy_table::Cell::new(self.verdict().replace('_', " ")),
            comfy_table::Cell::new(findings.join("\n")),
        ]
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use aspero_core::{
        Capabilities, FieldSet, Hit, IdKind, Lookup, Page, PartialDate, Person, Provider, Raw,
        ResiliencePolicy, Response, Result, SearchQuery, SourceRecord, Update, UpdateKind, Work,
        WorkId,
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
        let (checked, usage) = check(&engine(), &entries, 5, |_| {}).await;
        let reports = reports(&entries, &checked);
        let summary = Summary::of(&reports);

        let json = document(path, &reports, &summary, &usage, false);
        let json = serde_json::to_string_pretty(&json).unwrap() + "\n";
        let lines: String = reports
            .iter()
            .map(|r| format!("{}\n", r.json(true)))
            .collect();
        let text = text(path, &reports, &summary);
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
