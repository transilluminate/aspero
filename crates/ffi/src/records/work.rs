//! Works as found: their authors, notices (retractions, corrections) and
//! open access, and back to the engine's work for writing out.

use std::time::SystemTime;

use aspero_core::{SourceRecord, UpdateKind};
use aspero_engine::Candidate as CoreCandidate;

use super::time;

#[derive(Debug, Clone, uniffi::Record)]
pub struct Author {
    pub name: String,
    /// The bare ORCID iD.
    pub orcid: Option<String>,
    /// The family name: as the provider gave it, or split from `name`
    /// ("van Gogh"). Absent for an organisation.
    pub family: Option<String>,
    /// The given names, where known.
    pub given: Option<String>,
    /// As Vancouver writes it: "Laan DV"; an organisation as `name`.
    pub short: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Work {
    /// The provider the record came from.
    pub source: String,
    pub ids: Vec<String>,
    /// The work's one identifier, the same whichever identifier found it:
    /// for deduplicating.
    pub canonical_id: Option<String>,
    pub title: Option<String>,
    pub authors: Vec<Author>,
    /// `2015`, `2015-05` or `2015-05-27`.
    pub issued: Option<String>,
    pub year: Option<i16>,
    /// The CSL type: `article-journal`, `book`, ….
    pub work_type: Option<String>,
    /// Journal, proceedings or book title.
    pub container: Option<String>,
    pub volume: Option<String>,
    pub issue: Option<String>,
    pub page: Option<String>,
    pub publisher: Option<String>,
    pub url: Option<String>,
    pub abstract_text: Option<String>,
    /// Retractions, corrections and other notices about the work.
    pub notices: Vec<Notice>,
    /// When the work is itself a notice, the works it updates.
    pub notice_for: Vec<NoticeFor>,
    pub open_access: Option<OpenAccess>,
    /// Works citing this one, as the source counts them (counts differ
    /// between sources; a verified work merged from several shows
    /// OpenAlex's count when one has it, else the largest). DataCite counts
    /// only the citations it tracks, so gives none rather than 0.
    #[uniffi(default = None)]
    pub cited_by_count: Option<u64>,
    pub fetched_at: SystemTime,
    /// Everything known, as the JSON contract's full `work`.
    pub json: String,
    /// When the work was read at its `url`, ISO 8601: a web page's
    /// access date (`webPageWork`'s), exported and cited with it.
    #[uniffi(default = None)]
    pub accessed: Option<String>,
    /// The work's language, as a language tag (`en`, `en-GB`).
    #[uniffi(default = None)]
    pub language: Option<String>,
}

impl Work {
    pub(crate) fn from_record(record: &SourceRecord) -> Self {
        let w = &record.work;
        Work {
            source: record.source.to_string(),
            ids: record.ids.iter().map(ToString::to_string).collect(),
            canonical_id: aspero_core::canonical(&record.ids).map(ToString::to_string),
            title: w.title.clone(),
            authors: w
                .authors
                .iter()
                .filter_map(|p| {
                    let parts = p.parts();
                    Some(Author {
                        name: p.display_name()?,
                        orcid: p.orcid.clone(),
                        family: parts.as_ref().map(|(f, _)| f.clone()),
                        given: parts.and_then(|(_, g)| g),
                        short: p.short()?,
                    })
                })
                .collect(),
            issued: w.issued.map(|d| d.to_string()),
            year: w.issued.map(|d| d.year()),
            work_type: w.work_type.as_ref().map(|t| t.as_csl().to_owned()),
            container: w.container_title.clone(),
            volume: w.volume.clone(),
            issue: w.issue.clone(),
            page: w.page.clone(),
            publisher: w.publisher.clone(),
            url: w.url.clone(),
            abstract_text: w.abstract_text.clone(),
            notices: w.updates.iter().map(Notice::from).collect(),
            notice_for: w.update_to.iter().map(NoticeFor::from).collect(),
            open_access: w.open_access.as_ref().map(OpenAccess::from),
            cited_by_count: w.cited_by_count,
            fetched_at: time(record.fetched_at),
            json: aspero_json::work(record, true).to_string(),
            accessed: w.accessed.map(|d| d.to_string()),
            language: w.language.clone(),
        }
    }

    /// A candidate's work, with every identifier and notice its merged
    /// records gave.
    pub(super) fn from_candidate(c: &CoreCandidate) -> Self {
        Work::from_record(&c.merged_record())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum NoticeKind {
    Retraction,
    Withdrawal,
    ExpressionOfConcern,
    Correction,
    /// A newer version or edition supersedes the work; the notice is the
    /// newer work.
    NewVersion,
    Other {
        name: String,
    },
}

impl From<&UpdateKind> for NoticeKind {
    fn from(kind: &UpdateKind) -> Self {
        match kind {
            UpdateKind::Retraction => NoticeKind::Retraction,
            UpdateKind::Withdrawal => NoticeKind::Withdrawal,
            UpdateKind::ExpressionOfConcern => NoticeKind::ExpressionOfConcern,
            UpdateKind::Correction => NoticeKind::Correction,
            UpdateKind::NewVersion => NoticeKind::NewVersion,
            other => NoticeKind::Other {
                name: other.as_str().to_owned(),
            },
        }
    }
}

/// An editorial notice about a work.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Notice {
    pub kind: NoticeKind,
    /// The notice's own identifier (for a new version, the newer work's).
    pub notice: Option<String>,
    pub date: Option<String>,
    /// Who reported it: `retraction-watch`, `publisher`, `openalex`, ….
    pub source: Option<String>,
}

impl From<&aspero_core::Update> for Notice {
    fn from(u: &aspero_core::Update) -> Self {
        Notice {
            kind: (&u.kind).into(),
            notice: u.notice.as_ref().map(ToString::to_string),
            date: u.date.map(|d| d.to_string()),
            source: u.source.clone(),
        }
    }
}

/// A notice's link to the work it updates.
#[derive(Debug, Clone, uniffi::Record)]
pub struct NoticeFor {
    pub kind: NoticeKind,
    pub work: String,
    pub date: Option<String>,
}

impl From<&aspero_core::UpdateTo> for NoticeFor {
    fn from(u: &aspero_core::UpdateTo) -> Self {
        NoticeFor {
            kind: (&u.kind).into(),
            work: u.work.to_string(),
            date: u.date.map(|d| d.to_string()),
        }
    }
}

/// Whether, and where, a work can be read free and legally.
#[derive(Debug, Clone, uniffi::Record)]
pub struct OpenAccess {
    pub is_oa: bool,
    /// `gold`, `diamond`, `hybrid`, `bronze`, `green` or `closed`.
    pub status: Option<String>,
    pub pdf_url: Option<String>,
    pub url: Option<String>,
    pub license: Option<String>,
}

impl From<&aspero_core::OpenAccess> for OpenAccess {
    fn from(oa: &aspero_core::OpenAccess) -> Self {
        OpenAccess {
            is_oa: oa.is_oa,
            status: oa.status.clone(),
            pdf_url: oa.pdf_url.clone(),
            url: oa.url.clone(),
            license: oa.license.clone(),
        }
    }
}

impl Work {
    /// The work and its identifiers as found, for writing out.
    pub(crate) fn to_core(&self) -> (aspero_core::Work, Vec<aspero_core::WorkId>) {
        let mut w = aspero_core::Work::default();
        w.title.clone_from(&self.title);
        // The full JSON keeps family and given names apart.
        let json: Option<serde_json::Value> = serde_json::from_str(&self.json).ok();
        w.authors = json
            .and_then(|j| serde_json::from_value(j.get("authors")?.clone()).ok())
            .unwrap_or_else(|| {
                self.authors
                    .iter()
                    .map(|a| match &a.family {
                        Some(family) => {
                            let mut p = aspero_core::Person::default();
                            p.family = Some(family.clone());
                            p.given.clone_from(&a.given);
                            p.orcid.clone_from(&a.orcid);
                            p
                        }
                        None => aspero_core::Person::literal(&a.name),
                    })
                    .collect()
            });
        w.issued = self
            .issued
            .as_deref()
            .and_then(|d| d.parse().ok())
            .or_else(|| self.year.map(aspero_core::PartialDate::from_year));
        w.work_type = self
            .work_type
            .as_deref()
            .map(aspero_core::WorkType::from_csl);
        w.container_title.clone_from(&self.container);
        w.volume.clone_from(&self.volume);
        w.issue.clone_from(&self.issue);
        w.page.clone_from(&self.page);
        w.publisher.clone_from(&self.publisher);
        w.url.clone_from(&self.url);
        w.accessed = self.accessed.as_deref().and_then(|d| d.parse().ok());
        w.language.clone_from(&self.language);
        let ids = self.ids.iter().filter_map(|id| id.parse().ok()).collect();
        (w, ids)
    }
}
