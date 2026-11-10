//! OpenAlex JSON as received, and its mapping into aspero's model.
//!
//! Wire types are lenient: every field defaults, unknown fields are ignored
//! (the full response is kept in `Raw`), so upstream additions never break
//! decoding.

use std::collections::BTreeMap;

use aspero_core::{
    ArxivId, Doi, FieldSet, OpenAlexId, PartialDate, Person, Pmcid, Pmid, Raw, SourceRecord, Work,
    WorkId, WorkType,
};
use jiff::Timestamp;
use serde::Deserialize;

use crate::ID;

/// The DataCite prefix arXiv uses for its DOIs (`10.48550/arxiv.<id>`).
pub(crate) const ARXIV_DOI_PREFIX: &str = "10.48550";

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct WireWork {
    id: Option<String>,
    doi: Option<String>,
    title: Option<String>,
    display_name: Option<String>,
    publication_year: Option<i16>,
    publication_date: Option<String>,
    #[serde(rename = "type")]
    work_type: Option<String>,
    ids: WireIds,
    authorships: Vec<WireAuthorship>,
    primary_location: Option<WireLocation>,
    locations: Vec<WireLocation>,
    biblio: WireBiblio,
    abstract_inverted_index: Option<BTreeMap<String, Vec<usize>>>,
    cited_by_count: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WireIds {
    openalex: Option<String>,
    doi: Option<String>,
    pmid: Option<String>,
    pmcid: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WireAuthorship {
    author: WireAuthor,
    raw_author_name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WireAuthor {
    display_name: Option<String>,
    orcid: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WireLocation {
    landing_page_url: Option<String>,
    source: Option<WireSource>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WireSource {
    display_name: Option<String>,
    host_organization_name: Option<String>,
    #[serde(rename = "type")]
    source_type: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WireBiblio {
    volume: Option<String>,
    issue: Option<String>,
    first_page: Option<String>,
    last_page: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct WireList {
    pub meta: WireMeta,
    /// Kept as raw values so each work's `Raw` is its own JSON object.
    pub results: Vec<serde_json::Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct WireMeta {
    pub count: Option<u64>,
    pub next_cursor: Option<String>,
}

/// Maps one OpenAlex work object into a [`SourceRecord`].
pub(crate) fn to_record(
    value: &serde_json::Value,
    raw: Raw,
    fetched_at: Timestamp,
) -> serde_json::Result<SourceRecord> {
    let w = WireWork::deserialize(value)?;
    Ok(SourceRecord::new(
        ID,
        w.ids(),
        w.work(),
        FieldSet::ALL,
        raw,
        fetched_at,
    ))
}

impl WireWork {
    /// Every identifier OpenAlex asserts, OpenAlex ID first. Values that don't
    /// parse are skipped here; they remain in the raw record.
    fn ids(&self) -> Vec<WorkId> {
        let mut ids: Vec<WorkId> = Vec::new();
        let mut push = |id: WorkId| {
            if !ids.contains(&id) {
                ids.push(id);
            }
        };

        let openalex = self.ids.openalex.as_ref().or(self.id.as_ref());
        if let Some(id) = openalex.and_then(|s| s.parse::<OpenAlexId>().ok()) {
            push(id.into());
        }
        let doi = self.ids.doi.as_ref().or(self.doi.as_ref());
        if let Some(doi) = doi.and_then(|s| s.parse::<Doi>().ok()) {
            if let Some(arxiv) = arxiv_from_doi(&doi) {
                push(arxiv.into());
            }
            push(doi.into());
        }
        if let Some(id) = self.ids.pmid.as_ref().and_then(|s| s.parse::<Pmid>().ok()) {
            push(id.into());
        }
        if let Some(id) = self
            .ids
            .pmcid
            .as_ref()
            .and_then(|s| s.parse::<Pmcid>().ok())
        {
            push(id.into());
        }
        // arXiv versions of published works appear as arxiv.org locations.
        for url in self
            .locations
            .iter()
            .filter_map(|l| l.landing_page_url.as_ref())
        {
            if url.contains("arxiv.org/")
                && let Ok(id) = url.parse::<ArxivId>()
            {
                push(id.unversioned().into());
            }
        }
        ids
    }

    fn work(&self) -> Work {
        let source = self
            .primary_location
            .as_ref()
            .and_then(|l| l.source.as_ref());
        let mut work = Work::default();
        work.title = self.title.clone().or_else(|| self.display_name.clone());
        work.authors = self
            .authorships
            .iter()
            .filter_map(WireAuthorship::person)
            .collect();
        work.issued = self
            .publication_date
            .as_deref()
            .and_then(|d| d.parse().ok())
            .or(self.publication_year.map(PartialDate::from_year));
        work.work_type = self
            .work_type
            .as_deref()
            .map(|t| work_type(t, source.and_then(|s| s.source_type.as_deref())));
        work.container_title = source.and_then(|s| s.display_name.clone());
        work.publisher = source.and_then(|s| s.host_organization_name.clone());
        work.volume = self.biblio.volume.clone();
        work.issue = self.biblio.issue.clone();
        work.page = match (&self.biblio.first_page, &self.biblio.last_page) {
            (Some(first), Some(last)) if first != last => Some(format!("{first}-{last}")),
            (Some(first), _) => Some(first.clone()),
            (None, last) => last.clone(),
        };
        work.abstract_text = self.abstract_inverted_index.as_ref().map(rebuild_abstract);
        work.url = self
            .primary_location
            .as_ref()
            .and_then(|l| l.landing_page_url.clone());
        work.cited_by_count = self.cited_by_count;
        work
    }
}

impl WireAuthorship {
    fn person(&self) -> Option<Person> {
        let name = self
            .author
            .display_name
            .as_ref()
            .or(self.raw_author_name.as_ref())?;
        let mut person = Person::literal(name.clone());
        person.orcid = self
            .author
            .orcid
            .as_deref()
            .map(|o| o.trim_start_matches("https://orcid.org/").to_owned());
        Some(person)
    }
}

pub(crate) fn arxiv_from_doi(doi: &Doi) -> Option<ArxivId> {
    if doi.prefix() != ARXIV_DOI_PREFIX {
        return None;
    }
    doi.suffix().strip_prefix("arxiv.")?.parse().ok()
}

/// OpenAlex types to CSL. Conference papers have their own type
/// (`conference-paper`, seen 2026-11), but older records may still be an
/// `article` in a conference source. OpenAlex's `review` means a review
/// *article*, unlike CSL's `review` (a review of another work).
fn work_type(openalex: &str, source_type: Option<&str>) -> WorkType {
    match (openalex, source_type) {
        ("conference-paper", _) | ("article", Some("conference")) => WorkType::ConferencePaper,
        ("article" | "review" | "letter" | "editorial", _) => WorkType::JournalArticle,
        ("preprint", _) => WorkType::Preprint,
        ("book", _) => WorkType::Book,
        ("book-chapter", _) => WorkType::Chapter,
        ("dissertation", _) => WorkType::Thesis,
        ("report", _) => WorkType::Report,
        ("dataset", _) => WorkType::Dataset,
        (other, _) => WorkType::from_csl(other),
    }
}

/// OpenAlex ships abstracts as `{word: [positions]}`; put the words back in order.
fn rebuild_abstract(index: &BTreeMap<String, Vec<usize>>) -> String {
    let mut positioned: Vec<(usize, &str)> = index
        .iter()
        .flat_map(|(word, positions)| positions.iter().map(move |&p| (p, word.as_str())))
        .collect();
    positioned.sort_unstable();
    positioned
        .into_iter()
        .map(|(_, word)| word)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebuilds_inverted_abstracts() {
        let index = BTreeMap::from([
            ("is".to_owned(), vec![1]),
            ("Attention".to_owned(), vec![0]),
            ("all".to_owned(), vec![2]),
            ("you".to_owned(), vec![3, 5]),
            ("need".to_owned(), vec![4]),
        ]);
        assert_eq!(rebuild_abstract(&index), "Attention is all you need you");
    }

    #[test]
    fn maps_types_with_source_context() {
        assert_eq!(
            work_type("article", Some("conference")),
            WorkType::ConferencePaper
        );
        assert_eq!(
            work_type("conference-paper", Some("conference")),
            WorkType::ConferencePaper
        );
        assert_eq!(
            work_type("article", Some("journal")),
            WorkType::JournalArticle
        );
        assert_eq!(work_type("review", None), WorkType::JournalArticle);
        assert_eq!(
            work_type("paratext", None),
            WorkType::Other("paratext".into())
        );
    }

    #[test]
    fn arxiv_doi_maps_to_arxiv_id() {
        let doi: Doi = "10.48550/arXiv.1706.03762".parse().unwrap();
        assert_eq!(arxiv_from_doi(&doi).unwrap().to_string(), "1706.03762");
        let other: Doi = "10.1038/nature14539".parse().unwrap();
        assert_eq!(arxiv_from_doi(&other), None);
    }
}
