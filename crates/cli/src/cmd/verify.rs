//! `aspero verify`: does a citation refer to a real work, with matching
//! details?

use std::io::IsTerminal;

use aspero_core::{ProviderId, Usage};
use aspero_engine::{Engine, Verdict, Verification};
use aspero_json::{ClaimInput, InputClaim};
use futures::StreamExt;
use serde_json::{Value, json};

use super::claim::claims;
use super::{Failure, Out, print, print_line};
use crate::args::{ClaimArgs, Format};
use crate::exit::Exit;
use crate::output::{self, Level};

pub async fn run(engine: &Engine, args: ClaimArgs, out: Out) -> Result<Exit, Failure> {
    let limit = args.limit;
    let split_list = args.split;
    let (text, fields) = split(args);
    let claims = claims(text, fields, split_list)?;
    if claims.is_empty() {
        return Err(Failure::usage("no claims given"));
    }

    let (positions, valid) = InputClaim::valid(&claims);
    let total = claims.len();
    let show_progress = out.format == Format::Table && total > 1 && std::io::stderr().is_terminal();
    let mut results = Results::new(out.format, total, show_progress);
    let mut export = output::Exporter::new(&out)?;
    for (i, claim) in claims.iter().enumerate() {
        if let Err(e) = &claim.claim {
            let item = aspero_json::invalid_claim(claim.echo(), e);
            let text = format!("INVALID  {}\n  {e}\n", claim.input);
            results.add(
                i,
                Done {
                    item,
                    text,
                    verdict: None,
                },
            );
        }
    }
    let mut verified = engine.verify_batch(&valid, limit);
    while let Some((k, v)) = verified.next().await {
        let i = positions[k];
        let claim = &claims[i];
        let mut item = aspero_json::verification(claim.echo(), &v, out.full);
        if out.format.is_export() {
            // The work the citation refers to, when that's known: verified,
            // or found under other identifiers than those cited.
            let best = v.best.map(|i| &v.report.candidates[i]);
            let text = match best {
                Some(c) if matches!(v.verdict, Verdict::Verified | Verdict::Mismatch) => {
                    export.entry(&c.record.work, &c.ids)
                }
                _ => {
                    eprintln!("skipped {}: {}", claim.input, v.reason);
                    String::new()
                }
            };
            results.add(
                i,
                Done {
                    item,
                    text,
                    verdict: Some(v.verdict),
                },
            );
            continue;
        }
        let mut text = verdict_text(&claim.input, &v);
        if out.abstracts
            && let Some(best) = v.best.map(|i| &v.report.candidates[i])
        {
            // Search under every identifier merged into the match.
            let fetched = engine.abstracts(&[&best.merged_record()]).await;
            verified.add_usage(&fetched.usage);
            let found = fetched.found.into_iter().next().flatten();
            aspero_json::set_abstract(&mut item["match"]["work"], found.as_ref());
            text.push_str(&output::abstract_text(found.as_ref(), "  "));
            text.push('\n');
        }
        results.add(
            i,
            Done {
                item,
                text,
                verdict: Some(v.verdict),
            },
        );
    }
    let exit = results.finish(verified.usage());
    print!("{}", export.finish());
    exit
}

/// One claim's result: its JSON, its text, and its verdict (`None` when
/// the claim couldn't be read).
struct Done {
    item: Value,
    text: String,
    verdict: Option<Verdict>,
}

/// Results as claims complete: JSON lines at once, in any order (each
/// echoes its input and key); text in claim order as soon as each is
/// next; tables and documents at the end.
struct Results {
    format: Format,
    slots: Vec<Option<Done>>,
    /// The next claim to print as text.
    next: usize,
    done: usize,
    show_progress: bool,
    exit: Exit,
}

impl Results {
    fn new(format: Format, total: usize, show_progress: bool) -> Self {
        Results {
            format,
            slots: (0..total).map(|_| None).collect(),
            next: 0,
            done: 0,
            show_progress,
            exit: Exit::Ok,
        }
    }

    fn add(&mut self, i: usize, done: Done) {
        self.exit = self.exit.worst(done.verdict.map_or(Exit::Usage, exit_for));
        self.done += 1;
        match self.format {
            Format::Jsonl => print_line(&done.item),
            Format::Text | Format::Bibtex | Format::Ris | Format::Cite => {
                self.slots[i] = Some(done);
                while let Some(Some(done)) = self.slots.get(self.next) {
                    if !done.text.is_empty() {
                        println!("{}", done.text);
                    }
                    self.next += 1;
                }
            }
            _ => {
                self.slots[i] = Some(done);
                if self.show_progress {
                    eprint!("\rverified {}/{}…", self.done, self.slots.len());
                }
            }
        }
    }

    fn finish(self, usage: &[(ProviderId, Usage)]) -> Result<Exit, Failure> {
        if self.show_progress {
            eprint!("\r\x1b[K");
        }
        let done = self.slots.into_iter().flatten();
        match self.format {
            Format::Jsonl | Format::Bibtex | Format::Ris | Format::Cite => {}
            Format::Text => {
                if let Some(text) = output::usage_text(usage) {
                    eprintln!("{text}");
                }
            }
            Format::Table => {
                let done: Vec<Done> = done.collect();
                // One claim is detail, not rows: show it as text.
                if let [one] = &done[..] {
                    println!("{}", one.text);
                } else {
                    let mut table = output::table(&["Verdict", "Conf", "Citation", "Reason"]);
                    for d in &done {
                        table.add_row(row(&d.item, d.verdict));
                    }
                    println!("{table}");
                }
                if let Some(text) = output::usage_text(usage) {
                    eprintln!("{text}");
                }
            }
            Format::Json | Format::Auto => {
                let items: Vec<Value> = done.map(|d| d.item).collect();
                print(&json!({"results": items, "usage": output::usage(usage)}));
            }
        }
        Ok(self.exit)
    }
}

/// Positional text and the field flags.
pub fn split(args: ClaimArgs) -> (Option<String>, ClaimInput) {
    let fields = ClaimInput {
        title: args.title,
        authors: args.authors,
        year: args.year,
        container: args.journal,
        volume: args.volume,
        page: args.page,
        ids: args.ids,
        ..ClaimInput::default()
    };
    (args.text, fields)
}

/// One verification as a table row: verdict, confidence, citation, reason.
fn row(item: &Value, verdict: Option<Verdict>) -> Vec<comfy_table::Cell> {
    use comfy_table::Cell;
    let level = match verdict {
        Some(Verdict::Verified) => Level::Good,
        Some(Verdict::Plausible | Verdict::Unverifiable) => Level::Warning,
        _ => Level::Error,
    };
    let verdict = item["verdict"].as_str().unwrap_or("?");
    let confidence = item["match"]["confidence"]
        .as_f64()
        .map(|c| format!("{c:.2}"))
        .unwrap_or_default();
    vec![
        output::status_cell(&verdict.replace('_', " ").to_uppercase(), level),
        Cell::new(confidence),
        Cell::new(item["input"].as_str().unwrap_or("")),
        Cell::new(item["reason"].as_str().unwrap_or("")),
    ]
}

pub(super) fn exit_for(verdict: Verdict) -> Exit {
    match verdict {
        Verdict::Verified => Exit::Ok,
        Verdict::NotFound => Exit::NotFound,
        Verdict::Unverifiable => Exit::Unavailable,
        _ => Exit::Unverified,
    }
}

pub(super) fn verdict_text(input: &str, v: &Verification) -> String {
    let mut lines = vec![
        format!(
            "{}  {input}",
            v.verdict.as_str().to_uppercase().replace('_', " ")
        ),
        format!("  {}", v.reason),
    ];
    if let Some(best) = v.best {
        lines.push(output::candidate_text(&v.report.candidates[best], "  "));
    }
    for e in &v.report.errors {
        lines.push(format!("  warning: {e}"));
    }
    lines.join("\n") + "\n"
}
