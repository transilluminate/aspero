//! What calls return, as UniFFI records and enums: mirrors of the engine's
//! types, converted at the boundary so the engine stays free of FFI.
//!
//! Records carry what an app shows; `Work::json` has everything else, in
//! the JSON contract's shape. Identifiers are strings with their prefix
//! (`doi:10.1038/nature14539`), dates ISO 8601 as precise as known.

use std::collections::HashMap;
use std::time::SystemTime;

use aspero_core::{
    Agreement as CoreAgreement, Capabilities, Filter, ProviderId, Sort, SourceRecord, UpdateKind,
};
use aspero_engine::{
    self as engine, Candidate as CoreCandidate, Disagreement as CoreDisagreement, InputLookup,
    LookupResult, Outcome, Served,
};
use aspero_json::{InputClaim, Status};
use jiff::Timestamp;

fn time(t: Timestamp) -> SystemTime {
    SystemTime::from(t)
}

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
    /// One identifier per work, whichever found it: for deduplicating.
    pub canonical: Option<String>,
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
    /// between sources). DataCite counts only the citations it tracks, so
    /// gives none rather than 0.
    #[uniffi(default = None)]
    pub cited_by_count: Option<u64>,
    pub fetched_at: SystemTime,
    /// Everything known, as the JSON contract's full `work`.
    pub json: String,
}

impl Work {
    pub(crate) fn from_record(record: &SourceRecord) -> Self {
        let w = &record.work;
        Work {
            source: record.source.to_string(),
            ids: record.ids.iter().map(ToString::to_string).collect(),
            canonical: aspero_core::canonical(&record.ids).map(|id| id.to_string()),
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
        }
    }

    /// A candidate's work, with every identifier and notice its merged
    /// records gave.
    fn from_candidate(c: &CoreCandidate) -> Self {
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

/// What went wrong, exactly; new kinds may be added (`Other`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ErrorKind {
    RateLimited,
    /// The provider's usage budget is spent until it resets.
    BudgetSpent,
    Network,
    Timeout,
    Server,
    /// Offline, and not in the cache.
    NotCached,
    /// Skipped for a while after repeated failures.
    CircuitOpen,
    /// The API key was rejected.
    KeyRejected,
    NotFound,
    BadRequest,
    Unsupported,
    /// The answer couldn't be understood.
    Decode,
    InvalidId,
    Other,
}

impl From<aspero_core::ErrorKind> for ErrorKind {
    fn from(kind: aspero_core::ErrorKind) -> Self {
        use aspero_core::ErrorKind as K;
        match kind {
            K::RateLimited => ErrorKind::RateLimited,
            K::BudgetSpent => ErrorKind::BudgetSpent,
            K::Network => ErrorKind::Network,
            K::Timeout => ErrorKind::Timeout,
            K::Server => ErrorKind::Server,
            K::NotCached => ErrorKind::NotCached,
            K::CircuitOpen => ErrorKind::CircuitOpen,
            K::KeyRejected => ErrorKind::KeyRejected,
            K::NotFound => ErrorKind::NotFound,
            K::BadRequest => ErrorKind::BadRequest,
            K::Unsupported => ErrorKind::Unsupported,
            K::Decode => ErrorKind::Decode,
            K::InvalidId => ErrorKind::InvalidId,
            _ => ErrorKind::Other,
        }
    }
}

/// A provider's failure.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Failure {
    pub provider: Option<String>,
    pub kind: ErrorKind,
    /// In English.
    pub message: String,
}

impl From<&engine::EngineError> for Failure {
    fn from(e: &engine::EngineError) -> Self {
        let provider = match e {
            engine::EngineError::Provider { provider, .. } => Some(provider.to_string()),
            _ => None,
        };
        Failure {
            provider,
            kind: e.kind().into(),
            message: e.to_string(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ProviderUsage {
    pub provider: String,
    /// Spent by the call (OpenAlex: US dollars).
    pub cost: Option<f64>,
    /// Left in the budget period.
    pub remaining: Option<f64>,
    pub resets_at: Option<SystemTime>,
}

pub(crate) fn usage(usage: &[(aspero_core::ProviderId, aspero_core::Usage)]) -> Vec<ProviderUsage> {
    usage
        .iter()
        .map(|(p, u)| ProviderUsage {
            provider: p.to_string(),
            cost: u.cost,
            remaining: u.remaining,
            resets_at: u.resets_at.map(time),
        })
        .collect()
}

// Lookups.

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LookupStatus {
    Found,
    NotFound,
    /// Offline, and no provider's answer was cached.
    NotCached,
    /// No provider looks up this kind of identifier.
    Unsupported,
    /// Every provider asked failed: see `attempts`.
    Failed,
    /// Not an identifier: see `error`.
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Freshness {
    Live,
    /// Cached, within its time to live.
    Cache,
    /// Cached and expired, served because the provider failed.
    Stale,
}

impl From<Served> for Freshness {
    fn from(s: Served) -> Self {
        match s {
            Served::Cache => Freshness::Cache,
            Served::Stale => Freshness::Stale,
            _ => Freshness::Live,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AttemptOutcome {
    Found,
    NotFound,
    Failed,
}

/// One provider asked.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Attempt {
    pub provider: String,
    pub outcome: AttemptOutcome,
    /// For failures.
    pub kind: Option<ErrorKind>,
    pub message: Option<String>,
}

/// Where a record contradicts its DOI's registry.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Disagreement {
    /// `issued`, `volume`, `page` or `title`.
    pub field: String,
    pub value: String,
    pub registry: String,
    pub registry_value: String,
}

impl From<&CoreDisagreement> for Disagreement {
    fn from(d: &CoreDisagreement) -> Self {
        Disagreement {
            field: d.field.to_owned(),
            value: d.value.clone(),
            registry: d.registry.to_string(),
            registry_value: d.registry_value.clone(),
        }
    }
}

/// One identifier's lookup.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Lookup {
    /// As given.
    pub input: String,
    /// As parsed, with its prefix.
    pub id: Option<String>,
    pub status: LookupStatus,
    pub work: Option<Work>,
    /// For works found.
    pub freshness: Option<Freshness>,
    pub attempts: Vec<Attempt>,
    /// Why the input isn't an identifier, or isn't supported.
    pub error: Option<String>,
    /// Found only with the DOI's trailing punctuation kept.
    pub with_punctuation: bool,
    /// With `full`: the latest version, if superseded.
    pub latest: Option<Work>,
    /// With `full`.
    pub disagreements: Vec<Disagreement>,
}

impl Lookup {
    pub(crate) fn new(input: &str, lookup: &InputLookup) -> Self {
        let looked = match lookup {
            InputLookup::Invalid(e) => {
                return Lookup {
                    input: input.to_owned(),
                    id: None,
                    status: LookupStatus::Invalid,
                    work: None,
                    freshness: None,
                    attempts: Vec::new(),
                    error: Some(e.to_string()),
                    with_punctuation: false,
                    latest: None,
                    disagreements: Vec::new(),
                };
            }
            InputLookup::Looked(looked) => looked,
        };
        let result: &LookupResult = &looked.result;
        let status = match Status::of(result) {
            Status::Found => LookupStatus::Found,
            Status::Unsupported => LookupStatus::Unsupported,
            Status::NotCached => LookupStatus::NotCached,
            Status::Failed(_) => LookupStatus::Failed,
            _ => LookupStatus::NotFound,
        };
        let freshness = result.served().map(Freshness::from);
        let attempts = result
            .attempts
            .iter()
            .map(|a| {
                let (outcome, failure) = match &a.outcome {
                    Outcome::Found => (AttemptOutcome::Found, None),
                    Outcome::Failed(e) => (AttemptOutcome::Failed, Some(e)),
                    _ => (AttemptOutcome::NotFound, None),
                };
                Attempt {
                    provider: a.provider.to_string(),
                    outcome,
                    kind: failure.map(|e| e.kind().into()),
                    message: failure.map(ToString::to_string),
                }
            })
            .collect();
        let mut work = result.record.as_ref().map(Work::from_record);
        if let (Some(work), Some(found)) = (&mut work, &looked.abstract_text) {
            work.abstract_text = found.as_ref().map(|a| a.text.clone());
        }
        Lookup {
            input: input.to_owned(),
            id: Some(result.id.to_string()),
            status,
            work,
            freshness,
            attempts,
            error: result.unsupported(),
            with_punctuation: looked.with_punctuation,
            latest: looked.latest.as_ref().map(Work::from_record),
            disagreements: looked.disagreements.iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Lookups {
    /// One per input, in order.
    pub results: Vec<Lookup>,
    pub usage: Vec<ProviderUsage>,
}

// Verifications.

/// A citation to verify: free text, fields, or both. Fields are more
/// reliable than text, which is parsed for them.
#[derive(Debug, Clone, Default, uniffi::Record)]
pub struct Claim {
    /// The reference in any style; identifiers in it count as claimed.
    #[uniffi(default = None)]
    pub text: Option<String>,
    #[uniffi(default = None)]
    pub title: Option<String>,
    /// Names as written: "Vaswani, A.", "Ashish Vaswani".
    #[uniffi(default = [])]
    pub authors: Vec<String>,
    #[uniffi(default = None)]
    pub year: Option<i16>,
    /// The journal (or other container), full or abbreviated.
    #[uniffi(default = None)]
    pub container: Option<String>,
    #[uniffi(default = None)]
    pub volume: Option<String>,
    /// The first page, or a range.
    #[uniffi(default = None)]
    pub page: Option<String>,
    /// Identifiers in any form `lookup` takes.
    #[uniffi(default = [])]
    pub ids: Vec<String>,
    /// The app's own reference, echoed back.
    #[uniffi(default = None)]
    pub key: Option<String>,
    /// Other readings of claimed identifiers (a `Reference`'s), tried only
    /// if the claimed one doesn't exist.
    #[uniffi(default = [])]
    pub alternative_ids: Vec<IdAlternative>,
}

/// Another reading of a claimed identifier: a DOI broken across lines at
/// a hyphen, without the hyphen.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct IdAlternative {
    pub claimed: String,
    pub alternative: String,
}

impl From<&aspero_core::Alternative> for IdAlternative {
    fn from(a: &aspero_core::Alternative) -> Self {
        IdAlternative {
            claimed: a.claimed.to_string(),
            alternative: a.alternative.to_string(),
        }
    }
}

impl Claim {
    /// A claim as aspero read it, for `Verification.read`.
    pub(crate) fn read(c: &aspero_core::Claim, key: Option<String>) -> Self {
        Claim {
            text: c.text.clone(),
            title: c.title.clone(),
            authors: c.authors.clone(),
            year: c.year,
            container: c.container.clone(),
            volume: c.volume.clone(),
            page: c.page.clone(),
            ids: c.ids.iter().map(ToString::to_string).collect(),
            key,
            alternative_ids: c.alternatives.iter().map(Into::into).collect(),
        }
    }

    pub(crate) fn into_input(self) -> InputClaim {
        let fields = aspero_json::ClaimInput {
            text: self.text,
            title: self.title,
            authors: self.authors,
            year: self.year,
            container: self.container,
            volume: self.volume,
            page: self.page,
            ids: self.ids,
            ..Default::default()
        };
        let input = fields.text.clone().unwrap_or_else(|| fields.describe());
        let alternatives: Vec<aspero_core::Alternative> = self
            .alternative_ids
            .iter()
            .filter_map(|a| {
                Some(aspero_core::Alternative::new(
                    a.claimed.parse().ok()?,
                    a.alternative.parse().ok()?,
                ))
            })
            .collect();
        InputClaim {
            input,
            key: self.key.map(serde_json::Value::String),
            claim: fields.into_claim().map(|mut c| {
                c.alternatives = alternatives;
                c
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Verdict {
    /// A work matching the claim exists (and so do its identifiers).
    Verified,
    /// A work partly matching the claim exists; check the details.
    Plausible,
    /// The claimed identifier is wrong, or the closest work differs.
    Mismatch,
    NotFound,
    /// Providers failed, so nothing conclusive can be said.
    Unverifiable,
    /// The claim couldn't be read.
    Invalid,
}

impl From<engine::Verdict> for Verdict {
    fn from(v: engine::Verdict) -> Self {
        match v {
            engine::Verdict::Verified => Verdict::Verified,
            engine::Verdict::Plausible => Verdict::Plausible,
            engine::Verdict::Mismatch => Verdict::Mismatch,
            engine::Verdict::NotFound => Verdict::NotFound,
            _ => Verdict::Unverifiable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CitedField {
    Title,
    Authors,
    Year,
    Journal,
    Volume,
    FirstPage,
    /// The work is a reply, correction or commentary on the one cited;
    /// `actual` says which ("correction", "reply", "comment", …).
    Kind,
}

/// A cited field the work disagrees with, and the work's value when worth
/// showing (year, journal, volume, first page).
#[derive(Debug, Clone, uniffi::Record)]
pub struct Difference {
    pub field: CitedField,
    pub actual: Option<String>,
    /// The citation's value, as compared: the year, volume or first page
    /// read from it, or the title, authors or journal given. `nil` when
    /// free text was searched for the work's own.
    pub cited: Option<String>,
    /// For `CitedField.kind`: what the work is, as a value (`actual` in
    /// English).
    pub kind: Option<WorkKind>,
}

/// What a reply, correction or commentary is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum WorkKind {
    Reply,
    Correction,
    Comment,
    Retraction,
    ExpressionOfConcern,
    Recommendation,
}

impl From<aspero_core::matching::WorkKind> for WorkKind {
    fn from(k: aspero_core::matching::WorkKind) -> Self {
        use aspero_core::matching::WorkKind as K;
        match k {
            K::Reply => WorkKind::Reply,
            K::Correction => WorkKind::Correction,
            K::Comment => WorkKind::Comment,
            K::Retraction => WorkKind::Retraction,
            K::ExpressionOfConcern => WorkKind::ExpressionOfConcern,
            _ => WorkKind::Recommendation,
        }
    }
}

impl From<engine::CitedField> for CitedField {
    fn from(f: engine::CitedField) -> Self {
        match f {
            engine::CitedField::Title => CitedField::Title,
            engine::CitedField::Authors => CitedField::Authors,
            engine::CitedField::Year => CitedField::Year,
            engine::CitedField::Journal => CitedField::Journal,
            engine::CitedField::Volume => CitedField::Volume,
            engine::CitedField::Kind => CitedField::Kind,
            _ => CitedField::FirstPage,
        }
    }
}

impl From<&engine::Difference> for Difference {
    fn from(d: &engine::Difference) -> Self {
        Difference {
            field: d.field.into(),
            actual: d.actual.clone(),
            cited: d.cited.clone(),
            kind: d.kind.map(Into::into),
        }
    }
}

/// Why a verdict was reached. "The work" is the verification's `best`;
/// "the identifiers" are those the claim gave.
// Records cross the boundary by value; UniFFI can't box them.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, uniffi::Enum)]
pub enum Reason {
    /// The citation matches the work.
    Matches,
    /// Never empty.
    PartlyMatches {
        differs: Vec<Difference>,
    },
    /// The citation agrees with the work as far as it goes, but gives too
    /// little to confirm it.
    Unconfirmed,
    /// The closest work shares the title, but other details differ.
    /// Never empty.
    Closest {
        differs: Vec<Difference>,
    },
    NoMatch,
    /// The identifiers exist; the claim gives nothing else to compare.
    IdExists,
    /// The identifiers exist and the citation agrees.
    IdAgrees,
    /// Never empty.
    IdPartlyMatches {
        differs: Vec<Difference>,
    },
    /// The identifiers are `cited`; the citation matches the work.
    IdElsewhere {
        cited: Candidate,
    },
    /// The identifiers' work (the work) doesn't match the citation, and no
    /// other does.
    IdDisagrees,
    /// The identifiers don't exist; the citation matches the work.
    IdMissingElsewhere,
    IdMissing,
    ProvidersFailed,
    /// The claim couldn't be read; `why` in English.
    InvalidClaim {
        why: String,
    },
}

impl Reason {
    /// `why`, its cited candidate one of `candidates`.
    fn new(why: &engine::Reason, candidates: &[CoreCandidate]) -> Self {
        let differs = || why.differs().iter().map(Into::into).collect();
        match why {
            engine::Reason::Matches => Reason::Matches,
            engine::Reason::PartlyMatches { .. } => Reason::PartlyMatches { differs: differs() },
            engine::Reason::Unconfirmed => Reason::Unconfirmed,
            engine::Reason::Closest { .. } => Reason::Closest { differs: differs() },
            engine::Reason::NoMatch => Reason::NoMatch,
            engine::Reason::IdExists => Reason::IdExists,
            engine::Reason::IdAgrees => Reason::IdAgrees,
            engine::Reason::IdPartlyMatches { .. } => {
                Reason::IdPartlyMatches { differs: differs() }
            }
            engine::Reason::IdElsewhere { cited } => Reason::IdElsewhere {
                cited: Candidate::from(&candidates[*cited]),
            },
            engine::Reason::IdDisagrees => Reason::IdDisagrees,
            engine::Reason::IdMissingElsewhere => Reason::IdMissingElsewhere,
            engine::Reason::IdMissing => Reason::IdMissing,
            _ => Reason::ProvidersFailed,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Agreement {
    Match,
    Partial,
    Mismatch,
    /// The claim or the record doesn't say.
    Unknown,
}

impl From<CoreAgreement> for Agreement {
    fn from(a: CoreAgreement) -> Self {
        match a {
            CoreAgreement::Match => Agreement::Match,
            CoreAgreement::Partial => Agreement::Partial,
            CoreAgreement::Mismatch => Agreement::Mismatch,
            _ => Agreement::Unknown,
        }
    }
}

/// How each cited field compares with a candidate.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Checks {
    pub title: Agreement,
    pub authors: Agreement,
    pub year: Agreement,
    pub container: Agreement,
    pub volume: Agreement,
    pub page: Agreement,
    pub ids: Agreement,
}

/// A work considered for a claim, scored against it.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Candidate {
    /// 0 to 1.
    pub confidence: f64,
    pub checks: Checks,
    /// The providers that found it.
    pub found_by: Vec<String>,
    pub work: Work,
}

impl From<&CoreCandidate> for Candidate {
    fn from(c: &CoreCandidate) -> Self {
        let a = &c.assessment;
        let mut found_by: Vec<String> = Vec::new();
        for s in &c.sightings {
            let p = s.provider.to_string();
            if !found_by.contains(&p) {
                found_by.push(p);
            }
        }
        Candidate {
            confidence: a.confidence,
            checks: Checks {
                title: a.title.agreement.into(),
                authors: a.authors.agreement.into(),
                year: a.year.agreement.into(),
                container: a.container.agreement.into(),
                volume: a.volume.agreement.into(),
                page: a.page.agreement.into(),
                ids: a.ids.into(),
            },
            found_by,
            work: Work::from_candidate(c),
        }
    }
}

/// One claim's verification.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Verification {
    /// The claim's position in the call's `claims`.
    pub index: u32,
    /// The claim's text, or its fields on one line.
    pub input: String,
    /// The claim's `key`.
    pub key: Option<String>,
    pub verdict: Verdict,
    pub why: Reason,
    /// One sentence in English: `why` and `notes`.
    pub reason: String,
    /// About `best`, whatever the verdict: its retraction or withdrawal,
    /// and its newer version.
    pub notes: Vec<Notice>,
    /// The claim as read: identifiers as written (with the other readings
    /// tried in `alternativeIds`), and from free text the year, volume and
    /// first page. `nil` for an invalid claim.
    pub read: Option<Claim>,
    /// The work the verdict is about: the match, or the best guess.
    pub best: Option<Candidate>,
    /// With `full`: every candidate, most confident first.
    pub candidates: Vec<Candidate>,
    /// Providers that failed, searching or looking up the claim's
    /// identifiers; what they'd have found is unknown.
    pub failures: Vec<Failure>,
}

impl Verification {
    pub(crate) fn new(
        index: usize,
        claim: &InputClaim,
        v: &engine::Verification,
        full: bool,
    ) -> Self {
        Verification {
            index: index as u32,
            input: claim.input.clone(),
            key: key(claim),
            verdict: v.verdict.into(),
            why: Reason::new(&v.why, &v.report.candidates),
            reason: v.reason.clone(),
            notes: v.notes.iter().map(Notice::from).collect(),
            read: Some(Claim::read(&v.read, key(claim))),
            best: v.best.map(|i| Candidate::from(&v.report.candidates[i])),
            candidates: match full {
                true => v.report.candidates.iter().map(Candidate::from).collect(),
                false => Vec::new(),
            },
            failures: v.report.failures().iter().map(Failure::from).collect(),
        }
    }

    pub(crate) fn invalid(index: usize, claim: &InputClaim, why: &str) -> Self {
        Verification {
            index: index as u32,
            input: claim.input.clone(),
            key: key(claim),
            verdict: Verdict::Invalid,
            why: Reason::InvalidClaim {
                why: why.to_owned(),
            },
            reason: why.to_owned(),
            notes: Vec::new(),
            read: None,
            best: None,
            candidates: Vec::new(),
            failures: Vec::new(),
        }
    }
}

fn key(claim: &InputClaim) -> Option<String> {
    claim.key.as_ref().map(|k| match k {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Verifications {
    /// In claim order.
    pub results: Vec<Verification>,
    pub usage: Vec<ProviderUsage>,
}

// Scanning and health.

/// A UTF-16 range, for `NSString` and `NSTextView`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct Utf16Range {
    pub start: u64,
    pub end: u64,
}

/// An identifier mentioned in text.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Mention {
    /// With its prefix.
    pub id: String,
    /// The prefix alone: `doi`, `pmid`, `arxiv`, ….
    pub kind: String,
    /// As written, label included (`PMID: 123`).
    pub text: String,
    /// 1-based.
    pub line: u64,
    /// 1-based, in characters.
    pub column: u64,
    pub utf16: Utf16Range,
    /// For a DOI broken across lines at a hyphen, the DOI without it: look
    /// it up if `id` isn't found.
    pub alternative_id: Option<String>,
}

impl Mention {
    pub(crate) fn new(text: &str, m: &aspero_core::id::Mention, utf16: &mut Utf16Offsets) -> Self {
        Mention {
            id: m.id.to_string(),
            kind: m.id.kind().prefix().to_owned(),
            text: text[m.span.clone()].to_owned(),
            line: m.line as u64,
            column: m.column as u64,
            utf16: utf16.range(&m.span),
            alternative_id: m.alternative(text).map(|id| id.to_string()),
        }
    }
}

/// One reference from a pasted list.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Reference {
    /// On one line, without its number or bullet: a claim for `verify`.
    pub text: String,
    /// Where it is in the list, marker excluded.
    pub utf16: Utf16Range,
    /// DOIs broken at a hyphen, read without it: pass them on as the
    /// claim's `alternativeIds`.
    pub alternative_ids: Vec<IdAlternative>,
}

impl Reference {
    pub(crate) fn new(r: &aspero_core::Reference, utf16: &mut Utf16Offsets) -> Self {
        Reference {
            text: r.text.clone(),
            utf16: utf16.range(&r.span),
            alternative_ids: r.alternatives.iter().map(Into::into).collect(),
        }
    }
}

/// A line of a reference list as laid out: its text and its distance from
/// the left edge (PDFKit's x-offset, in any unit).
#[derive(Debug, Clone, uniffi::Record)]
pub struct Line {
    pub text: String,
    pub indent: f64,
}

/// Line indices, `end` exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct LineRange {
    pub start: u64,
    pub end: u64,
}

/// One reference from laid-out lines.
#[derive(Debug, Clone, uniffi::Record)]
pub struct LineReference {
    /// On one line, without its number or bullet: a claim for `verify`.
    pub text: String,
    /// The lines it spans.
    pub lines: LineRange,
    /// Pass on as the claim's `alternativeIds`.
    pub alternative_ids: Vec<IdAlternative>,
}

impl From<&aspero_core::LineReference> for LineReference {
    fn from(r: &aspero_core::LineReference) -> Self {
        LineReference {
            text: r.text.clone(),
            lines: LineRange {
                start: r.lines.start as u64,
                end: r.lines.end as u64,
            },
            alternative_ids: r.alternatives.iter().map(Into::into).collect(),
        }
    }
}

/// Byte offsets in a text as UTF-16 ones, counted on from the last one
/// asked for: spans in order cost one pass over the text.
pub(crate) struct Utf16Offsets<'a> {
    text: &'a str,
    byte: usize,
    utf16: u64,
}

impl<'a> Utf16Offsets<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        Utf16Offsets {
            text,
            byte: 0,
            utf16: 0,
        }
    }

    fn at(&mut self, byte: usize) -> u64 {
        if byte < self.byte {
            // Out of order: count from the start again.
            *self = Utf16Offsets::new(self.text);
        }
        self.utf16 += self.text[self.byte..byte].encode_utf16().count() as u64;
        self.byte = byte;
        self.utf16
    }

    fn range(&mut self, span: &std::ops::Range<usize>) -> Utf16Range {
        Utf16Range {
            start: self.at(span.start),
            end: self.at(span.end),
        }
    }
}

/// A provider refusing calls for now.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Unavailable {
    pub provider: String,
    pub kind: ErrorKind,
    pub message: String,
    /// When it will be tried again; never, for a rejected key.
    pub until: Option<SystemTime>,
}

// Settings.

/// What a provider last said was left of its budget.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Budget {
    pub provider: String,
    /// OpenAlex: US dollars.
    pub remaining: Option<f64>,
    pub resets_at: Option<SystemTime>,
    /// When the provider said so.
    pub seen_at: SystemTime,
}

impl Budget {
    pub(crate) fn new(provider: &aspero_core::ProviderId, b: &engine::Budget) -> Self {
        Budget {
            provider: provider.to_string(),
            remaining: b.remaining,
            resets_at: b.resets_at.map(time),
            seen_at: time(b.seen_at),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum KeyStatus {
    /// Accepted (even if its budget is spent: see `budget`).
    Valid,
    Rejected,
    /// OpenAlex couldn't be reached, so the key is untested.
    Unreachable,
}

impl From<engine::KeyStatus> for KeyStatus {
    fn from(s: engine::KeyStatus) -> Self {
        match s {
            engine::KeyStatus::Valid => KeyStatus::Valid,
            engine::KeyStatus::Rejected => KeyStatus::Rejected,
            _ => KeyStatus::Unreachable,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct KeyCheck {
    pub status: KeyStatus,
    /// Why, in English, unless valid.
    pub message: Option<String>,
    /// What the key has left.
    pub budget: Option<Budget>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct CacheStats {
    pub records: u64,
    pub identifiers: u64,
    /// Remembered "not found" answers.
    pub misses: u64,
    pub searches: u64,
}

// Searching.

#[derive(Debug, Clone, uniffi::Record)]
pub struct SearchRequest {
    /// Keywords or a title.
    pub text: String,
    #[uniffi(default = None)]
    pub year_from: Option<i16>,
    #[uniffi(default = None)]
    pub year_to: Option<i16>,
    /// A CSL type: `article-journal`, `book`, ….
    #[uniffi(default = None)]
    pub work_type: Option<String>,
    #[uniffi(default = false)]
    pub open_access_only: bool,
    #[uniffi(default = 10)]
    pub limit: u32,
    /// A provider by name; otherwise the first that searches (OpenAlex,
    /// $0.001 a page).
    #[uniffi(default = None)]
    pub source: Option<String>,
    /// A previous page's `next`, with the same request otherwise. Crossref
    /// sorted by `newest` pages by offset, and orders works sharing a date
    /// differently from one request to the next, so its pages may repeat
    /// or skip such works.
    #[uniffi(default = None)]
    pub cursor: Option<String>,
    /// An author's name. Fields are ANDed with `text`, which may then be
    /// empty; a source that can't search one fails (`unsupported`), as
    /// `searchCapabilities` tells.
    #[uniffi(default = None)]
    pub author: Option<String>,
    /// Words of the title.
    #[uniffi(default = None)]
    pub title: Option<String>,
    /// Where the work appeared: the journal, or the book for a chapter.
    #[uniffi(default = None)]
    pub container: Option<String>,
    /// The order of hits; `None` is relevance. A source without the order
    /// fails (`unsupported`).
    #[uniffi(default = None)]
    pub sort: Option<SearchSort>,
    /// Provider-specific parameters, passed through by name: OpenAlex
    /// `filter` and `sort`, Crossref `query.*`, Europe PMC `query` (its
    /// own syntax), …. A name the source doesn't know fails.
    #[uniffi(default)]
    pub extra: HashMap<String, String>,
}

/// The order of search hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SearchSort {
    /// Best match first; with only fields to go on, the source's own order.
    Relevance,
    /// Most recently published first.
    Newest,
    MostCited,
}

impl SearchSort {
    fn from_core(sort: Sort) -> Option<Self> {
        match sort {
            Sort::Relevance => Some(SearchSort::Relevance),
            Sort::Newest => Some(SearchSort::Newest),
            Sort::MostCited => Some(SearchSort::MostCited),
            _ => None,
        }
    }
}

impl From<SearchSort> for Sort {
    fn from(sort: SearchSort) -> Self {
        match sort {
            SearchSort::Relevance => Sort::Relevance,
            SearchSort::Newest => Sort::Newest,
            SearchSort::MostCited => Sort::MostCited,
        }
    }
}

/// A field a search can name ([`SearchRequest`]'s `author`, `title`,
/// `container`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SearchField {
    Author,
    Title,
    Container,
}

impl SearchField {
    fn from_core(filter: Filter) -> Option<Self> {
        match filter {
            Filter::Author => Some(SearchField::Author),
            Filter::Title => Some(SearchField::Title),
            Filter::Container => Some(SearchField::Container),
            _ => None,
        }
    }
}

/// A filter a search can apply. One a source lacks is ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SearchFilter {
    /// `yearFrom` and `yearTo`.
    Years,
    WorkType,
    /// `openAccessOnly`.
    OpenAccess,
}

impl SearchFilter {
    fn from_core(filter: Filter) -> Option<Self> {
        match filter {
            Filter::Years => Some(SearchFilter::Years),
            Filter::WorkType => Some(SearchFilter::WorkType),
            Filter::OpenAccess => Some(SearchFilter::OpenAccess),
            _ => None,
        }
    }
}

/// What one source can search by.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SourceSearch {
    /// As `SearchRequest.source` and `SearchResults.provider` name it.
    pub source: String,
    pub fields: Vec<SearchField>,
    pub filters: Vec<SearchFilter>,
    pub sorts: Vec<SearchSort>,
}

impl SourceSearch {
    pub(crate) fn new(source: &ProviderId, caps: &Capabilities) -> Self {
        let filters = &caps.search_filters;
        SourceSearch {
            source: source.to_string(),
            fields: filters
                .iter()
                .filter_map(|f| SearchField::from_core(*f))
                .collect(),
            filters: filters
                .iter()
                .filter_map(|f| SearchFilter::from_core(*f))
                .collect(),
            sorts: caps
                .sorts
                .iter()
                .filter_map(|s| SearchSort::from_core(*s))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SearchHit {
    pub work: Work,
    /// The provider's relevance score.
    pub score: Option<f64>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SearchResults {
    /// Who answered.
    pub provider: Option<String>,
    pub hits: Vec<SearchHit>,
    /// Total matches, if the provider reports it.
    pub total: Option<u64>,
    /// The cursor for the next page; none on the last.
    pub next: Option<String>,
    pub freshness: Option<Freshness>,
    /// Why there are no hits, if the search failed.
    pub failure: Option<Failure>,
    pub usage: Vec<ProviderUsage>,
}

impl SearchResults {
    pub(crate) fn new(result: Result<engine::SearchReport, engine::EngineError>) -> Self {
        match result {
            Err(e) => SearchResults {
                provider: None,
                hits: Vec::new(),
                total: None,
                next: None,
                freshness: None,
                failure: Some(Failure::from(&e)),
                usage: Vec::new(),
            },
            Ok(report) => SearchResults {
                provider: Some(report.provider.to_string()),
                hits: report
                    .page
                    .hits
                    .iter()
                    .map(|h| SearchHit {
                        work: Work::from_record(&h.record),
                        score: h.score,
                    })
                    .collect(),
                total: report.page.total,
                next: report.page.next.map(|c| c.0),
                freshness: Some(report.served.into()),
                failure: None,
                usage: report
                    .usage
                    .map(|u| usage(&[(report.provider.clone(), u)]))
                    .unwrap_or_default(),
            },
        }
    }
}

/// A bibliography file's format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BibFormat {
    Bibtex,
    Ris,
    CslJson,
}

impl From<aspero_bib::Format> for BibFormat {
    fn from(f: aspero_bib::Format) -> Self {
        match f {
            aspero_bib::Format::Ris => BibFormat::Ris,
            aspero_bib::Format::CslJson => BibFormat::CslJson,
            _ => BibFormat::Bibtex,
        }
    }
}

/// A bibliography file's entries, each a claim ready to verify.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Bibliography {
    pub format: BibFormat,
    pub entries: Vec<BibEntry>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct BibEntry {
    /// Its fields and identifiers, with `key` the entry's key (an RIS
    /// record or CSL-JSON item without one: its 1-based position).
    pub claim: Claim,
    /// Lowercase, in the file's own vocabulary: `article` (BibTeX),
    /// `jour` (RIS), `article-journal` (CSL-JSON).
    pub entry_type: String,
    /// Problems with the entry's own fields: a `doi` that isn't a DOI.
    pub notes: Vec<BibNote>,
    /// Whether there is anything to verify: a title or an identifier.
    pub checkable: bool,
}

impl From<aspero_bib::BibEntry> for BibEntry {
    fn from(e: aspero_bib::BibEntry) -> Self {
        let checkable = e.is_checkable();
        let c = e.claim;
        BibEntry {
            claim: Claim {
                text: None,
                title: c.title,
                authors: c.authors,
                year: c.year,
                container: c.container,
                volume: c.volume,
                page: c.page,
                ids: c.ids.iter().map(ToString::to_string).collect(),
                key: Some(e.key),
                alternative_ids: Vec::new(),
            },
            entry_type: e.entry_type,
            notes: e.notes.iter().map(Into::into).collect(),
            checkable,
        }
    }
}

/// A problem with one of an entry's own fields.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BibNote {
    /// The field as the file names it: `doi`, `eprint`, `DO`, `ISBN`.
    pub field: String,
    pub value: String,
    pub problem: BibProblem,
    /// In English, for logs.
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BibProblem {
    NotADoi,
    NotAnArxivId,
    NotAnIsbn,
    NotAPmid,
}

impl From<aspero_bib::Problem> for BibProblem {
    fn from(p: aspero_bib::Problem) -> Self {
        use aspero_bib::Problem;
        match p {
            Problem::NotADoi => BibProblem::NotADoi,
            Problem::NotAnArxivId => BibProblem::NotAnArxivId,
            Problem::NotAPmid => BibProblem::NotAPmid,
            _ => BibProblem::NotAnIsbn,
        }
    }
}

impl From<&aspero_bib::Note> for BibNote {
    fn from(n: &aspero_bib::Note) -> Self {
        BibNote {
            field: n.field.clone(),
            value: n.value.clone(),
            problem: n.problem.into(),
            message: n.to_string(),
        }
    }
}

/// Why text couldn't be read as a bibliography file.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum ReadError {
    /// Not BibTeX, RIS or CSL-JSON.
    #[error("not BibTeX, RIS or CSL-JSON")]
    Unrecognised,
    /// The format was recognised, but the file is broken at `line`.
    #[error("{message} (line {line})")]
    Invalid {
        format: BibFormat,
        line: u64,
        message: String,
    },
}

impl From<aspero_bib::ReadError> for ReadError {
    fn from(e: aspero_bib::ReadError) -> Self {
        match e {
            aspero_bib::ReadError::Invalid {
                format,
                line,
                message,
            } => ReadError::Invalid {
                format: format.into(),
                line: line as u64,
                message,
            },
            _ => ReadError::Unrecognised,
        }
    }
}

/// A format to write works in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ExportFormat {
    Bibtex,
    Ris,
}

impl Work {
    /// The work and its identifiers as found, for writing out.
    pub(crate) fn to_core(&self) -> (aspero_core::Work, Vec<aspero_core::WorkId>) {
        let mut w = aspero_core::Work::default();
        w.title = self.title.clone();
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
                            p.given = a.given.clone();
                            p.orcid = a.orcid.clone();
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
        w.container_title = self.container.clone();
        w.volume = self.volume.clone();
        w.issue = self.issue.clone();
        w.page = self.page.clone();
        w.publisher = self.publisher.clone();
        w.url = self.url.clone();
        let ids = self.ids.iter().filter_map(|id| id.parse().ok()).collect();
        (w, ids)
    }
}

/// A bundled citation style.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CitationStyle {
    /// How `StyleSource.bundled` names it: `apa`, `vancouver`, ….
    pub id: String,
    pub title: String,
}

/// Which citation style to format in.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum StyleSource {
    /// A bundled style, by `CitationStyle.id`.
    Bundled { id: String },
    /// An independent style's `.csl` file, as text.
    Csl { xml: String },
}

/// Plain text, or HTML marking italics, bold and small caps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CiteFormat {
    Text,
    Html,
}

/// One work in a formatted reference list.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CitedEntry {
    /// The work's position in the input.
    pub index: u32,
    /// The style's number or label for it ("1.", "[1]"), when the style
    /// sets it apart from the reference.
    pub label: Option<String>,
    pub text: String,
    /// How the style cites it in running text: "(LeCun et al., 2015)",
    /// "(1)".
    pub in_text: String,
}

/// Why a style couldn't be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum StyleError {
    /// No bundled style has this id.
    #[error("no bundled style {id:?}")]
    UnknownStyle { id: String },
    /// A dependent style (a journal's alias): use its `parent` style, by
    /// its id in the CSL repository (`nlm-citation-sequence`).
    #[error("a dependent style; use its parent, {parent}")]
    Dependent { parent: String },
    /// Not a CSL style, or a broken one.
    #[error("not a CSL style: {message}")]
    Invalid { message: String },
}

impl From<aspero_bib::cite::StyleError> for StyleError {
    fn from(e: aspero_bib::cite::StyleError) -> Self {
        use aspero_bib::cite::StyleError as E;
        match e {
            E::Unknown { id } => StyleError::UnknownStyle { id },
            E::Dependent { parent } => StyleError::Dependent { parent },
            E::Invalid { message } => StyleError::Invalid { message },
            _ => StyleError::Invalid {
                message: e.to_string(),
            },
        }
    }
}

/// The citation manager whose fields a Word document holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CitationManager {
    Zotero,
    Mendeley,
}

/// What a Word document cites.
#[derive(Debug, Clone, uniffi::Record)]
pub struct DocxReferences {
    /// Each work cited, once, in order of first citation, its `claim`
    /// ready to verify; `key` is the manager's item key.
    pub entries: Vec<BibEntry>,
    /// Whose citation fields they came from; `nil` when there were none.
    pub manager: Option<CitationManager>,
    /// Citations in the text (one may cite several works).
    pub citations: u32,
    /// The document's text, a paragraph per line: without fields, its
    /// reference list can be split with `splitReferences` instead.
    pub text: String,
}

impl From<aspero_bib::docx::Manager> for CitationManager {
    fn from(m: aspero_bib::docx::Manager) -> Self {
        match m {
            aspero_bib::docx::Manager::Mendeley => CitationManager::Mendeley,
            _ => CitationManager::Zotero,
        }
    }
}

impl From<aspero_bib::docx::DocxReferences> for DocxReferences {
    fn from(d: aspero_bib::docx::DocxReferences) -> Self {
        DocxReferences {
            entries: d.entries.into_iter().map(Into::into).collect(),
            manager: d.manager.map(Into::into),
            citations: d.citations as u32,
            text: d.text,
        }
    }
}

/// What decided which work a document is, surest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum IdentifiedBy {
    /// An identifier on the page, its work's title matching the title given.
    IdentifierAndTitle,
    /// An identifier on the page, its work's title and first author on the
    /// page too.
    IdentifierAndPage,
    /// The page's only identifier, with no title to check it against.
    OnlyIdentifier,
    /// No identifier confirmed: found by searching the title and the top of
    /// the page.
    TitleSearch,
}

impl From<engine::Decision> for IdentifiedBy {
    fn from(d: engine::Decision) -> Self {
        match d {
            engine::Decision::IdentifierAndTitle => IdentifiedBy::IdentifierAndTitle,
            engine::Decision::IdentifierAndPage => IdentifiedBy::IdentifierAndPage,
            engine::Decision::OnlyIdentifier => IdentifiedBy::OnlyIdentifier,
            _ => IdentifiedBy::TitleSearch,
        }
    }
}

/// The work a document is, and how that was decided.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Identification {
    pub decided_by: IdentifiedBy,
    /// The work as a verified citation (plausible when found by title).
    pub verification: Verification,
}

#[cfg(test)]
mod tests {
    use aspero_core::{Claim, FieldSet, ProviderId, Raw, Work as CoreWork};

    use super::*;

    /// A value's variant in snake case, from its `Debug`: `PartlyMatches {
    /// differs: [] }` is `partly_matches`.
    fn name(value: impl std::fmt::Debug) -> String {
        let debug = format!("{value:?}");
        let mut name = String::new();
        for c in debug.chars().take_while(char::is_ascii_alphanumeric) {
            if c.is_ascii_uppercase() && !name.is_empty() {
                name.push('_');
            }
            name.push(c.to_ascii_lowercase());
        }
        name
    }

    /// Engine values reach the app as the case of the same name, not a
    /// catch-all's: each `ALL` lists every value, so a new one is checked
    /// here as soon as it exists.
    #[test]
    fn values_keep_their_names() {
        for v in engine::Verdict::ALL {
            assert_eq!(name(Verdict::from(*v)), v.as_str());
        }
        for f in engine::CitedField::ALL {
            assert_eq!(name(CitedField::from(*f)), f.as_str());
        }
        for d in engine::Decision::ALL {
            assert_eq!(name(IdentifiedBy::from(*d)), d.as_str());
        }
        for s in engine::Served::ALL {
            assert_eq!(name(Freshness::from(*s)), s.as_str());
        }
        for s in engine::KeyStatus::ALL {
            assert_eq!(name(KeyStatus::from(*s)), s.as_str());
        }
        for k in aspero_core::matching::WorkKind::ALL {
            assert_eq!(name(WorkKind::from(*k)), k.as_str().replace(' ', "_"));
        }
        for s in Sort::ALL {
            let sort = SearchSort::from_core(*s).unwrap();
            assert_eq!(name(sort), s.as_str());
            assert_eq!(Sort::from(sort), *s);
        }
        // Each filter is a field or a filter, but for `place`, kept
        // internal.
        for f in Filter::ALL {
            let field = SearchField::from_core(*f).map(name);
            let filter = SearchFilter::from_core(*f).map(name);
            match (field, filter) {
                (Some(n), None) | (None, Some(n)) => assert_eq!(n, f.as_str()),
                (None, None) => assert_eq!(*f, Filter::Place),
                both => panic!("{f:?} maps twice: {both:?}"),
            }
        }

        // `IdElsewhere` cites a candidate.
        let record = SourceRecord::new(
            ProviderId::from_static("crossref"),
            Vec::new(),
            CoreWork::default(),
            FieldSet::ALL,
            Raw {
                media_type: "application/json".into(),
                body: "{}".into(),
            },
            Timestamp::UNIX_EPOCH,
        );
        let candidates = [CoreCandidate {
            assessment: aspero_core::matching::assess(&Claim::default(), &record),
            record,
            ids: Vec::new(),
            sightings: Vec::new(),
            updates: Vec::new(),
        }];
        for r in engine::Reason::ALL {
            assert_eq!(name(Reason::new(r, &candidates)), r.as_str());
        }
    }

    /// aspero-bib's enums have no `ALL`, so only the values known now are
    /// checked.
    #[test]
    fn bib_values_keep_their_names() {
        use aspero_bib::{Format, Problem, docx::Manager};
        for f in [Format::Bibtex, Format::Ris, Format::CslJson] {
            assert_eq!(name(BibFormat::from(f)), f.as_str());
        }
        let problems = [
            Problem::NotADoi,
            Problem::NotAnArxivId,
            Problem::NotAnIsbn,
            Problem::NotAPmid,
        ];
        for p in problems {
            assert_eq!(name(BibProblem::from(p)), p.as_str());
        }
        for m in [Manager::Zotero, Manager::Mendeley] {
            assert_eq!(name(CitationManager::from(m)), name(m));
        }
    }

    #[test]
    fn utf16_offsets_match_counting_from_the_start() {
        // "é" is one UTF-16 unit, "😀" and "𝔸" two, "👩‍⚕️" five.
        let text = "é 😀 doi:10.1/x\n👩‍⚕️ PMID 1 𝔸 end";
        let bytes: Vec<usize> = (0..=text.len())
            .filter(|&b| text.is_char_boundary(b))
            .collect();
        let from_start = |b: usize| text[..b].encode_utf16().count() as u64;
        let mut utf16 = Utf16Offsets::new(text);
        // In order, then out of order.
        for &b in bytes.iter().chain(bytes.iter().rev()) {
            assert_eq!(utf16.at(b), from_start(b), "at byte {b}");
        }
        let mut utf16 = Utf16Offsets::new(text);
        for pair in bytes.windows(2) {
            let span = pair[0]..pair[1];
            let expected = Utf16Range {
                start: from_start(span.start),
                end: from_start(span.end),
            };
            assert_eq!(utf16.range(&span), expected);
        }
    }
}
