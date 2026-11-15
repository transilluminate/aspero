//! `aspero verify`: does a citation refer to a real work, with matching
//! details?

use aspero_core::{PageSource, ProviderId, Usage};
use aspero_engine::{
    Candidate, Difference, Engine, Reason, Verdict, Verification, describe_update,
};
use aspero_json::{ClaimInput, InputClaim};
use futures::StreamExt;
use serde_json::{Value, json};

use super::claim::claims;
use super::{Failure, Out, print, print_line, read_stdin};
use crate::args::{ClaimArgs, Format, PageArgs};
use crate::exit::Exit;
use crate::grid::{self, Col, Grid, Text};
use crate::output::{self, Level};
use crate::term::{self, Paint, Progress};

pub async fn run(
    engine: &Engine,
    args: ClaimArgs,
    page: PageArgs,
    out: Out,
) -> Result<Exit, Failure> {
    let limit = args.limit;
    let (claims, page_source) = if let Some(url) = &page.url {
        let html = page.html.as_deref().map(read_page).transpose()?;
        let (claim, source) = from_page(html.as_deref(), url, args.title.as_deref())?;
        (vec![claim], Some(source))
    } else {
        let split_list = args.split;
        let (text, fields) = split(args);
        (claims(text, fields, split_list)?, None)
    };
    if claims.is_empty() {
        return Err(Failure::usage("no claims given"));
    }

    let (positions, valid) = InputClaim::valid(&claims);
    let total = claims.len();
    let mut results = Results::new(out.format, total);
    let mut export = output::Exporter::new(&out)?;
    for (i, claim) in claims.iter().enumerate() {
        if let Err(e) = &claim.claim {
            let item = aspero_json::invalid_claim(claim.echo(), e);
            let invalid = term::paint("INVALID", Paint::Strong(Level::Error));
            let text = format!("{invalid}  {}\n  {e}\n", claim.input);
            let row = vec![
                grid::status("INVALID", Level::Error),
                Text::default(),
                Text::from(&claim.input),
                Text::from(e.as_str()),
            ];
            results.add(
                i,
                Done {
                    item,
                    text,
                    row,
                    verdict: None,
                },
            );
        }
    }
    let mut verified = engine.verify_batch(&valid, limit);
    while let Some((k, v)) = verified.next().await {
        let i = positions[k];
        let claim = &claims[i];
        let mut item = aspero_json::verification(echo(claim, page_source), &v, out.full);
        if out.format.is_export() {
            // The work the citation refers to, when that's known: verified,
            // or found under other identifiers than those cited.
            let best = v.best_match.map(|i| &v.report.candidates[i]);
            let text = match best {
                Some(c) if matches!(v.verdict, Verdict::Verified | Verdict::Mismatch) => {
                    export.entry_at(i, &c.record.work, &c.ids)
                }
                _ => {
                    eprintln!("skipped {}: {}", claim.input, v.reason_text);
                    String::new()
                }
            };
            results.add(
                i,
                Done {
                    item,
                    text,
                    row: Vec::new(),
                    verdict: Some(v.verdict),
                },
            );
            continue;
        }
        let mut text = verdict_text(&claim.input, &v);
        if let Some(source) = page_source {
            text = read_from(&text, source);
        }
        if out.abstracts
            && let Some(best) = v.best_match.map(|i| &v.report.candidates[i])
        {
            // Search under every identifier merged into the match.
            let fetched = engine.abstracts(&[&best.merged_record()]).await;
            verified.add_usage(&fetched.usage);
            let found = fetched.found.into_iter().next().flatten();
            aspero_json::set_abstract(&mut item["best_match"]["work"], found.as_ref());
            text.push_str(&output::abstract_text(found.as_ref(), "  "));
            text.push('\n');
        }
        let row = row(&claim.input, &v);
        results.add(
            i,
            Done {
                item,
                text,
                row,
                verdict: Some(v.verdict),
            },
        );
    }
    let exit = results.finish(verified.usage());
    print!("{}", export.finish());
    Ok(exit)
}

/// An article page's markup, from a file or (`-`) stdin. Pages saved in
/// another encoding read with what isn't UTF-8 replaced: the metadata
/// that matters is mostly ASCII.
fn read_page(path: &str) -> Result<String, Failure> {
    if path == "-" {
        return read_stdin();
    }
    std::fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .map_err(|e| Failure::usage(format!("could not read {path}: {e}")))
}

/// The claim an article page makes (see `aspero_core::webpage`), with
/// its address as the input, and what answered.
pub(super) fn from_page(
    html: Option<&str>,
    url: &str,
    title: Option<&str>,
) -> Result<(InputClaim, PageSource), Failure> {
    let page = aspero_core::claim_from_page(html, url, title);
    let Some(claim) = page.claim else {
        return Err(Failure {
            exit: Exit::NotFound,
            message: format!("{url}: nothing on the page or in its address says which work it is"),
        });
    };
    let claim = InputClaim {
        input: url.to_owned(),
        key: None,
        claim: Ok(claim),
    };
    Ok((claim, page.source))
}

/// A claim's echo, saying what answered when it was read from a page.
fn echo(claim: &InputClaim, page_source: Option<PageSource>) -> Value {
    let mut echo = claim.echo();
    if let Some(source) = page_source {
        echo["page_source"] = json!(source);
    }
    echo
}

/// `text` with where its claim was read from after its first line.
fn read_from(text: &str, source: PageSource) -> String {
    let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
    let note = term::paint(&format!("read from {}", source.describe()), Paint::Dim);
    format!("{first}\n  {note}\n{rest}")
}

/// One claim's result: its JSON, its text, its table row, and its verdict
/// (`None` when the claim couldn't be read).
struct Done {
    item: Value,
    text: String,
    row: Vec<Text>,
    verdict: Option<Verdict>,
}

/// Results as claims complete: JSON lines at once, in any order (each
/// echoes its input and key); text in claim order as soon as each is
/// next; tables and documents at the end. On a terminal, how many are
/// done shows on stderr meanwhile.
struct Results {
    format: Format,
    slots: Vec<Option<Done>>,
    /// The next claim to print as text.
    next: usize,
    done: usize,
    progress: Progress,
    exit: Exit,
}

impl Results {
    fn new(format: Format, total: usize) -> Self {
        let mut progress = Progress::new("citations", if format.is_human() { total } else { 0 });
        progress.show(0);
        Results {
            format,
            slots: (0..total).map(|_| None).collect(),
            next: 0,
            done: 0,
            progress,
            exit: Exit::Ok,
        }
    }

    fn add(&mut self, i: usize, done: Done) {
        self.exit = self.exit.worst(done.verdict.map_or(Exit::Usage, exit_for));
        self.done += 1;
        match self.format {
            Format::Jsonl => print_line(&done.item),
            Format::Text | Format::Bibtex | Format::Ris | Format::CslJson | Format::Cite => {
                self.slots[i] = Some(done);
                while let Some(Some(done)) = self.slots.get(self.next) {
                    if !done.text.is_empty() {
                        self.progress.clear();
                        if self.format == Format::Text {
                            println!("{}", term::fill(&done.text));
                        } else {
                            println!("{}", done.text);
                        }
                    }
                    self.next += 1;
                }
                self.progress.show(self.done);
            }
            _ => {
                self.slots[i] = Some(done);
                self.progress.show(self.done);
            }
        }
    }

    fn finish(mut self, usage: &[(ProviderId, Usage)]) -> Exit {
        self.progress.clear();
        let done = self.slots.into_iter().flatten();
        match self.format {
            Format::Jsonl | Format::Bibtex | Format::Ris | Format::CslJson | Format::Cite => {}
            Format::Text => output::eprint_usage(usage),
            Format::Table => {
                let done: Vec<Done> = done.collect();
                // One claim is detail, not rows: show it as text.
                if let [one] = &done[..] {
                    println!("{}", term::fill(&one.text));
                } else {
                    let mut grid = Grid::new(cols());
                    for d in done {
                        grid.row(d.row);
                    }
                    println!("{grid}");
                }
                output::eprint_usage(usage);
            }
            Format::Json | Format::Auto => {
                let items: Vec<Value> = done.map(|d| d.item).collect();
                print(&json!({"results": items, "usage": output::usage(usage)}));
            }
        }
        self.exit
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
        site: args.site,
        ..ClaimInput::default()
    };
    (args.text, fields)
}

fn level(verdict: Verdict) -> Level {
    match verdict {
        Verdict::Verified => Level::Good,
        Verdict::Plausible | Verdict::Unverifiable => Level::Warning,
        _ => Level::Error,
    }
}

/// "VERIFIED", "NOT FOUND".
fn verdict_word(verdict: Verdict) -> String {
    verdict.as_str().replace('_', " ").to_uppercase()
}

/// The columns of [`row`].
fn cols() -> [Col; 4] {
    [
        Col::content("Verdict"),
        Col::content("Conf"),
        Col::fill("Citation"),
        Col::fill("Match"),
    ]
}

/// One verification as a table row: verdict, confidence, citation, and
/// what's wrong (as in the text) above the work matched.
fn row(input: &str, v: &Verification) -> Vec<Text> {
    let best = v.best_match.map(|i| &v.report.candidates[i]);
    let confidence = best.map_or(String::new(), |c| format!("{:.2}", c.assessment.confidence));
    let mut found: Vec<String> = why(v).into_iter().collect();
    if let Some(c) = best {
        found.push(described(c));
        found.push(output::main_id(&c.ids));
    }
    found.extend(v.notes.iter().map(|n| format!("⚠ {}", describe_update(n))));
    vec![
        grid::status(verdict_word(v.verdict), level(v.verdict)),
        Text::from(confidence),
        Text::from(input),
        Text::from(found.join("\n")),
    ]
}

/// `"Deep learning" (2015)`.
fn described(c: &Candidate) -> String {
    let w = &c.record.work;
    let title = w.title.as_deref().unwrap_or("(untitled)");
    match w.issued {
        Some(d) => format!("\"{title}\" ({})", d.year()),
        None => format!("\"{title}\""),
    }
}

/// "its year (2017) differs", "its authors and year (2017) differ".
fn clause(differs: &[Difference]) -> Option<String> {
    let fields: Vec<String> = differs.iter().map(ToString::to_string).collect();
    match fields.len() {
        0 => None,
        1 => Some(format!("its {} differs", fields[0])),
        _ => Some(format!("its {} differ", fields.join(" and "))),
    }
}

/// Why the verdict, worded not to repeat the work shown with it: nothing
/// for a plain match, what differs for a partial one, and what became of
/// the identifiers cited.
fn why(v: &Verification) -> Option<String> {
    let claimed = v
        .read
        .ids
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let differs = || clause(v.reason.differs());
    Some(match &v.reason {
        Reason::Matches | Reason::IdExists | Reason::IdAgrees => return None,
        Reason::PartlyMatches { .. } => differs().unwrap_or_else(|| "check the details".into()),
        Reason::Unconfirmed => {
            "agrees as far as it goes, but gives too little to confirm it".into()
        }
        Reason::Closest { .. } => format!(
            "the closest work, but {}",
            differs().unwrap_or_else(|| "its details differ".into())
        ),
        Reason::NoMatch => "no matching work found".into(),
        Reason::IdPartlyMatches { .. } => match differs() {
            Some(d) => format!("{claimed} exists, but {d}"),
            None => format!("{claimed} exists and partly matches"),
        },
        Reason::IdElsewhere { cited } => format!(
            "{claimed} is another work, {}; the citation matches this one",
            described(&v.report.candidates[*cited])
        ),
        Reason::IdDisagrees => format!("{claimed} is this work, which doesn't match the citation"),
        Reason::IdMissingElsewhere => {
            format!("{claimed} doesn't exist; the citation matches this work")
        }
        Reason::IdMissing => format!("{claimed} doesn't exist, and no matching work was found"),
        _ => v.reason_text.clone(),
    })
}

pub(super) fn exit_for(verdict: Verdict) -> Exit {
    match verdict {
        Verdict::Verified => Exit::Ok,
        Verdict::NotFound => Exit::NotFound,
        Verdict::Unverifiable => Exit::Unavailable,
        _ => Exit::Unverified,
    }
}

/// The verdict and the citation; why, unless it simply matches; then the
/// work matched: checks, details, and how it was found.
pub(super) fn verdict_text(input: &str, v: &Verification) -> String {
    let verdict = term::paint(&verdict_word(v.verdict), Paint::Strong(level(v.verdict)));
    let mut lines = vec![format!("{verdict}  {input}")];
    if let Some(why) = why(v) {
        lines.push(format!("  {why}"));
    }
    if let Some(best) = v.best_match {
        lines.push(output::candidate_text(&v.report.candidates[best], "  "));
    }
    if v.chosen_by_site {
        let site = v.read.site.as_deref().unwrap_or("the site");
        let note = format!("chosen as {site}'s publisher's, of works matching about as well");
        lines.push(format!("  {}", term::paint(&note, Paint::Dim)));
    }
    for e in &v.report.errors {
        let warning = term::paint("warning:", Paint::Warning);
        lines.push(format!("  {warning} {e}"));
    }
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::testing::{Scripted, engine, pinned, work};

    /// `verify --html jama.html --url …`: the page's DOI is claimed, and
    /// the JSON says the Highwire tags answered.
    #[tokio::test]
    async fn a_page_verifies_as_its_doi() {
        let html = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../core/tests/fixtures/pages/jama.html"
        ))
        .unwrap();
        let url = "https://jamanetwork.com/journals/jama/article-abstract/2856706";
        let (claim, source) = from_page(Some(&html), url, None).unwrap();
        assert_eq!(source, PageSource::Highwire);
        let c = claim.claim.as_ref().unwrap();
        let ids: Vec<String> = c.ids.iter().map(ToString::to_string).collect();
        assert_eq!(ids, ["doi:10.1001/jama.2026.19482"]);

        let title = c.title.clone().unwrap();
        let engine = engine(vec![Scripted::new(
            "openalex",
            vec![work(
                &["10.1001/jama.2026.19482"],
                &title,
                &["Gregory W. Albers", "Jeremy Josef Heit"],
                2026,
            )],
        )]);
        let v = engine.verify(c, 5).await;
        assert_eq!(v.verdict, Verdict::Verified, "{}", v.reason_text);
        let item = aspero_json::verification(echo(&claim, Some(source)), &v, false);
        assert_eq!(item["page_source"], "highwire");
        assert_eq!(item["input"], url);
        assert_eq!(item["claim"]["ids"][0], "doi:10.1001/jama.2026.19482");
        assert!(
            read_from(&verdict_text(url, &v), source)
                .lines()
                .nth(1)
                .unwrap()
                .contains("read from the page's Highwire tags")
        );

        // A page that says nothing, from an address that holds nothing.
        let none = from_page(Some("<title>Just a moment...</title>"), url, Some("PDF"));
        assert!(matches!(
            none,
            Err(Failure {
                exit: Exit::NotFound,
                ..
            })
        ));
    }

    /// What `verify` prints, byte for byte, as text and as a table at 80
    /// columns: a match, a cited DOI that is another work's, and none.
    #[tokio::test]
    async fn text_is_pinned() {
        let lecun = ["Yann LeCun", "Yoshua Bengio", "Geoffrey Hinton"];
        let engine = engine(vec![Scripted::new(
            "openalex",
            vec![
                work(&["10.1038/nature14539"], "Deep learning", &lecun, 2015),
                work(&["10.1145/3448250"], "Deep learning for AI", &lecun, 2021),
            ],
        )]);
        let lines = [
            r#"{"title": "Deep learning", "authors": ["LeCun Y"], "year": 2015}"#,
            r#"{"title": "Deep learning", "authors": ["LeCun Y"], "year": 2015, "doi": "10.1145/3448250"}"#,
            r#"{"title": "A wholly invented study", "authors": ["Nobody A"], "year": 2020}"#,
        ];
        let mut text = String::new();
        let mut grid = Grid::new(cols());
        for line in lines {
            let claim = InputClaim::from_line(line);
            let v = engine.verify(claim.claim.as_ref().unwrap(), 5).await;
            text.push_str(&verdict_text(&claim.input, &v));
            text.push('\n');
            grid.row(row(&claim.input, &v));
        }
        text.push_str(&grid.render(Some(80), false));
        text.push('\n');
        pinned("verify/expected.txt", &text);
    }
}
