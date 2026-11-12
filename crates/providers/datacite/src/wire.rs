//! DataCite JSON:API records as received, and their mapping.
//!
//! Lenient: every field defaults; unknown fields are ignored (the whole
//! record is kept in `Raw`).

use aspero_core::csl::{self, null_default, plain_text, strip_markup};
use aspero_core::{
    ArxivId, Doi, FieldSet, PartialDate, Person, Raw, SourceRecord, Work, WorkId, WorkType,
    tidy_abstract,
};
use jiff::Timestamp;
use serde::Deserialize;

use crate::{ARXIV_DOI_PREFIX, ID};

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct WireList {
    #[serde(deserialize_with = "null_default")]
    pub data: Vec<serde_json::Value>,
    #[serde(deserialize_with = "null_default")]
    pub meta: WireMeta,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct WireMeta {
    pub total: Option<u64>,
    pub total_pages: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WireDoi {
    id: Option<String>,
    #[serde(deserialize_with = "null_default")]
    attributes: Attributes,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Attributes {
    doi: Option<String>,
    #[serde(deserialize_with = "null_default")]
    titles: Vec<Title>,
    #[serde(deserialize_with = "null_default")]
    creators: Vec<Creator>,
    publication_year: Option<serde_json::Value>,
    publisher: Option<serde_json::Value>,
    #[serde(deserialize_with = "null_default")]
    container: Container,
    #[serde(deserialize_with = "null_default")]
    types: Types,
    #[serde(deserialize_with = "null_default")]
    descriptions: Vec<Description>,
    #[serde(deserialize_with = "null_default")]
    identifiers: Vec<Identifier>,
    url: Option<String>,
    citation_count: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Title {
    title: Option<String>,
    /// Absent for the main title; "Subtitle", "TranslatedTitle", …
    title_type: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Creator {
    name: Option<String>,
    given_name: Option<String>,
    family_name: Option<String>,
    #[serde(deserialize_with = "null_default")]
    name_identifiers: Vec<NameIdentifier>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct NameIdentifier {
    name_identifier: Option<String>,
    name_identifier_scheme: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Container {
    title: Option<String>,
    volume: Option<String>,
    issue: Option<String>,
    #[serde(rename = "firstPage")]
    first_page: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Types {
    resource_type_general: Option<String>,
    citeproc: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Description {
    description: Option<String>,
    description_type: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Identifier {
    identifier: Option<String>,
    identifier_type: Option<String>,
}

pub(crate) fn to_record(
    value: &serde_json::Value,
    fetched_at: Timestamp,
) -> serde_json::Result<SourceRecord> {
    let wire = WireDoi::deserialize(value)?;
    let a = &wire.attributes;
    let doi = a
        .doi
        .as_deref()
        .or(wire.id.as_deref())
        .and_then(|d| d.parse::<Doi>().ok());
    Ok(SourceRecord::new(
        ID,
        ids(doi, &a.identifiers),
        a.work(),
        FieldSet::ALL,
        Raw::json(value),
        fetched_at,
    ))
}

/// The DOI first, then the arXiv ID (stated, or implied by an arXiv DOI).
fn ids(doi: Option<Doi>, identifiers: &[Identifier]) -> Vec<WorkId> {
    let mut ids = Vec::new();
    let mut push = |id: WorkId| {
        if !ids.contains(&id) {
            ids.push(id);
        }
    };
    let arxiv_from_doi = doi.as_ref().and_then(|d| {
        (d.prefix() == ARXIV_DOI_PREFIX)
            .then(|| d.suffix().strip_prefix("arxiv.")?.parse::<ArxivId>().ok())
            .flatten()
    });
    if let Some(doi) = doi {
        push(WorkId::Doi(doi));
    }
    let stated = identifiers
        .iter()
        .filter(|i| i.identifier_type.as_deref() == Some("arXiv"))
        .filter_map(|i| i.identifier.as_deref()?.parse::<ArxivId>().ok());
    for arxiv in arxiv_from_doi.into_iter().chain(stated) {
        push(WorkId::Arxiv(arxiv.unversioned()));
    }
    ids
}

impl Attributes {
    fn work(&self) -> Work {
        let mut work = Work::default();
        work.title = self
            .titles
            .iter()
            .find(|t| t.title_type.is_none())
            .or(self.titles.first())
            .and_then(|t| t.title.clone())
            .map(plain_text);
        work.authors = self.creators.iter().filter_map(Creator::person).collect();
        work.issued = self
            .publication_year
            .as_ref()
            .and_then(|y| match y {
                serde_json::Value::Number(n) => n.as_i64(),
                serde_json::Value::String(s) => s.trim().parse().ok(),
                _ => None,
            })
            .and_then(|y| i16::try_from(y).ok())
            .map(PartialDate::from_year);
        work.work_type = work_type(&self.types);
        work.container_title = self.container.title.clone().map(plain_text);
        work.volume = self.container.volume.clone();
        work.issue = self.container.issue.clone();
        work.page = self.container.first_page.clone();
        // Since schema 4.4, publisher may be an object with a name.
        work.publisher = match &self.publisher {
            Some(serde_json::Value::String(s)) => Some(s.clone()),
            Some(serde_json::Value::Object(o)) => {
                o.get("name").and_then(|n| n.as_str()).map(str::to_owned)
            }
            _ => None,
        };
        work.abstract_text = self
            .descriptions
            .iter()
            .find(|d| d.description_type.as_deref() == Some("Abstract"))
            .and_then(|d| d.description.as_deref())
            .map(|d| tidy_abstract(&strip_markup(d)))
            .filter(|d| !d.is_empty());
        work.url = self.url.clone();
        work.cited_by_count = self.citation_count;
        work
    }
}

impl Creator {
    fn person(&self) -> Option<Person> {
        let mut person = Person::default();
        match (&self.family_name, &self.name) {
            (Some(family), _) => {
                person.family = Some(family.clone());
                person.given = self.given_name.clone();
            }
            (None, Some(name)) => person.literal = Some(name.clone()),
            (None, None) => return None,
        }
        person.orcid = self
            .name_identifiers
            .iter()
            .find(|n| n.name_identifier_scheme.as_deref() == Some("ORCID"))
            .and_then(|n| n.name_identifier.as_deref())
            .map(|o| o.trim_start_matches("https://orcid.org/").to_owned());
        Some(person)
    }
}

/// `resourceTypeGeneral` first (DataCite's controlled vocabulary), then
/// the CSL type DataCite derives.
fn work_type(types: &Types) -> Option<WorkType> {
    let general = types.resource_type_general.as_deref().and_then(|g| {
        Some(match g {
            "JournalArticle" => WorkType::JournalArticle,
            "ConferencePaper" => WorkType::ConferencePaper,
            "Preprint" => WorkType::Preprint,
            "Book" => WorkType::Book,
            "BookChapter" => WorkType::Chapter,
            "Dissertation" => WorkType::Thesis,
            "Report" => WorkType::Report,
            "Dataset" => WorkType::Dataset,
            "Software" => WorkType::Software,
            "PeerReview" => WorkType::Other("peer-review".into()),
            _ => return None,
        })
    });
    general.or_else(|| types.citeproc.as_deref().map(csl::work_type))
}

/// The DataCite `resource-type-id` filter value for a [`WorkType`].
pub(crate) fn resource_type_id(t: &WorkType) -> Option<&str> {
    Some(match t {
        WorkType::JournalArticle => "journal-article",
        WorkType::ConferencePaper => "conference-paper",
        WorkType::Preprint => "preprint",
        WorkType::Book => "book",
        WorkType::Chapter => "book-chapter",
        WorkType::Thesis => "dissertation",
        WorkType::Report => "report",
        WorkType::Dataset => "dataset",
        WorkType::Software => "software",
        _ => return None,
    })
}
