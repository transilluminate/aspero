//! CSL-JSON items, as served by doi.org content negotiation and used for
//! citation export/import.
//!
//! Reading is lenient, because real producers deviate from the spec:
//! `title`/`container-title` arrive as a string or an array; Crossref's
//! "CSL" uses its own type names (`journal-article`) rather than CSL's
//! (`article-journal`); abstracts may be JATS XML. Unknown fields are
//! ignored (the raw response is kept elsewhere).

use serde::Deserialize;

use crate::id::{Doi, WorkId};
use crate::work::{PartialDate, Person, Update, UpdateKind, Work, WorkType, tidy_abstract};

/// One CSL-JSON item.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
#[non_exhaustive]
pub struct CslItem {
    #[serde(rename = "type")]
    pub item_type: Option<String>,
    pub title: Option<OneOrMany>,
    pub container_title: Option<OneOrMany>,
    pub author: Vec<CslName>,
    pub issued: Option<CslDate>,
    pub published: Option<CslDate>,
    pub publisher: Option<String>,
    pub volume: Option<Scalar>,
    pub issue: Option<Scalar>,
    pub page: Option<Scalar>,
    #[serde(rename = "DOI")]
    pub doi: Option<String>,
    #[serde(rename = "URL")]
    pub url: Option<String>,
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
    pub is_referenced_by_count: Option<u64>,
    /// Crossref-registered DOIs carry their notices here (see
    /// [`CrossrefUpdate`]).
    pub updated_by: Vec<CrossrefUpdate>,
}

/// One entry of Crossref's `updated-by`: a notice about the work, e.g.
/// `{"DOI": "10.1016/…", "type": "retraction", "source":
/// "retraction-watch", "updated": {"date-parts": [[2010, 2, 6]]}}`. The
/// same shape appears in Crossref's REST API and its CSL via doi.org.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CrossrefUpdate {
    #[serde(rename = "DOI")]
    pub doi: Option<String>,
    #[serde(rename = "type")]
    pub update_type: Option<String>,
    pub source: Option<String>,
    pub updated: Option<CslDate>,
}

impl CrossrefUpdate {
    pub fn to_update(&self) -> Option<Update> {
        let mut update = Update::new(UpdateKind::parse(self.update_type.as_deref()?));
        update.notice = self
            .doi
            .as_deref()
            .and_then(|d| d.parse::<Doi>().ok())
            .map(WorkId::Doi);
        update.date = self.updated.as_ref().and_then(CslDate::to_partial);
        update.source = self.source.clone();
        Some(update)
    }
}

/// A string, or an array of strings of which the first is used.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    pub fn first(&self) -> Option<&str> {
        match self {
            OneOrMany::One(s) => Some(s),
            OneOrMany::Many(v) => v.first().map(String::as_str),
        }
    }
}

/// CSL allows numbers or strings for volume, issue and page.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Scalar {
    Text(String),
    Number(i64),
}

impl Scalar {
    fn text(&self) -> String {
        match self {
            Scalar::Text(s) => s.clone(),
            Scalar::Number(n) => n.to_string(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CslName {
    pub family: Option<String>,
    pub given: Option<String>,
    pub literal: Option<String>,
    #[serde(rename = "ORCID")]
    pub orcid: Option<String>,
}

/// `{"date-parts": [[2015, 5, 27]]}`; parts may be missing, null or strings.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct CslDate {
    pub date_parts: Vec<Vec<Option<serde_json::Value>>>,
}

impl CslDate {
    pub fn to_partial(&self) -> Option<PartialDate> {
        let parts = self.date_parts.first()?;
        let get = |i: usize| -> Option<i64> {
            match parts.get(i)?.as_ref()? {
                serde_json::Value::Number(n) => n.as_i64(),
                serde_json::Value::String(s) => s.trim().parse().ok(),
                _ => None,
            }
        };
        let year = i16::try_from(get(0)?).ok()?;
        let month = get(1).and_then(|m| u8::try_from(m).ok());
        let day = month.and(get(2)).and_then(|d| u8::try_from(d).ok());
        PartialDate::new(year, month, day)
            .or_else(|| PartialDate::new(year, month, None))
            .or(Some(PartialDate::from_year(year)))
    }
}

impl CslName {
    fn person(&self) -> Option<Person> {
        let mut person = Person::default();
        match (&self.family, &self.literal) {
            (Some(family), _) => {
                person.family = Some(family.clone());
                person.given = self.given.clone();
            }
            (None, Some(literal)) => person.literal = Some(literal.clone()),
            (None, None) => person.literal = Some(self.given.clone()?),
        }
        person.orcid = self.orcid.as_deref().map(|o| {
            o.trim_start_matches("https://orcid.org/")
                .trim_start_matches("http://orcid.org/")
                .to_owned()
        });
        Some(person)
    }
}

/// CSL type names, plus Crossref's own vocabulary (which its "CSL" uses).
pub fn work_type(name: &str) -> WorkType {
    match name {
        "journal-article" => WorkType::JournalArticle,
        "proceedings-article" => WorkType::ConferencePaper,
        "posted-content" => WorkType::Preprint,
        "book-chapter" | "book-section" | "book-part" => WorkType::Chapter,
        "monograph" | "edited-book" | "reference-book" => WorkType::Book,
        "dissertation" => WorkType::Thesis,
        csl => WorkType::from_csl(csl),
    }
}

/// Strips tags from a JATS/HTML fragment and decodes the basic entities.
pub fn strip_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl CslItem {
    pub fn to_work(&self) -> Work {
        let first = |t: &Option<OneOrMany>| t.as_ref().and_then(|t| t.first()).map(str::to_owned);
        Work {
            title: first(&self.title),
            authors: self.author.iter().filter_map(CslName::person).collect(),
            issued: self
                .issued
                .as_ref()
                .and_then(CslDate::to_partial)
                .or_else(|| self.published.as_ref().and_then(CslDate::to_partial)),
            work_type: self.item_type.as_deref().map(work_type),
            container_title: first(&self.container_title),
            publisher: self.publisher.clone(),
            volume: self.volume.as_ref().map(Scalar::text),
            issue: self.issue.as_ref().map(Scalar::text),
            page: self.page.as_ref().map(Scalar::text),
            abstract_text: self
                .abstract_text
                .as_deref()
                .map(|a| tidy_abstract(&strip_markup(a)))
                .filter(|a| !a.is_empty()),
            url: self.url.clone(),
            cited_by_count: self.is_referenced_by_count,
            updates: self
                .updated_by
                .iter()
                .filter_map(CrossrefUpdate::to_update)
                .collect(),
            other_years: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(json: serde_json::Value) -> CslItem {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn reads_crossref_flavoured_csl() {
        let w = item(serde_json::json!({
            "type": "journal-article",
            "title": "Deep learning",
            "container-title": "Nature",
            "author": [{"given": "Yann", "family": "LeCun", "sequence": "first"}],
            "issued": {"date-parts": [[2015, 5, 27]]},
            "volume": "521", "page": "436-444",
            "is-referenced-by-count": 79265
        }))
        .to_work();
        assert_eq!(w.work_type, Some(WorkType::JournalArticle));
        assert_eq!(w.issued.unwrap().to_string(), "2015-05-27");
        assert_eq!(w.authors[0].display_name().as_deref(), Some("Yann LeCun"));
        assert_eq!(w.cited_by_count, Some(79265));
    }

    #[test]
    fn reads_retraction_watch_notices() {
        let w = item(serde_json::json!({
            "title": "RETRACTED: Ileal-lymphoid-nodular hyperplasia",
            "updated-by": [
                {"DOI": "10.1016/s0140-6736(04)15715-2", "type": "correction", "source": "retraction-watch",
                 "updated": {"date-parts": [[2004, 3, 6]]}},
                {"DOI": "10.1016/s0140-6736(10)60175-4", "type": "retraction", "source": "retraction-watch",
                 "updated": {"date-parts": [[2010, 2, 6]]}}
            ]
        }))
        .to_work();
        assert_eq!(w.updates.len(), 2);
        let r = w.retraction().unwrap();
        assert_eq!(r.kind, UpdateKind::Retraction);
        assert_eq!(r.date.unwrap().to_string(), "2010-02-06");
        assert_eq!(
            r.notice.as_ref().unwrap().to_string(),
            "doi:10.1016/s0140-6736(10)60175-4"
        );
        assert_eq!(r.source.as_deref(), Some("retraction-watch"));
    }

    #[test]
    fn tolerates_arrays_numbers_strings_and_markup() {
        let w = item(serde_json::json!({
            "type": "article",
            "title": ["Attention Is All You Need", "ignored"],
            "container-title": [],
            "issued": {"date-parts": [["2017"]]},
            "volume": 30,
            "author": [{"literal": "Google Brain"}],
            "abstract": "<jats:title>Abstract</jats:title><jats:p>The dominant &amp; best.</jats:p>"
        }))
        .to_work();
        assert_eq!(w.title.as_deref(), Some("Attention Is All You Need"));
        assert_eq!(w.container_title, None);
        assert_eq!(w.issued.unwrap().year(), 2017);
        assert_eq!(w.volume.as_deref(), Some("30"));
        assert_eq!(w.work_type, Some(WorkType::Preprint));
        assert_eq!(w.abstract_text.as_deref(), Some("The dominant & best."));
    }
}
