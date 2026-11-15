//! Rendering works, results and usage as terminal text and tables. JSON
//! shapes are the contract in `aspero-json`, re-exported here.

use aspero_core::matching::{PLAUSIBLE, VERIFIED};
use aspero_core::{
    Agreement, Hit, IdKind, ProviderId, SourceRecord, UpdateKind, Usage, Work, WorkId,
};
use aspero_engine::{Abstract, Candidate, describe_update};
pub use aspero_json::{candidate, set_abstract, usage};

use crate::grid::{self, Col, Text};
pub use crate::term::Level;
use crate::term::{self, Paint};

/// Abstract characters shown in tables; JSON has the full text.
const TABLE_ABSTRACT: usize = 600;

/// Authors shown in a work's text before "et al.".
const TABLE_AUTHORS: usize = 3;

/// The widest first-author cell in a table: "Krizhevsky et al." fits.
const TABLE_AUTHOR: usize = 20;

/// Works written as BibTeX entries or RIS records (BibTeX keys kept
/// distinct: `lecun2015deep`, `lecun2015deepa`, …), or gathered into a
/// CSL-JSON array or a reference list in a citation style.
pub struct Exporter {
    format: crate::args::Format,
    keys: aspero_bib::Keys,
    style: Option<aspero_bib::cite::Style>,
    locale: Option<String>,
    /// Gathered for the end, with their input positions.
    works: Vec<(usize, Work, Vec<WorkId>)>,
}

impl Exporter {
    pub fn new(out: &crate::cmd::Out) -> Result<Self, crate::cmd::Failure> {
        use crate::cmd::Failure;
        let style = match (out.format, &out.style) {
            (crate::args::Format::Cite, None) => {
                return Err(Failure::usage(
                    "--format cite needs --style (`aspero styles` lists them)",
                ));
            }
            (crate::args::Format::Cite, Some(s))
                if std::path::Path::new(s)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("csl")) =>
            {
                let xml = std::fs::read_to_string(s)
                    .map_err(|e| Failure::usage(format!("could not read {s}: {e}")))?;
                let style = aspero_bib::cite::Style::from_xml(&xml)
                    .map_err(|e| Failure::usage(format!("{s}: {e}")))?;
                Some(style)
            }
            (crate::args::Format::Cite, Some(id)) => Some(
                aspero_bib::cite::Style::bundled(id)
                    .map_err(|e| Failure::usage(format!("{e} (`aspero styles` lists them)")))?,
            ),
            _ => None,
        };
        Ok(Exporter {
            format: out.format,
            keys: aspero_bib::Keys::default(),
            style,
            locale: out.locale.clone(),
            works: Vec::new(),
        })
    }

    /// The work's entry to print now; empty for CSL-JSON, one array, and
    /// a style, whose list is ordered once every work is in
    /// ([`Exporter::finish`]).
    pub fn entry(&mut self, work: &Work, ids: &[WorkId]) -> String {
        self.entry_at(self.works.len(), work, ids)
    }

    /// [`Exporter::entry`] for the input at `position`, for works that
    /// arrive out of order: gathered ones are given in input order.
    pub fn entry_at(&mut self, position: usize, work: &Work, ids: &[WorkId]) -> String {
        match self.format {
            crate::args::Format::Ris => aspero_bib::ris(work, ids),
            crate::args::Format::Cite | crate::args::Format::CslJson => {
                self.works.push((position, work.clone(), ids.to_vec()));
                String::new()
            }
            _ => aspero_bib::bibtex(work, ids, &self.keys.key(work)),
        }
    }

    /// The CSL-JSON array, or a style's reference list, one entry per
    /// line; empty otherwise.
    pub fn finish(mut self) -> String {
        self.works.sort_by_key(|(position, ..)| *position);
        let works: Vec<(&Work, &[WorkId])> =
            self.works.iter().map(|(_, w, ids)| (w, &ids[..])).collect();
        if self.format == crate::args::Format::CslJson {
            return aspero_bib::csl_json(&works) + "\n";
        }
        let Some(style) = &self.style else {
            return String::new();
        };
        let cited = aspero_bib::cite::cite(
            &works,
            style,
            self.locale.as_deref(),
            aspero_bib::cite::Markup::Text,
        );
        cited
            .iter()
            .map(|c| match &c.label {
                Some(label) => format!("{label} {}\n", c.text),
                None => format!("{}\n", c.text),
            })
            .collect()
    }
}

/// A work as a few lines of text, indented by `indent`.
pub fn work_text(record: &SourceRecord, indent: &str) -> String {
    let w = &record.work;
    let mut lines = vec![format!(
        "{indent}{}",
        w.title.as_deref().unwrap_or("(untitled)")
    )];

    let mut names: Vec<String> = w
        .authors
        .iter()
        .take(TABLE_AUTHORS)
        .filter_map(aspero_core::Person::display_name)
        .collect();
    if w.authors.len() > TABLE_AUTHORS {
        names.push("et al.".into());
    }
    let details: Vec<String> = [
        (!names.is_empty()).then(|| names.join(", ")),
        w.issued.map(|d| d.year().to_string()),
        w.container_title.clone(),
        w.work_type.as_ref().map(aspero_core::WorkType::label),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !details.is_empty() {
        lines.push(format!("{indent}{}", details.join(" · ")));
    }
    let ids: Vec<String> = record.ids.iter().map(ToString::to_string).collect();
    lines.push(format!("{indent}{}", ids.join("  ")));
    for update in &w.updates {
        let paint = match update.kind {
            UpdateKind::Correction => continue,
            UpdateKind::Retraction | UpdateKind::Withdrawal => Paint::Error,
            _ => Paint::Warning,
        };
        let note = format!("⚠ {}", describe_update(update));
        lines.push(format!("{indent}{}", term::paint(&note, paint)));
    }
    if let Some(oa) = w.open_access.as_ref().filter(|oa| oa.is_oa) {
        let terms: Vec<&str> = [oa.status.as_deref(), oa.license.as_deref()]
            .into_iter()
            .flatten()
            .collect();
        let link = oa.pdf_url.as_deref().or(oa.url.as_deref()).unwrap_or("");
        let terms = if terms.is_empty() {
            String::new()
        } else {
            format!(" ({})", terms.join(", "))
        };
        lines.push(format!("{indent}Free copy{terms}: {link}"));
    }
    lines.join("\n")
}

/// "PDF" when a free PDF is known, "yes" for a free copy without one,
/// "no" when known to be closed, "" when unknown.
pub fn free_copy(w: &Work) -> &'static str {
    match &w.open_access {
        Some(oa) if oa.pdf_url.is_some() => "PDF",
        Some(oa) if oa.is_oa => "yes",
        Some(_) => "no",
        None => "",
    }
}

/// An abstract for terminal display, indented and shortened.
pub fn abstract_text(found: Option<&Abstract>, indent: &str) -> String {
    let Some(a) = found else {
        return format!("{indent}(no abstract available)");
    };
    let mut text: String = a.text.chars().take(TABLE_ABSTRACT).collect();
    if a.text.chars().count() > TABLE_ABSTRACT {
        text.push('…');
    }
    format!("{indent}abstract ({}): {text}", a.source)
}

/// A ranked search hit: `" 1. Title"`, then the rest of the work under it.
pub fn hit_text(rank: usize, hit: &Hit) -> String {
    let work = work_text(&hit.record, "    ");
    format!("{rank:>2}. {}\n", work.trim_start_matches(' '))
}

/// ✓ agrees, ~ partly, ✗ differs, ? unknown.
fn mark(g: Agreement) -> &'static str {
    match g {
        Agreement::Match => "✓",
        Agreement::Partial => "~",
        Agreement::Mismatch => "✗",
        Agreement::Unknown => "?",
    }
}

/// The fields compared with the citation, and how each agrees; those
/// that couldn't be compared left out.
fn checks(c: &Candidate) -> Vec<(&'static str, Agreement)> {
    let a = &c.assessment;
    [
        ("title", a.title.agreement),
        ("authors", a.authors.agreement),
        ("year", a.year.agreement),
        ("journal", a.container.agreement),
        ("volume", a.volume.agreement),
        ("page", a.page.agreement),
        ("id", a.ids),
    ]
    .into_iter()
    .filter(|(_, g)| *g != Agreement::Unknown)
    .collect()
}

/// "crossref citation match", "openalex lookup": how a candidate was found.
fn found_via(c: &Candidate) -> Vec<String> {
    c.sightings
        .iter()
        .map(|s| format!("{} {}", s.provider, s.via.as_str().replace('_', " ")))
        .collect()
}

/// "confidence 0.97  title ✓  authors ✓  year ✓  id ✓", marks coloured.
pub fn checks_text(c: &Candidate) -> String {
    let checks: Vec<String> = checks(c)
        .into_iter()
        .map(|(name, g)| {
            let paint = match g {
                Agreement::Match => Paint::Good,
                Agreement::Partial => Paint::Warning,
                _ => Paint::Error,
            };
            format!("{name} {}", term::paint(mark(g), paint))
        })
        .collect();
    format!(
        "confidence {:.2}  {}",
        c.assessment.confidence,
        checks.join("  ")
    )
}

/// A candidate as terminal text: confidence and checks, the work, and how
/// it was found.
pub fn candidate_text(c: &Candidate, indent: &str) -> String {
    let via = format!("via {}", found_via(c).join(", "));
    format!(
        "{indent}{}\n{}\n{indent}{}",
        checks_text(c),
        work_text(&c.merged_record(), indent),
        term::paint(&via, Paint::Dim)
    )
}

/// One line per provider, for stderr: "openalex: $0.0010 spent, $0.9960 left".
/// Costs are provider units; both built-in paid providers report USD.
pub fn usage_text(usage: &[(ProviderId, Usage)]) -> Option<String> {
    let lines: Vec<String> = usage
        .iter()
        .filter_map(|(p, u)| {
            let parts: Vec<String> = [
                u.cost.map(|c| format!("${c:.4} spent")),
                u.remaining.map(|r| format!("${r:.4} left today")),
            ]
            .into_iter()
            .flatten()
            .collect();
            (!parts.is_empty()).then(|| format!("{p}: {}", parts.join(", ")))
        })
        .collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// [`usage_text`] on stderr, dimmed: it's secondary to the results.
pub fn eprint_usage(usage: &[(ProviderId, Usage)]) {
    if let Some(text) = usage_text(usage) {
        eprint_dim(&text);
    }
}

/// A secondary line on stderr (totals, paging, cost), dimmed.
pub fn eprint_dim(text: &str) {
    eprintln!("{}", term::paint_err(text, Paint::Dim));
}

/// "warning: …" on stderr.
pub fn eprint_warning(text: &str) {
    eprintln!("{} {text}", term::paint_err("warning:", Paint::Warning));
}

/// "LeCun et al.", "Bishop", or "" when there are no authors.
pub fn first_author(w: &Work) -> String {
    let Some(first) = w.authors.first() else {
        return String::new();
    };
    // Family names throughout; an organisation's name (or one that isn't
    // clearly a person's) whole. Its last word made "et et al." of a
    // society listed as "Société française d'anesthésie et".
    let name = first
        .parts()
        .map(|(family, _)| family)
        .or_else(|| first.display_name())
        .unwrap_or_default();
    if w.authors.len() > 1 {
        format!("{name} et al.")
    } else {
        name
    }
}

/// [`first_author`] cut to fit a table: "Société française… et al.".
pub fn author_cell(w: &Work) -> String {
    let name = first_author(w);
    if name.chars().count() <= TABLE_AUTHOR {
        return name;
    }
    let (name, rest) = match name.strip_suffix(" et al.") {
        Some(name) => (name, " et al."),
        None => (name.as_str(), ""),
    };
    let keep = TABLE_AUTHOR - 1 - rest.chars().count();
    let cut: String = name.chars().take(keep).collect();
    format!("{}…{rest}", cut.trim_end())
}

/// 4300 → "4,300".
pub fn thousands(n: u64) -> String {
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

pub fn year(w: &Work) -> String {
    w.issued.map(|d| d.year().to_string()).unwrap_or_default()
}

/// The identifier most useful to cite: a DOI, else the first.
pub fn main_id(ids: &[WorkId]) -> String {
    ids.iter()
        .find(|id| id.kind() == IdKind::Doi)
        .or(ids.first())
        .map(ToString::to_string)
        .unwrap_or_default()
}

/// How much a match's confidence needs looking at.
pub fn confidence_level(confidence: f64) -> Level {
    match confidence {
        c if c >= VERIFIED => Level::Good,
        c if c >= PLAUSIBLE => Level::Warning,
        _ => Level::Error,
    }
}

/// The columns of [`candidate_row`]. Too narrow for them all, how each
/// work was found goes first, then its identifier, author and year move
/// under its title.
pub fn candidate_cols() -> [Col; 8] {
    [
        Col::content("#").right(),
        Col::content("Conf"),
        Col::content("Checks"),
        Col::cap("Author", TABLE_AUTHOR).folds(3, "Title"),
        Col::content("Year").folds_beside(3, "Title"),
        Col::fill("Title").or_when_folded("Work"),
        Col::content("ID").folds(2, "Title"),
        Col::cap("Via", 16).drops(1),
    ]
}

/// A scored candidate as a table row, coloured by confidence.
pub fn candidate_row(rank: usize, c: &Candidate) -> Vec<Text> {
    let checks: Vec<String> = checks(c)
        .into_iter()
        .filter(|(name, _)| matches!(*name, "title" | "authors" | "year" | "id"))
        .map(|(name, g)| format!("{name} {}", mark(g)))
        .collect();
    let w = &c.record.work;
    let mut title = w.title.clone().unwrap_or_default();
    if let Some(r) = c.retraction() {
        title = format!("{title}\n⚠ {}", describe_update(r));
    }
    vec![
        Text::from(rank.to_string()),
        grid::status(
            format!("{:.2}", c.assessment.confidence),
            confidence_level(c.assessment.confidence),
        ),
        Text::from(checks.join("\n")),
        Text::from(author_cell(w)),
        Text::from(year(w)),
        Text::from(title),
        Text::from(main_id(&c.ids)),
        Text::from(found_via(c).join("\n")),
    ]
}

#[cfg(test)]
mod tests {
    use aspero_core::{FieldSet, PartialDate, Person, Raw, Work, WorkType};
    use jiff::Timestamp;

    use super::*;

    fn record() -> SourceRecord {
        let mut work = Work::default();
        work.title = Some("Deep learning".into());
        work.authors = [
            "Yann LeCun",
            "Yoshua Bengio",
            "Geoffrey Hinton",
            "A. N. Other",
        ]
        .map(Person::literal)
        .to_vec();
        work.issued = PartialDate::new(2015, Some(5), Some(27));
        work.work_type = Some(WorkType::JournalArticle);
        work.container_title = Some("Nature".into());
        work.abstract_text = Some("Deep learning allows…".into());
        let raw = Raw {
            media_type: "application/json".into(),
            body: "{}".into(),
        };
        SourceRecord::new(
            ProviderId::from_static("crossref"),
            vec!["10.1038/nature14539".parse().unwrap()],
            work,
            FieldSet::ALL,
            raw,
            Timestamp::UNIX_EPOCH,
        )
    }

    #[test]
    fn text_abbreviates_long_author_lists() {
        assert_eq!(
            work_text(&record(), "  "),
            "  Deep learning\n  Yann LeCun, Yoshua Bengio, Geoffrey Hinton, et al. · 2015 · Nature · journal article\n  doi:10.1038/nature14539"
        );
    }

    #[test]
    fn first_authors_are_family_names_or_whole_organisations() {
        let first = |names: &[&str]| {
            let mut w = Work::default();
            w.authors = names.iter().map(|n| Person::literal(*n)).collect();
            first_author(&w)
        };
        assert_eq!(first(&["Yann LeCun", "Yoshua Bengio"]), "LeCun et al.");
        assert_eq!(first(&["Vincent van Gogh"]), "van Gogh");
        // Not "et et al." (W894996489, 2015).
        assert_eq!(
            first(&[
                "Société française d’anesthésie et de réanimation",
                "Société française de médecine d’urgence"
            ]),
            "Société française d’anesthésie et de réanimation et al."
        );
        assert_eq!(first(&["S. C. S"]), "S. C. S");
        assert_eq!(first(&[]), "");
    }

    #[test]
    fn csl_json_is_one_array_at_the_end() {
        let out = crate::cmd::Out {
            format: crate::args::Format::CslJson,
            full: false,
            abstracts: false,
            style: None,
            locale: None,
        };
        let mut export = Exporter::new(&out).unwrap();
        let r = record();
        let mut other = record();
        other.work.title = Some("Attention is all you need".into());
        // Verifications finish in any order; the array is in input order.
        assert_eq!(export.entry_at(1, &other.work, &other.ids), "");
        assert_eq!(export.entry_at(0, &r.work, &r.ids), "");
        let json: serde_json::Value = serde_json::from_str(&export.finish()).unwrap();
        assert_eq!(json[0]["title"], "Deep learning");
        assert_eq!(json[0]["DOI"], "10.1038/nature14539");
        assert_eq!(json[0]["type"], "article-journal");
        assert_eq!(json[1]["title"], "Attention is all you need");
    }
}
