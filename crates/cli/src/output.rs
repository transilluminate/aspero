//! Rendering works, results and usage as JSON or terminal text.
//!
//! JSON shapes are the agent contract: fields are omitted when unknown rather
//! than `null`, and the lean default set is enough to identify and cite.

use aspero_core::{
    Agreement, Hit, IdKind, Person, ProviderId, SourceRecord, UpdateKind, Usage, Work, WorkId,
};
use aspero_engine::{Abstract, Candidate, describe_update};

/// Abstract characters shown in tables; JSON has the full text.
const TABLE_ABSTRACT: usize = 600;
use serde_json::{Map, Value, json};

/// Authors shown in a table before "et al.".
const TABLE_AUTHORS: usize = 3;

/// One work: lean by default, every known field with `full`.
pub fn work(record: &SourceRecord, full: bool) -> Value {
    let w = &record.work;
    let mut out = Map::new();
    out.insert("source".into(), json!(record.source));
    out.insert("ids".into(), json!(record.ids));
    let mut put = |key: &str, value: Option<Value>| {
        if let Some(v) = value {
            out.insert(key.into(), v);
        }
    };
    put("title", w.title.as_ref().map(|t| json!(t)));
    if !w.authors.is_empty() {
        let authors: Vec<Value> = w.authors.iter().filter_map(|a| author(a, full)).collect();
        put("authors", Some(json!(authors)));
    }
    put("issued", w.issued.map(|d| json!(d.to_string())));
    put("type", w.work_type.as_ref().map(|t| json!(t.as_csl())));
    put("container", w.container_title.as_ref().map(|c| json!(c)));
    put("url", w.url.as_ref().map(|u| json!(u)));
    // Never lean enough to leave out: a retracted work is not citable.
    put("updates", (!w.updates.is_empty()).then(|| json!(w.updates)));
    put(
        "update_to",
        (!w.update_to.is_empty()).then(|| json!(w.update_to)),
    );
    if full {
        // Everything else, in the core `Work` shape (structured authors).
        if let Ok(Value::Object(all)) = serde_json::to_value(w) {
            for (k, v) in all {
                out.entry(k).or_insert(v);
            }
        }
        out.insert("fetched_at".into(), json!(record.fetched_at.to_string()));
    }
    Value::Object(out)
}

/// One author, the same shape lean or full: always a `name`, then the ORCID
/// iD, and with `full` the name's parts.
fn author(person: &Person, full: bool) -> Option<Value> {
    let mut out = Map::new();
    out.insert("name".into(), json!(person.display_name()?));
    if full && let Ok(Value::Object(parts)) = serde_json::to_value(person) {
        out.extend(parts);
    } else if let Some(orcid) = &person.orcid {
        out.insert("orcid".into(), json!(orcid));
    }
    Some(Value::Object(out))
}

pub fn usage(usage: &[(ProviderId, Usage)]) -> Value {
    let map: Map<String, Value> = usage
        .iter()
        .map(|(p, u)| (p.to_string(), usage_one(u)))
        .collect();
    Value::Object(map)
}

pub fn usage_one(u: &Usage) -> Value {
    let mut out = Map::new();
    if let Some(c) = u.cost {
        out.insert("cost".into(), json!(c));
    }
    if let Some(r) = u.remaining {
        out.insert("remaining".into(), json!(r));
    }
    if let Some(t) = u.resets_at {
        out.insert("resets_at".into(), json!(t.to_string()));
    }
    Value::Object(out)
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
    lines.join("\n")
}

/// Sets a work's `abstract` (null when none was found) and
/// `abstract_source`.
pub fn set_abstract(work: &mut Value, found: Option<&Abstract>) {
    match found {
        Some(a) => {
            work["abstract"] = json!(a.text);
            work["abstract_source"] = json!(a.source);
        }
        None => work["abstract"] = Value::Null,
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

/// A scored candidate: confidence, per-field checks, who found it, and the
/// work (with every identifier merged records asserted).
pub fn candidate(c: &Candidate, full: bool) -> Value {
    let a = &c.assessment;
    let checks = if full {
        json!({"title": a.title, "authors": a.authors, "year": a.year, "ids": a.ids})
    } else {
        json!({
            "title": a.title.agreement,
            "authors": a.authors.agreement,
            "year": a.year.agreement,
            "ids": a.ids,
        })
    };
    let found_by: Vec<Value> = c
        .sightings
        .iter()
        .map(|s| json!({"provider": s.provider, "via": s.via.as_str(), "rank": s.rank, "score": s.score}))
        .collect();
    let mut work = work(&c.record, full);
    work["ids"] = json!(c.ids);
    if !c.updates.is_empty() {
        work["updates"] = json!(c.updates);
    }
    json!({
        "confidence": (a.confidence * 100.0).round() / 100.0,
        "checks": checks,
        "found_by": found_by,
        "work": work,
    })
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
    use aspero_core::{FieldSet, PartialDate, Raw, Work, WorkType};
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
    fn lean_json_identifies_and_cites_without_extras() {
        assert_eq!(
            work(&record(), false),
            json!({
                "source": "crossref",
                "ids": ["doi:10.1038/nature14539"],
                "title": "Deep learning",
                "authors": [
                    {"name": "Yann LeCun"},
                    {"name": "Yoshua Bengio"},
                    {"name": "Geoffrey Hinton"},
                    {"name": "A. N. Other"},
                ],
                "issued": "2015-05-27",
                "type": "article-journal",
                "container": "Nature",
            })
        );
    }

    #[test]
    fn full_json_adds_everything() {
        let full = work(&record(), true);
        assert_eq!(full["abstract"], "Deep learning allows…");
        assert_eq!(full["authors"][0]["literal"], "Yann LeCun");
        assert!(full.get("fetched_at").is_some());
    }

    #[test]
    fn authors_have_one_shape_lean_or_full() {
        let mut r = record();
        let mut lecun = Person::new("Yann", "LeCun");
        lecun.orcid = Some("0000-0000-0000-0001".into());
        r.work.authors = vec![lecun];
        assert_eq!(
            work(&r, false)["authors"],
            json!([{"name": "Yann LeCun", "orcid": "0000-0000-0000-0001"}])
        );
        assert_eq!(
            work(&r, true)["authors"],
            json!([{
                "name": "Yann LeCun",
                "given": "Yann",
                "family": "LeCun",
                "orcid": "0000-0000-0000-0001",
            }])
        );
    }

    #[test]
    fn text_abbreviates_long_author_lists() {
        assert_eq!(
            work_text(&record(), "  "),
            "  Deep learning\n  Yann LeCun, Yoshua Bengio, Geoffrey Hinton, et al. · 2015 · Nature · article-journal\n  doi:10.1038/nature14539"
        );
    }
}
