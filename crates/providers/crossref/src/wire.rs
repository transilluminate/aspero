//! Crossref REST JSON as received, and its mapping into aspero's model.
//!
//! A work is CSL-like, so it is read as a [`CslItem`], plus the fields CSL
//! lacks. Lenient: every field defaults and unknown fields are ignored (the
//! full item is kept in `Raw`).

use aspero_core::csl::{CslDate, CslItem, null_default};
use aspero_core::{Doi, FieldSet, Raw, SourceRecord, Work, WorkId, WorkType};
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
    #[serde(deserialize_with = "null_default")]
    pub items: Vec<serde_json::Value>,
}

/// A work's fields beyond CSL's.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct WireExtras {
    published_print: Option<CslDate>,
    published_online: Option<CslDate>,
}

/// Maps one Crossref work object into a [`SourceRecord`]. Crossref asserts
/// exactly one identifier: the DOI.
pub(crate) fn to_record(
    value: &serde_json::Value,
    fetched_at: Timestamp,
) -> serde_json::Result<SourceRecord> {
    let item = CslItem::deserialize(value)?;
    let extras = WireExtras::deserialize(value)?;
    let ids: Vec<WorkId> = item
        .doi
        .as_deref()
        .and_then(|d| d.parse::<Doi>().ok())
        .map(WorkId::from)
        .into_iter()
        .collect();
    Ok(SourceRecord::new(
        ID,
        ids,
        extras.work(&item),
        FieldSet::ALL,
        Raw::json(value),
        fetched_at,
    ))
}

impl WireExtras {
    fn work(&self, item: &CslItem) -> Work {
        let mut work = item.to_work();
        // Online-first and print years often differ by one; citations use
        // either.
        let issued_year = work.issued.map(|d| d.year());
        let mut other: Vec<i16> = [&self.published_print, &self.published_online]
            .into_iter()
            .filter_map(|d| d.as_ref()?.to_partial().map(|p| p.year()))
            .filter(|y| Some(*y) != issued_year)
            .collect();
        other.sort_unstable();
        other.dedup();
        work.other_years = other;
        work
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

    fn work(v: serde_json::Value) -> Work {
        to_record(&v, Timestamp::UNIX_EPOCH).unwrap().work
    }

    #[test]
    fn reads_as_csl_plus_print_and_online_years() {
        let w = work(serde_json::json!({
            "DOI": "10.1000/x",
            "type": "reference-entry",
            "author": [{"name": "ATLAS Collaboration"}],
            "issued": {"date-parts": [[2016, 12, 30]]},
            "published-online": {"date-parts": [[2016, 12, 30]]},
            "published-print": {"date-parts": [[2017, 2]]},
            "ISBN": ["9780262035613"],
            "abstract": "<jats:p> </jats:p>"
        }));
        assert_eq!(w.work_type, Some(WorkType::Chapter));
        assert_eq!(
            w.authors[0].display_name().as_deref(),
            Some("ATLAS Collaboration")
        );
        assert_eq!(w.other_years, [2017]);
        assert_eq!(w.abstract_text, None, "tags alone are no abstract");
    }
}
