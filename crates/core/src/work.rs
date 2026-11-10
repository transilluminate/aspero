//! The minimal, provider-neutral description of a scholarly work.
//!
//! Shaped after CSL-JSON so export and citation formatting stay cheap, but it
//! is not CSL-JSON itself: identifiers live on [`SourceRecord`](crate::SourceRecord),
//! and anything source-specific stays in the provider's raw response.
//!
//! Every field is optional, because no provider fills everything. The structs
//! are `#[non_exhaustive]`: build them from `Default` and assign fields, so
//! adding a field later is not a breaking change.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Work {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// In byline order. Empty means "not provided", not "anonymous".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<Person>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issued: Option<PartialDate>,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub work_type: Option<WorkType>,
    /// Journal, proceedings or book title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    #[serde(default, rename = "abstract", skip_serializing_if = "Option::is_none")]
    pub abstract_text: Option<String>,
    /// Landing page for the work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// As counted by the providing source; counts differ between sources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cited_by_count: Option<u64>,
    /// Retractions, corrections and other editorial notices about the work.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub updates: Vec<Update>,
    /// When the work is itself a notice, the works it updates: what an
    /// erratum corrects or a retraction notice retracts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub update_to: Vec<UpdateTo>,
    /// Whether, and where, the work can be read free and legally.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_access: Option<OpenAccess>,
    /// Years of other editions or versions of the same work, when the
    /// provider knows them (a book's later editions). A citation of any of
    /// them is a citation of this work.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub other_years: Vec<i16>,
}

/// An editorial notice about a work: retraction, correction, expression of
/// concern, withdrawal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Update {
    pub kind: UpdateKind,
    /// The notice's own identifier (usually a DOI), if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice: Option<crate::id::WorkId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<PartialDate>,
    /// Who reported it: "retraction-watch", "publisher", "openalex", ….
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// The date the source gives for the update (Crossref's `updated`,
    /// often the issue's), when the notice appeared earlier: `date` is then
    /// the earliest known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deposited: Option<PartialDate>,
}

impl Update {
    pub fn new(kind: UpdateKind) -> Self {
        Update {
            kind,
            notice: None,
            date: None,
            source: None,
            deposited: None,
        }
    }
}

/// Whether, and where, a work can be read free and legally (as with
/// Unpaywall, whose data OpenAlex carries).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct OpenAccess {
    /// Free to read somewhere.
    pub is_oa: bool,
    /// Unpaywall's classes: `gold`, `diamond`, `hybrid`, `bronze` (free on
    /// the publisher's site without a licence), `green` (a repository
    /// copy), `closed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// The best free copy as a PDF.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdf_url: Option<String>,
    /// The best free copy's page (HTML full text or a landing page).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// That copy's licence, e.g. `cc-by`; none for bronze.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// `publishedVersion`, `acceptedVersion` or `submittedVersion`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Who said so: "openalex", "europepmc", ….
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// A notice's link to the work it updates (Crossref's `update-to`): the
/// other direction of an [`Update`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UpdateTo {
    pub kind: UpdateKind,
    /// The updated work.
    pub work: crate::id::WorkId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<PartialDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl UpdateTo {
    pub fn new(kind: UpdateKind, work: crate::id::WorkId) -> Self {
        UpdateTo {
            kind,
            work,
            date: None,
            source: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UpdateKind {
    Retraction,
    Withdrawal,
    ExpressionOfConcern,
    Correction,
    /// A newer version or edition supersedes the work (Cochrane's
    /// `.pub7` after `.pub6`), or a preprint's published version; the
    /// update's `notice` is the newer work.
    NewVersion,
    Other(String),
}

impl UpdateKind {
    pub fn as_str(&self) -> &str {
        match self {
            UpdateKind::Retraction => "retraction",
            UpdateKind::Withdrawal => "withdrawal",
            UpdateKind::ExpressionOfConcern => "expression_of_concern",
            UpdateKind::Correction => "correction",
            UpdateKind::NewVersion => "new_version",
            UpdateKind::Other(s) => s,
        }
    }

    /// From Crossref/Retraction Watch type names (`retraction`,
    /// `expression_of_concern`, `erratum`, …); never fails.
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().replace('-', "_").as_str() {
            "retraction" | "partial_retraction" => UpdateKind::Retraction,
            "withdrawal" | "removal" => UpdateKind::Withdrawal,
            "expression_of_concern" => UpdateKind::ExpressionOfConcern,
            "correction" | "erratum" | "corrigendum" | "addendum" => UpdateKind::Correction,
            "new_version" | "new_edition" => UpdateKind::NewVersion,
            _ => UpdateKind::Other(s.to_owned()),
        }
    }

    /// Whether citing the work is unsafe: retracted or withdrawn.
    pub fn is_retraction(&self) -> bool {
        matches!(self, UpdateKind::Retraction | UpdateKind::Withdrawal)
    }
}

impl Serialize for UpdateKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for UpdateKind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(UpdateKind::parse(&String::deserialize(d)?))
    }
}

impl Work {
    /// The first retraction or withdrawal notice, if any.
    pub fn retraction(&self) -> Option<&Update> {
        self.updates.iter().find(|u| u.kind.is_retraction())
    }

    /// Types a work that is itself a notice (it has `update_to`) as one:
    /// `retraction` for retractions and withdrawals, `erratum` for other
    /// notices, as OpenAlex names them. Registries type notices as plain
    /// articles. New versions are works in their own right and keep theirs.
    pub fn type_as_notice(&mut self) {
        let kinds = || self.update_to.iter().map(|t| &t.kind);
        let name = if kinds().any(UpdateKind::is_retraction) {
            "retraction"
        } else if kinds().any(|k| *k != UpdateKind::NewVersion) {
            "erratum"
        } else {
            return;
        };
        self.work_type = Some(WorkType::Other(name.into()));
    }

    /// The newer version superseding this work, if one is known.
    pub fn newer_version(&self) -> Option<&crate::id::WorkId> {
        self.updates
            .iter()
            .filter(|u| u.kind == UpdateKind::NewVersion)
            .find_map(|u| u.notice.as_ref())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Person {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub given: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// A name that was not split into given/family, or an organisation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub literal: Option<String>,
    /// Bare ORCID iD, e.g. `0000-0002-1825-0097`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orcid: Option<String>,
}

impl Person {
    pub fn new(given: impl Into<String>, family: impl Into<String>) -> Self {
        Person {
            given: Some(given.into()),
            family: Some(family.into()),
            ..Person::default()
        }
    }

    pub fn literal(name: impl Into<String>) -> Self {
        Person {
            literal: Some(name.into()),
            ..Person::default()
        }
    }

    /// The best single-string name: "Given Family", or the literal.
    pub fn display_name(&self) -> Option<String> {
        match (&self.given, &self.family, &self.literal) {
            (Some(g), Some(f), _) => Some(format!("{g} {f}")),
            (None, Some(f), _) => Some(f.clone()),
            (_, _, Some(l)) => Some(l.clone()),
            (Some(g), None, None) => Some(g.clone()),
            (None, None, None) => None,
        }
    }
}

/// Removes an "Abstract" heading that some sources leave at the start of
/// abstract text (JATS `<title>Abstract</title>`, kept as a word by
/// OpenAlex). Only a standalone heading: "Abstraction is…" is untouched.
pub fn tidy_abstract(text: &str) -> String {
    let text = text.trim();
    for heading in ["Abstract", "ABSTRACT"] {
        if let Some(rest) = text.strip_prefix(heading)
            && rest.starts_with(|c: char| c.is_whitespace() || matches!(c, ':' | '.' | '-' | '—'))
        {
            return rest
                .trim_start_matches(|c: char| {
                    c.is_whitespace() || matches!(c, ':' | '.' | '-' | '—')
                })
                .to_owned();
        }
    }
    text.to_owned()
}

/// A date known to year, month or day precision. Serialises as `2017`,
/// `2017-06` or `2017-06-12`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PartialDate {
    year: i16,
    month: Option<u8>,
    day: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid date {0:?}: expected YYYY, YYYY-MM or YYYY-MM-DD")]
pub struct DateError(String);

impl PartialDate {
    pub fn from_year(year: i16) -> Self {
        PartialDate {
            year,
            month: None,
            day: None,
        }
    }

    /// `None` if the month or day is out of range for the calendar.
    pub fn new(year: i16, month: Option<u8>, day: Option<u8>) -> Option<Self> {
        match (month, day) {
            (None, Some(_)) => return None,
            (Some(m), None) if !(1..=12).contains(&m) => return None,
            (Some(m), Some(d)) => {
                jiff::civil::Date::new(year, m as i8, d as i8).ok()?;
            }
            _ => {}
        }
        Some(PartialDate { year, month, day })
    }

    pub fn year(&self) -> i16 {
        self.year
    }

    pub fn month(&self) -> Option<u8> {
        self.month
    }

    pub fn day(&self) -> Option<u8> {
        self.day
    }
}

impl FromStr for PartialDate {
    type Err = DateError;

    fn from_str(s: &str) -> Result<Self, DateError> {
        let err = || DateError(s.to_owned());
        let mut parts = s.trim().splitn(3, '-');
        let mut num = |len: usize| -> Result<Option<u16>, DateError> {
            match parts.next() {
                None => Ok(None),
                Some(p) if p.len() == len && p.bytes().all(|b| b.is_ascii_digit()) => {
                    Ok(Some(p.parse().map_err(|_| err())?))
                }
                Some(_) => Err(err()),
            }
        };
        let year = num(4)?.ok_or_else(err)?;
        let month = num(2)?;
        let day = num(2)?;
        PartialDate::new(year as i16, month.map(|m| m as u8), day.map(|d| d as u8)).ok_or_else(err)
    }
}

impl fmt::Display for PartialDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}", self.year)?;
        if let Some(m) = self.month {
            write!(f, "-{m:02}")?;
        }
        if let Some(d) = self.day {
            write!(f, "-{d:02}")?;
        }
        Ok(())
    }
}

/// Kind of work, as CSL types. Unknown upstream types are kept verbatim in
/// `Other` rather than dropped.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WorkType {
    JournalArticle,
    ConferencePaper,
    /// CSL `article`: preprints and other unreviewed articles.
    Preprint,
    Book,
    Chapter,
    Thesis,
    Report,
    Dataset,
    Software,
    Review,
    Other(String),
}

impl WorkType {
    /// The CSL type name.
    pub fn as_csl(&self) -> &str {
        match self {
            WorkType::JournalArticle => "article-journal",
            WorkType::ConferencePaper => "paper-conference",
            WorkType::Preprint => "article",
            WorkType::Book => "book",
            WorkType::Chapter => "chapter",
            WorkType::Thesis => "thesis",
            WorkType::Report => "report",
            WorkType::Dataset => "dataset",
            WorkType::Software => "software",
            WorkType::Review => "review",
            WorkType::Other(s) => s,
        }
    }

    /// Inverse of [`WorkType::as_csl`]; never fails.
    pub fn from_csl(s: &str) -> Self {
        match s {
            "article-journal" => WorkType::JournalArticle,
            "paper-conference" => WorkType::ConferencePaper,
            "article" => WorkType::Preprint,
            "book" => WorkType::Book,
            "chapter" => WorkType::Chapter,
            "thesis" => WorkType::Thesis,
            "report" => WorkType::Report,
            "dataset" => WorkType::Dataset,
            "software" => WorkType::Software,
            "review" => WorkType::Review,
            other => WorkType::Other(other.to_owned()),
        }
    }
}

impl Serialize for WorkType {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_csl())
    }
}

impl<'de> Deserialize<'de> for WorkType {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(WorkType::from_csl(&String::deserialize(d)?))
    }
}

impl Serialize for PartialDate {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for PartialDate {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_dates_parse_validate_and_round_trip() {
        for s in ["2017", "2017-06", "2017-06-12", "2024-02-29"] {
            assert_eq!(s.parse::<PartialDate>().unwrap().to_string(), s);
        }
        for s in [
            "17",
            "2017-6",
            "2017-13",
            "2017-06-31",
            "2023-02-29",
            "2017--01",
            "x",
        ] {
            assert!(s.parse::<PartialDate>().is_err(), "{s:?}");
        }
        assert_eq!(PartialDate::new(2017, None, Some(3)), None);
    }

    #[test]
    fn work_type_keeps_unknown_values() {
        assert_eq!(
            WorkType::from_csl("article-journal"),
            WorkType::JournalArticle
        );
        let other = WorkType::from_csl("musical-score");
        assert_eq!(other.as_csl(), "musical-score");
    }

    #[test]
    fn work_serialises_sparsely_with_csl_names() {
        let w = Work {
            title: Some("Attention Is All You Need".into()),
            issued: Some(PartialDate::from_year(2017)),
            work_type: Some(WorkType::ConferencePaper),
            authors: vec![Person::new("Ashish", "Vaswani")],
            ..Work::default()
        };
        let json = serde_json::to_value(&w).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "title": "Attention Is All You Need",
                "authors": [{"given": "Ashish", "family": "Vaswani"}],
                "issued": "2017",
                "type": "paper-conference",
            })
        );
        assert_eq!(serde_json::from_value::<Work>(json).unwrap(), w);
    }

    #[test]
    fn tidy_abstract_drops_only_a_standalone_heading() {
        assert_eq!(
            tidy_abstract("Abstract Proteins are essential"),
            "Proteins are essential"
        );
        assert_eq!(tidy_abstract("ABSTRACT: We show"), "We show");
        assert_eq!(
            tidy_abstract("  Abstraction is useful"),
            "Abstraction is useful"
        );
        assert_eq!(tidy_abstract("We show"), "We show");
    }

    #[test]
    fn person_display_name_prefers_split_names() {
        assert_eq!(
            Person::new("Ada", "Lovelace").display_name().unwrap(),
            "Ada Lovelace"
        );
        assert_eq!(Person::literal("CERN").display_name().unwrap(), "CERN");
        assert_eq!(Person::default().display_name(), None);
    }
}
