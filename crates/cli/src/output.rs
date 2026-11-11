//! Rendering works, results and usage as terminal text and tables. JSON
//! shapes are the contract in `aspero-json`, re-exported here.

use aspero_core::{
    Agreement, Hit, IdKind, ProviderId, SourceRecord, UpdateKind, Usage, Work, WorkId,
};
use aspero_engine::{Abstract, Candidate, describe_update};
pub use aspero_json::{candidate, set_abstract, usage, work};

/// Abstract characters shown in tables; JSON has the full text.
const TABLE_ABSTRACT: usize = 600;

/// Authors shown in a table before "et al.".
const TABLE_AUTHORS: usize = 3;

/// Works written as BibTeX entries or RIS records (BibTeX keys kept
/// distinct: `lecun2015deep`, `lecun2015deepa`, …), or gathered into a
/// reference list in a citation style.
pub struct Exporter {
    format: crate::args::Format,
    keys: aspero_bib::Keys,
    style: Option<aspero_bib::cite::Style>,
    locale: Option<String>,
    works: Vec<(Work, Vec<WorkId>)>,
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
            (crate::args::Format::Cite, Some(s)) if s.ends_with(".csl") => {
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

    /// The work's entry to print now; empty for a style, whose list is
    /// ordered once every work is in ([`Exporter::finish`]).
    pub fn entry(&mut self, work: &Work, ids: &[WorkId]) -> String {
        match self.format {
            crate::args::Format::Ris => aspero_bib::ris(work, ids),
            crate::args::Format::Cite => {
                self.works.push((work.clone(), ids.to_vec()));
                String::new()
            }
            _ => aspero_bib::bibtex(work, ids, &self.keys.key(work)),
        }
    }

    /// A style's reference list, one entry per line; empty otherwise.
    pub fn finish(self) -> String {
        let Some(style) = &self.style else {
            return String::new();
        };
        let works: Vec<(&Work, &[WorkId])> =
            self.works.iter().map(|(w, ids)| (w, &ids[..])).collect();
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
        .filter_map(|a| a.display_name())
        .collect();
    if w.authors.len() > TABLE_AUTHORS {
        names.push("et al.".into());
    }
    let details: Vec<String> = [
        (!names.is_empty()).then(|| names.join(", ")),
        w.issued.map(|d| d.year().to_string()),
        w.container_title.clone(),
        w.work_type.as_ref().map(|t| t.as_csl().to_owned()),
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
        if !matches!(update.kind, UpdateKind::Correction) {
            lines.push(format!("{indent}⚠ {}", describe_update(update)));
        }
    }
    if let Some(oa) = w.open_access.as_ref().filter(|oa| oa.is_oa) {
        let terms: Vec<&str> = [oa.status.as_deref(), oa.license.as_deref()]
            .into_iter()
            .flatten()
            .collect();
        let link = oa.pdf_url.as_deref().or(oa.url.as_deref()).unwrap_or("");
        let terms = match terms.is_empty() {
            true => String::new(),
            false => format!(" ({})", terms.join(", ")),
        };
        lines.push(format!("{indent}Free copy{terms}: {link}"));
    }
    lines.join("\n")
}

/// "pdf" when a free PDF is known, "yes" for a free copy without one, "no"
/// when known to be closed, "" when unknown.
pub fn free_copy(w: &Work) -> &'static str {
    match &w.open_access {
        Some(oa) if oa.pdf_url.is_some() => "pdf",
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

/// A ranked search hit: `" 1. [91.9]"` then the work, indented.
pub fn hit_text(rank: usize, hit: &Hit) -> String {
    let score = hit.score.map(|s| format!(" [{s:.1}]")).unwrap_or_default();
    format!("{rank:>2}.{score}\n{}\n", work_text(&hit.record, "    "))
}

/// A candidate as terminal text: confidence and checks, then the work.
pub fn candidate_text(c: &Candidate, indent: &str) -> String {
    let a = &c.assessment;
    let check = |name: &str, g: Agreement| match g {
        Agreement::Unknown => None,
        g => Some(format!("{name} {}", agreement_word(g))),
    };
    let checks: Vec<String> = [
        check("title", a.title.agreement),
        check("authors", a.authors.agreement),
        check("year", a.year.agreement),
        check("journal", a.container.agreement),
        check("volume", a.volume.agreement),
        check("page", a.page.agreement),
        check("id", a.ids),
    ]
    .into_iter()
    .flatten()
    .collect();
    let via: Vec<String> = c
        .sightings
        .iter()
        .map(|s| format!("{} {}", s.provider, s.via.as_str().replace('_', " ")))
        .collect();
    let mut record = c.record.clone();
    record.ids = c.ids.clone();
    record.work.updates = c.updates.clone();
    format!(
        "{indent}{:.2}  {}  (via {})\n{}",
        a.confidence,
        checks.join(" · "),
        via.join(", "),
        work_text(&record, indent)
    )
}

fn agreement_word(g: Agreement) -> &'static str {
    match g {
        Agreement::Match => "match",
        Agreement::Partial => "partial",
        Agreement::Mismatch => "MISMATCH",
        _ => "unknown",
    }
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

// ------------------------------------------------------------ tables

/// A table in aspero's style: rounded borders, bold headers, columns that
/// wrap to the terminal's width. Colours apply only on a terminal.
pub fn table(headers: &[&str]) -> comfy_table::Table {
    use comfy_table::{Attribute, Cell, ContentArrangement, Table, presets};
    let mut t = Table::new();
    t.load_style(presets::UTF8_FULL_CONDENSED.with_rounded_corners())
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(
            headers
                .iter()
                .map(|h| Cell::new(h).add_attribute(Attribute::Bold)),
        );
    t
}

/// "LeCun et al.", "Bishop", or "" when there are no authors.
pub fn first_author(w: &Work) -> String {
    let Some(first) = w.authors.first() else {
        return String::new();
    };
    // Family names throughout; for unsplit names, the last word.
    let name = first
        .family
        .clone()
        .or_else(|| {
            let literal = first.literal.as_deref()?;
            literal.split_whitespace().last().map(str::to_owned)
        })
        .or_else(|| first.display_name())
        .unwrap_or_default();
    if w.authors.len() > 1 {
        format!("{name} et al.")
    } else {
        name
    }
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

/// A cell coloured by how much attention it needs.
pub fn status_cell(text: &str, level: Level) -> comfy_table::Cell {
    use comfy_table::{Cell, Color};
    let colour = match level {
        Level::Good => Color::Green,
        Level::Info => Color::Blue,
        Level::Warning => Color::Yellow,
        Level::Error => Color::Red,
    };
    Cell::new(text).fg(colour)
}

/// A scored candidate as a table row, coloured by confidence.
pub fn candidate_row(rank: usize, c: &Candidate) -> Vec<comfy_table::Cell> {
    use comfy_table::Cell;
    let a = &c.assessment;
    let mark = |g: Agreement| match g {
        Agreement::Match => "✓",
        Agreement::Partial => "~",
        Agreement::Mismatch => "✗",
        _ => "?",
    };
    let mut checks = format!(
        "title {} authors {} year {}",
        mark(a.title.agreement),
        mark(a.authors.agreement),
        mark(a.year.agreement)
    );
    if a.ids != Agreement::Unknown {
        checks.push_str(&format!(" id {}", mark(a.ids)));
    }
    let level = match a.confidence {
        c if c >= 0.85 => Level::Good,
        c if c >= 0.6 => Level::Warning,
        _ => Level::Error,
    };
    let w = &c.record.work;
    let mut title = w.title.clone().unwrap_or_default();
    if let Some(r) = c.retraction() {
        title = format!("{title}\n⚠ {}", describe_update(r));
    }
    let found_by: Vec<String> = c
        .sightings
        .iter()
        .map(|s| format!("{} {}", s.provider, s.via.as_str().replace('_', " ")))
        .collect();
    vec![
        Cell::new(rank),
        status_cell(&format!("{:.2}", a.confidence), level),
        Cell::new(checks),
        Cell::new(year(w)),
        Cell::new(first_author(w)),
        Cell::new(title),
        Cell::new(main_id(&c.ids)),
        Cell::new(found_by.join("\n")),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Good,
    Info,
    Warning,
    Error,
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
            "  Deep learning\n  Yann LeCun, Yoshua Bengio, Geoffrey Hinton, et al. · 2015 · Nature · article-journal\n  doi:10.1038/nature14539"
        );
    }
}
