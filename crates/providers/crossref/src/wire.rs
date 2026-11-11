//! Crossref REST JSON as received, and its mapping into aspero's model.
//!
//! Wire types are lenient: every field defaults and unknown fields are
//! ignored (the full item is kept in `Raw`).

use std::collections::BTreeMap;

use aspero_core::csl::{
    CrossrefRelation, CrossrefUpdate, plain_text, relation_updates, strip_markup,
};
use aspero_core::{
    Doi, FieldSet, PartialDate, Person, Raw, SourceRecord, Work, WorkId, WorkType, tidy_abstract,
};
use jiff::Timestamp;
use serde::Deserialize;

use crate::ID;

/// The `{status, message}` envelope around every response.
#[derive(Debug, Deserialize)]
pub(crate) struct Envelope<T> {
    pub message: T,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub(crate) struct WireList {
    pub total_results: Option<u64>,
    pub next_cursor: Option<String>,
    /// Kept as raw values so each work's `Raw` is its own JSON object.
    pub items: Vec<serde_json::Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct WireWork {
    #[serde(rename = "DOI")]
    doi: Option<String>,
    #[serde(rename = "URL")]
    url: Option<String>,
    #[serde(rename = "type")]
    work_type: Option<String>,
    title: Vec<String>,
    container_title: Vec<String>,
    publisher: Option<String>,
    volume: Option<String>,
    issue: Option<String>,
    page: Option<String>,
    issued: WireDate,
    published: WireDate,
    published_print: WireDate,
    published_online: WireDate,
    author: Vec<WireAuthor>,
    #[serde(rename = "abstract")]
    abstract_jats: Option<String>,
    is_referenced_by_count: Option<u64>,
    /// Retraction Watch and publisher notices about this work.
    updated_by: Vec<CrossrefUpdate>,
    /// For a notice, the works it updates.
    update_to: Vec<CrossrefUpdate>,
    relation: BTreeMap<String, Vec<CrossrefRelation>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WireAuthor {
    given: Option<String>,
    family: Option<String>,
    /// Organisational authors.
    name: Option<String>,
    #[serde(rename = "ORCID")]
    orcid: Option<String>,
}

/// `{"date-parts": [[2015, 5, 27]]}`; parts may be missing or `null`.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct WireDate {
    date_parts: Vec<Vec<Option<i64>>>,
}

impl WireDate {
    fn to_partial(&self) -> Option<PartialDate> {
        let parts = self.date_parts.first()?;
        let get = |i: usize| parts.get(i).copied().flatten();
        let year = i16::try_from(get(0)?).ok()?;
        let month = get(1).and_then(|m| u8::try_from(m).ok());
        let day = month.and(get(2)).and_then(|d| u8::try_from(d).ok());
        PartialDate::new(year, month, day)
            .or_else(|| PartialDate::new(year, month, None))
            .or(Some(PartialDate::from_year(year)))
    }
}

/// Maps one Crossref work object into a [`SourceRecord`]. Crossref asserts
/// exactly one identifier: the DOI.
pub(crate) fn to_record(
    value: &serde_json::Value,
    fetched_at: Timestamp,
) -> serde_json::Result<SourceRecord> {
    let w = WireWork::deserialize(value)?;
    let ids: Vec<WorkId> = w
        .doi
        .as_deref()
        .and_then(|d| d.parse::<Doi>().ok())
        .map(WorkId::from)
        .into_iter()
        .collect();
    Ok(SourceRecord::new(
        ID,
        ids,
        w.work(),
        FieldSet::ALL,
        Raw::json(value),
        fetched_at,
    ))
}

impl WireWork {
    fn work(self) -> Work {
        let mut work = Work::default();
        work.title = self.title.into_iter().next().map(plain_text);
        work.authors = self
            .author
            .into_iter()
            .filter_map(WireAuthor::person)
            .collect();
        work.issued = self
            .issued
            .to_partial()
            .or_else(|| self.published.to_partial());
        work.work_type = self.work_type.as_deref().map(work_type);
        work.container_title = self.container_title.into_iter().next().map(plain_text);
        work.publisher = self.publisher;
        work.volume = self.volume;
        work.issue = self.issue;
        work.page = self.page;
        work.abstract_text = self
            .abstract_jats
            .as_deref()
            .map(|jats| tidy_abstract(&strip_markup(jats)));
        work.url = self.url;
        work.cited_by_count = self.is_referenced_by_count;
        // Online-first and print years often differ by one; citations use
        // either.
        let issued_year = work.issued.map(|d| d.year());
        let mut other: Vec<i16> = [&self.published_print, &self.published_online]
            .into_iter()
            .filter_map(|d| d.to_partial().map(|p| p.year()))
            .filter(|y| Some(*y) != issued_year)
            .collect();
        other.sort_unstable();
        other.dedup();
        work.other_years = other;
        work.updates = self
            .updated_by
            .iter()
            .filter_map(CrossrefUpdate::to_update)
            .chain(relation_updates(&self.relation))
            .collect();
        work.update_to = self
            .update_to
            .iter()
            .filter_map(CrossrefUpdate::to_update_to)
            .collect();
        work.type_as_notice();
        work
    }
}

impl WireAuthor {
    fn person(self) -> Option<Person> {
        let mut person = Person::default();
        match (self.given, self.family, self.name) {
            (given, Some(family), _) => {
                person.given = given;
                person.family = Some(family);
            }
            (_, None, Some(name)) | (Some(name), None, None) => person.literal = Some(name),
            (None, None, None) => return None,
        }
        person.orcid = self.orcid.map(|o| {
            o.trim_start_matches("https://orcid.org/")
                .trim_start_matches("http://orcid.org/")
                .to_owned()
        });
        Some(person)
    }
}

/// Crossref types to CSL.
fn work_type(crossref: &str) -> WorkType {
    match crossref {
        "journal-article" => WorkType::JournalArticle,
        "proceedings-article" => WorkType::ConferencePaper,
        "posted-content" => WorkType::Preprint,
        "book" | "monograph" | "edited-book" | "reference-book" | "book-set" => WorkType::Book,
        "book-chapter" | "book-section" | "book-part" | "reference-entry" => WorkType::Chapter,
        "dissertation" => WorkType::Thesis,
        "report" | "report-component" => WorkType::Report,
        "dataset" | "database" => WorkType::Dataset,
        other => WorkType::Other(other.to_owned()),
    }
}

/// The Crossref type to filter on for a [`WorkType`], if there is one.
pub(crate) fn crossref_type(t: &WorkType) -> Option<&str> {
    Some(match t {
        WorkType::JournalArticle => "journal-article",
        WorkType::ConferencePaper => "proceedings-article",
        WorkType::Preprint => "posted-content",
        WorkType::Book => "book",
        WorkType::Chapter => "book-chapter",
        WorkType::Thesis => "dissertation",
        WorkType::Report => "report",
        WorkType::Dataset => "dataset",
        WorkType::Other(other) => other,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_jats_to_plain_text() {
        let jats = "<jats:title>Abstract</jats:title><jats:p>Deep  learning &amp; <jats:italic>more</jats:italic>.</jats:p>";
        assert_eq!(strip_markup(jats), "Abstract Deep learning & more.");
    }

    #[test]
    fn dates_keep_their_precision_and_tolerate_nulls() {
        let date = |v: serde_json::Value| {
            serde_json::from_value::<WireDate>(v)
                .unwrap()
                .to_partial()
                .map(|d| d.to_string())
        };
        assert_eq!(
            date(serde_json::json!({"date-parts": [[2015, 5, 27]]})).unwrap(),
            "2015-05-27"
        );
        assert_eq!(
            date(serde_json::json!({"date-parts": [[2017, 4]]})).unwrap(),
            "2017-04"
        );
        assert_eq!(
            date(serde_json::json!({"date-parts": [[2017, 2, 31]]})).unwrap(),
            "2017-02"
        );
        assert_eq!(date(serde_json::json!({"date-parts": [[null]]})), None);
        assert_eq!(date(serde_json::json!({})), None);
    }

    #[test]
    fn organisational_and_orcid_authors() {
        let author = |v: serde_json::Value| {
            serde_json::from_value::<WireAuthor>(v)
                .unwrap()
                .person()
                .unwrap()
        };
        let org = author(serde_json::json!({"name": "ATLAS Collaboration"}));
        assert_eq!(org.display_name().as_deref(), Some("ATLAS Collaboration"));
        let p = author(serde_json::json!({
            "given": "Ada", "family": "Lovelace", "ORCID": "http://orcid.org/0000-0002-1825-0097"
        }));
        assert_eq!(p.display_name().as_deref(), Some("Ada Lovelace"));
        assert_eq!(p.orcid.as_deref(), Some("0000-0002-1825-0097"));
    }
}
