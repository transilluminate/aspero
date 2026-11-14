//! What calls return, as UniFFI records and enums: mirrors of the engine's
//! types, converted at the boundary so the engine stays free of FFI.
//!
//! Records carry what an app shows; `Work::json` has everything else, in
//! the JSON contract's shape. Identifiers are strings with their prefix
//! (`doi:10.1038/nature14539`), dates ISO 8601 as precise as known.

use std::collections::HashMap;
use std::time::SystemTime;

use aspero_core::{
    Agreement as CoreAgreement, BooleanSupport, Capabilities, Filter, FilterValue, ProviderId,
    QueryNode, Relation, Sort, SourceRecord, TermField, UpdateKind, WorkType, YearRange,
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
    /// The app's daily cap on the provider's spend (`Config.openalexDailyCap`)
    /// is reached until the budget resets: see `Failure.budgetCap`. Last, so
    /// the cases before keep their places.
    BudgetCapped,
}

impl From<aspero_core::ErrorKind> for ErrorKind {
    fn from(kind: aspero_core::ErrorKind) -> Self {
        use aspero_core::ErrorKind as K;
        match kind {
            K::RateLimited => ErrorKind::RateLimited,
            K::BudgetSpent => ErrorKind::BudgetSpent,
            K::BudgetCapped => ErrorKind::BudgetCapped,
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
    /// For `budgetCapped`: the cap, what's used and when it resets, to
    /// word ("Stopped at your $0.50 daily cap; resets at 01:00").
    #[uniffi(default = None)]
    pub budget_cap: Option<BudgetCap>,
}

/// A daily cap reached: paid calls to the provider stop until `resetsAt`.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct BudgetCap {
    /// The cap (US dollars), as `Config.openalexDailyCap` set it.
    pub cap: f64,
    /// Used today, as the provider reports it: by everything using the key.
    pub used: f64,
    /// When the provider's budget resets (OpenAlex: midnight UTC).
    pub resets_at: Option<SystemTime>,
}

impl From<&engine::EngineError> for Failure {
    fn from(e: &engine::EngineError) -> Self {
        let (provider, budget_cap) = match e {
            engine::EngineError::Provider { provider, error } => {
                (Some(provider.to_string()), BudgetCap::of(error))
            }
            engine::EngineError::Disabled(provider) => (Some(provider.to_string()), None),
            _ => (None, None),
        };
        Failure {
            provider,
            kind: e.kind().into(),
            message: e.to_string(),
            budget_cap,
        }
    }
}

impl BudgetCap {
    pub(crate) fn of(error: &aspero_core::Error) -> Option<BudgetCap> {
        match error {
            aspero_core::Error::BudgetCapped {
                cap,
                used,
                resets_at,
            } => Some(BudgetCap {
                cap: *cap,
                used: *used,
                resets_at: resets_at.map(time),
            }),
            _ => None,
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
        let mut lookup = Lookup::from_result(input, &looked.result);
        if let (Some(work), Some(found)) = (&mut lookup.work, &looked.abstract_text) {
            work.abstract_text = found.as_ref().map(|a| a.text.clone());
        }
        lookup.with_punctuation = looked.with_punctuation;
        lookup.latest = looked.latest.as_ref().map(Work::from_record);
        lookup.disagreements = looked.disagreements.iter().map(Into::into).collect();
        lookup
    }

    /// One identifier's lookup, without extras.
    pub(crate) fn from_result(input: &str, result: &LookupResult) -> Self {
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
        let work = result.record.as_ref().map(Work::from_record);
        Lookup {
            input: input.to_owned(),
            id: Some(result.id.to_string()),
            status,
            work,
            freshness,
            attempts,
            error: result.unsupported(),
            with_punctuation: false,
            latest: None,
            disagreements: Vec::new(),
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
    /// The daily allowance, as the provider says it (OpenAlex: $1 with a
    /// key, $0.10 without). Used today is `limit - remaining`.
    #[uniffi(default = None)]
    pub limit: Option<f64>,
    /// A prepaid balance beside the daily allowance, where the provider
    /// says one.
    #[uniffi(default = None)]
    pub prepaid_remaining: Option<f64>,
}

impl Budget {
    pub(crate) fn new(provider: &aspero_core::ProviderId, b: &engine::Budget) -> Self {
        Budget {
            provider: provider.to_string(),
            remaining: b.remaining,
            resets_at: b.resets_at.map(time),
            seen_at: time(b.seen_at),
            limit: b.limit,
            prepaid_remaining: b.prepaid_remaining,
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
    /// The cache's size on disk (its database pages; the write-ahead log
    /// aside), as `Config.cacheMaxMb` limits it.
    #[uniffi(default = 0)]
    pub bytes: u64,
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
    /// The search as a tree of all, any and none, instead of `text`,
    /// `author`, `title` and `container`: giving it with any of those fails
    /// (`badRequest`). The other filters, the sort, paging, `source` and
    /// `extra` still apply. A tree the source can't say fails
    /// (`unsupported`), as `searchSupports` tells beforehand.
    #[uniffi(default = None)]
    pub query: Option<SearchNode>,
    /// Only works the source added on or after this day, `YYYY-MM-DD`
    /// (first deposited or indexed), whenever they were published: for
    /// watching a search. Without a `source`, the first that can
    /// (`searchCapabilities`' filters hold `since`). A source that can't
    /// fails (`unsupported`) and says why, never ignoring it; a date that
    /// isn't one fails (`badRequest`).
    #[uniffi(default = None)]
    pub since: Option<String>,
}

/// A search as a tree. Groups hold any number of nodes, groups included
/// (where `SourceSearch.nesting`); empty groups are dropped, and an all or
/// any of one node is that node.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum SearchNode {
    /// Every node matches.
    All {
        nodes: Vec<SearchNode>,
    },
    /// At least one node matches (where `SourceSearch.any` allows).
    Any {
        nodes: Vec<SearchNode>,
    },
    /// No node matches (where `SourceSearch.none` allows).
    None {
        nodes: Vec<SearchNode>,
    },
    /// Words in a field, or with `phrase`, the words together in order.
    /// The value is searched as written: quotes and operators in it are
    /// never syntax.
    Term {
        field: SearchField,
        value: String,
        phrase: bool,
    },
    Filter {
        filter: SearchFilterValue,
    },
    /// Works citing, or cited by, the work `id` names: any identifier
    /// `lookup` takes. Aspero looks it up (through the cache) to find the
    /// work at the source; a work it can't find fails the search.
    Relation {
        relation: SearchRelation,
        id: String,
    },
}

impl From<SearchNode> for QueryNode {
    fn from(node: SearchNode) -> Self {
        let nodes = |nodes: Vec<SearchNode>| nodes.into_iter().map(QueryNode::from).collect();
        match node {
            SearchNode::All { nodes: n } => QueryNode::All(nodes(n)),
            SearchNode::Any { nodes: n } => QueryNode::Any(nodes(n)),
            SearchNode::None { nodes: n } => QueryNode::None(nodes(n)),
            SearchNode::Term {
                field,
                value,
                phrase: false,
            } => QueryNode::term(field.into(), value),
            SearchNode::Term { field, value, .. } => QueryNode::phrase(field.into(), value),
            SearchNode::Filter { filter } => QueryNode::Filter(filter.into()),
            SearchNode::Relation { relation, id } => QueryNode::relation(relation.into(), id),
        }
    }
}

/// A filter in a query tree.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum SearchFilterValue {
    /// Published from `from` to `to`, inclusive; either may be open.
    Years { from: Option<i16>, to: Option<i16> },
    /// A CSL type: `article-journal`, `book`, ….
    WorkType { r#type: String },
    /// Whether a free, legal copy exists.
    OpenAccess { is_oa: bool },
    /// Leaves out retracted works.
    NotRetracted,
    /// An ISO 639-1 code, like `en`; aspero gives each source its own form.
    Language { code: String },
    /// Only works with an abstract.
    HasAbstract,
}

impl From<SearchFilterValue> for FilterValue {
    fn from(filter: SearchFilterValue) -> Self {
        match filter {
            SearchFilterValue::Years { from, to } => FilterValue::Years(YearRange { from, to }),
            SearchFilterValue::WorkType { r#type } => {
                FilterValue::WorkType(WorkType::from_csl(&r#type))
            }
            SearchFilterValue::OpenAccess { is_oa } => FilterValue::OpenAccess(is_oa),
            SearchFilterValue::NotRetracted => FilterValue::NotRetracted,
            SearchFilterValue::Language { code } => FilterValue::Language(code),
            SearchFilterValue::HasAbstract => FilterValue::HasAbstract,
        }
    }
}

impl SearchFilterValue {
    #[cfg(test)]
    fn from_core(filter: &FilterValue) -> Option<Self> {
        Some(match filter {
            FilterValue::Years(y) => SearchFilterValue::Years {
                from: y.from,
                to: y.to,
            },
            FilterValue::WorkType(t) => SearchFilterValue::WorkType {
                r#type: t.as_csl().to_owned(),
            },
            FilterValue::OpenAccess(is_oa) => SearchFilterValue::OpenAccess { is_oa: *is_oa },
            FilterValue::NotRetracted => SearchFilterValue::NotRetracted,
            FilterValue::Language(code) => SearchFilterValue::Language { code: code.clone() },
            FilterValue::HasAbstract => SearchFilterValue::HasAbstract,
            _ => return None,
        })
    }
}

/// How one work relates to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SearchRelation {
    /// Works citing the given work.
    Cites,
    /// Works the given work cites.
    CitedBy,
}

impl SearchRelation {
    fn from_core(relation: Relation) -> Option<Self> {
        match relation {
            Relation::Cites => Some(SearchRelation::Cites),
            Relation::CitedBy => Some(SearchRelation::CitedBy),
            _ => None,
        }
    }
}

impl From<SearchRelation> for Relation {
    fn from(relation: SearchRelation) -> Self {
        match relation {
            SearchRelation::Cites => Relation::Cites,
            SearchRelation::CitedBy => Relation::CitedBy,
        }
    }
}

/// How far a source can combine terms with any or none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SearchBoolean {
    No,
    /// Only among terms in one field, or values of one filter.
    WithinField,
    /// Across fields.
    Anywhere,
}

impl SearchBoolean {
    fn from_core(b: BooleanSupport) -> Self {
        match b {
            BooleanSupport::WithinField => SearchBoolean::WithinField,
            BooleanSupport::Anywhere => SearchBoolean::Anywhere,
            _ => SearchBoolean::No,
        }
    }
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
///
/// In a query tree, the rest too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SearchField {
    Author,
    Title,
    Container,
    /// Plain text, wherever the source searches it: every source.
    Keywords,
    Abstract,
    /// An ORCID (bare or as a URL) or an OpenAlex author id (`A…`);
    /// anything else fails (`badRequest`).
    AuthorId,
    /// In a query tree only: the work an identifier names, any `lookup`
    /// takes (a DOI, bare, `doi:` or as a URL; `pmid:`, `pmcid:`,
    /// `arXiv:`, an OpenAlex `W…` id, an ISBN). One that isn't fails
    /// (`badRequest`); `phrase` means nothing. Sources take some kinds
    /// only (Crossref DOIs, Open Library ISBNs, …): `searchSupports` says
    /// which, and why not.
    Identifier,
}

impl SearchField {
    fn from_core(filter: Filter) -> Option<Self> {
        match filter {
            Filter::Author => Some(SearchField::Author),
            Filter::Title => Some(SearchField::Title),
            Filter::Container => Some(SearchField::Container),
            Filter::Keywords => Some(SearchField::Keywords),
            Filter::Abstract => Some(SearchField::Abstract),
            Filter::AuthorId => Some(SearchField::AuthorId),
            Filter::Identifier => Some(SearchField::Identifier),
            _ => None,
        }
    }
}

impl From<SearchField> for TermField {
    fn from(field: SearchField) -> Self {
        match field {
            SearchField::Author => TermField::Author,
            SearchField::Title => TermField::Title,
            SearchField::Container => TermField::Container,
            SearchField::Keywords => TermField::Keywords,
            SearchField::Abstract => TermField::Abstract,
            SearchField::AuthorId => TermField::AuthorId,
            SearchField::Identifier => TermField::Identifier,
        }
    }
}

/// A filter a search can apply. One a source lacks is ignored, but for
/// `since`.
///
/// In a query tree, one a source lacks fails (`unsupported`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SearchFilter {
    /// `yearFrom` and `yearTo`.
    Years,
    WorkType,
    /// `openAccessOnly`.
    OpenAccess,
    /// In a query tree only, as are the rest.
    NotRetracted,
    Language,
    HasAbstract,
    /// `since`, never in a query tree. A source without it fails
    /// (`unsupported`) rather than ignoring it.
    Since,
}

impl SearchFilter {
    fn from_core(filter: Filter) -> Option<Self> {
        match filter {
            Filter::Years => Some(SearchFilter::Years),
            Filter::WorkType => Some(SearchFilter::WorkType),
            Filter::OpenAccess => Some(SearchFilter::OpenAccess),
            Filter::NotRetracted => Some(SearchFilter::NotRetracted),
            Filter::Language => Some(SearchFilter::Language),
            Filter::HasAbstract => Some(SearchFilter::HasAbstract),
            Filter::Since => Some(SearchFilter::Since),
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
    /// How far a query tree's any can reach: `withinField` is terms of one
    /// field (or values of one filter) only.
    pub any: SearchBoolean,
    /// How far a query tree's none can reach.
    pub none: SearchBoolean,
    /// Whether a query tree's groups can hold groups.
    pub nesting: bool,
    pub relations: Vec<SearchRelation>,
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
            any: SearchBoolean::from_core(caps.search_any),
            none: SearchBoolean::from_core(caps.search_none),
            nesting: caps.search_nesting,
            relations: caps
                .relations
                .iter()
                .filter_map(|r| SearchRelation::from_core(*r))
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

// Citation links.

/// Whether a work's references are known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Coverage {
    /// OpenAlex lists the work's references.
    Known,
    /// OpenAlex has no reference data for it (books, older work): it may
    /// cite anything.
    Unknown,
}

impl From<engine::Coverage> for Coverage {
    fn from(c: engine::Coverage) -> Self {
        match c {
            engine::Coverage::Known => Coverage::Known,
            engine::Coverage::Unknown => Coverage::Unknown,
            _ => Coverage::Unknown,
        }
    }
}

/// A page of a work's references.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ReferencesPage {
    /// As given.
    pub input: String,
    /// The work's OpenAlex id (`openalex:W…`).
    pub id: Option<String>,
    /// The work, when it was looked up (not when its references were
    /// cached).
    pub work: Option<Work>,
    pub coverage: Coverage,
    /// How many works it cites; none when unknown.
    pub total: Option<u64>,
    /// This page, in OpenAlex's order (by id), each looked up.
    pub references: Vec<Lookup>,
    /// The cursor for the next page; none on the last.
    pub next_cursor: Option<String>,
    /// Why there's no page: the work couldn't be resolved.
    pub failure: Option<Failure>,
    pub usage: Vec<ProviderUsage>,
}

impl ReferencesPage {
    pub(crate) fn new(
        input: &str,
        result: Result<engine::ReferencesPage, engine::EngineError>,
    ) -> Self {
        match result {
            Err(e) => ReferencesPage {
                input: input.to_owned(),
                id: None,
                work: None,
                coverage: Coverage::Unknown,
                total: None,
                references: Vec::new(),
                next_cursor: None,
                failure: Some(Failure::from(&e)),
                usage: Vec::new(),
            },
            Ok(page) => ReferencesPage {
                input: input.to_owned(),
                id: Some(page.work.to_string()),
                work: page.record.as_ref().map(Work::from_record),
                coverage: page.coverage.into(),
                total: page.total.map(|t| t as u64),
                references: page
                    .references
                    .iter()
                    .map(|r| Lookup::from_result(&r.id.to_string(), r))
                    .collect(),
                next_cursor: page.next_cursor.map(|c| c.0),
                failure: None,
                usage: usage(&page.usage),
            },
        }
    }
}

/// An analysis of a set of works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AnalysisKind {
    /// Which seed cites which retracted or withdrawn work.
    RetractedReferences,
    /// Seeds linked to no other seed.
    OddOneOut,
    /// The best `limit` works several seeds cite that the set lacks.
    ProbablyMissing { limit: u32 },
    /// The best 20 works around the set, by co-citation and bibliographic
    /// coupling (`related`).
    Related,
}

/// Related works listed: as many as `aspero analyse` lists by default.
const RELATED_LIMIT: u32 = 20;

impl From<AnalysisKind> for engine::AnalysisKind {
    fn from(k: AnalysisKind) -> Self {
        match k {
            AnalysisKind::RetractedReferences => engine::AnalysisKind::RetractedReferences,
            AnalysisKind::OddOneOut => engine::AnalysisKind::OddOneOut,
            AnalysisKind::ProbablyMissing { limit } => {
                engine::AnalysisKind::ProbablyMissing { limit }
            }
            AnalysisKind::Related => engine::AnalysisKind::Related {
                limit: RELATED_LIMIT,
            },
        }
    }
}

impl From<engine::AnalysisKind> for AnalysisKind {
    fn from(k: engine::AnalysisKind) -> Self {
        match k {
            engine::AnalysisKind::RetractedReferences => AnalysisKind::RetractedReferences,
            engine::AnalysisKind::OddOneOut => AnalysisKind::OddOneOut,
            engine::AnalysisKind::ProbablyMissing { limit } => {
                AnalysisKind::ProbablyMissing { limit }
            }
            engine::AnalysisKind::Related { .. } => AnalysisKind::Related,
            _ => AnalysisKind::Related,
        }
    }
}

/// Why a seed has no OpenAlex work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SeedProblem {
    /// Not an identifier.
    InvalidId,
    /// OpenAlex can't look up this kind of identifier (an ISBN), and no
    /// record found by it has a DOI or PMID it can.
    UnsupportedId,
    NotFound,
    /// The lookup failed (asked once; seeds aren't retried).
    ProviderFailed,
    /// Not looked up: the budget ran out first.
    OverBudget,
}

impl From<engine::SeedProblem> for SeedProblem {
    fn from(p: engine::SeedProblem) -> Self {
        match p {
            engine::SeedProblem::InvalidId => SeedProblem::InvalidId,
            engine::SeedProblem::UnsupportedId => SeedProblem::UnsupportedId,
            engine::SeedProblem::NotFound => SeedProblem::NotFound,
            engine::SeedProblem::ProviderFailed => SeedProblem::ProviderFailed,
            engine::SeedProblem::OverBudget => SeedProblem::OverBudget,
            _ => SeedProblem::ProviderFailed,
        }
    }
}

/// One work of the set.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Seed {
    /// Its place among the seeds given; findings name seeds by it.
    pub index: u32,
    /// As given.
    pub input: String,
    /// Its OpenAlex id, when resolved.
    pub id: Option<String>,
    pub work: Option<Work>,
    /// For resolved seeds: whether its references are known.
    pub coverage: Option<Coverage>,
    /// How many works it cites, when known.
    pub references: Option<u64>,
    /// Why it isn't resolved.
    pub problem: Option<SeedProblem>,
    /// The problem in English.
    pub message: Option<String>,
    /// The earlier seed that is the same work (a preprint and its
    /// published version): the two count as one.
    pub same_as: Option<u32>,
}

impl Seed {
    fn new(index: usize, s: &engine::Seed) -> Self {
        Seed {
            index: index as u32,
            input: s.input.clone(),
            id: s.work().map(ToString::to_string),
            work: s.record.as_ref().map(Work::from_record),
            coverage: s.coverage().map(Into::into),
            references: s.references().map(|r| r.len() as u64),
            problem: s.problem.map(Into::into),
            message: s.message.clone(),
            same_as: s.same_as.map(|i| i as u32),
        }
    }
}

/// What stopped part of an analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum IncompleteKind {
    /// A fixed cap: related works read a sample of a much cited work's
    /// citing works.
    Capped,
    /// The budget ran out.
    Budget,
    ProviderFailed,
}

impl From<engine::IncompleteKind> for IncompleteKind {
    fn from(k: engine::IncompleteKind) -> Self {
        match k {
            engine::IncompleteKind::Capped => IncompleteKind::Capped,
            engine::IncompleteKind::Budget => IncompleteKind::Budget,
            engine::IncompleteKind::ProviderFailed => IncompleteKind::ProviderFailed,
            _ => IncompleteKind::ProviderFailed,
        }
    }
}

/// How far a stopped step got: `done` of `total` (seeds looked up,
/// references checked, works looked up, citing works read), to word as
/// "checked 1,200 of 4,300".
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct Incomplete {
    pub kind: IncompleteKind,
    pub done: u64,
    pub total: u64,
}

impl From<&engine::Incomplete> for Incomplete {
    fn from(i: &engine::Incomplete) -> Self {
        Incomplete {
            kind: i.kind.into(),
            done: i.done,
            total: i.total,
        }
    }
}

/// A seed citing a retracted or withdrawn work.
#[derive(Debug, Clone, uniffi::Record)]
pub struct RetractedReference {
    pub seed: u32,
    pub cites: Work,
    /// Who says so: `openalex`, or a cached notice's source (`crossref`,
    /// `retraction-watch`, `publisher`).
    pub source: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct RetractedReferences {
    pub items: Vec<RetractedReference>,
    /// What stopped the check, if anything.
    pub incomplete: Option<Incomplete>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum OddReason {
    /// It cites no other seed, none cites it, and it shares no
    /// references with any.
    NoLinks,
    /// Its references are unknown and no other seed cites it.
    NoReferenceData,
}

impl From<engine::OddReason> for OddReason {
    fn from(r: engine::OddReason) -> Self {
        match r {
            engine::OddReason::NoLinks => OddReason::NoLinks,
            engine::OddReason::NoReferenceData => OddReason::NoReferenceData,
            _ => OddReason::NoReferenceData,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct OddOne {
    pub seed: u32,
    pub reason: OddReason,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct OddOnesOut {
    pub items: Vec<OddOne>,
    pub incomplete: Option<Incomplete>,
}

/// A work several seeds cite that the set lacks.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MissingWork {
    /// Its OpenAlex id.
    pub id: String,
    /// Every OpenAlex id merged into it (preprint and published versions).
    pub ids: Vec<String>,
    /// When looked up.
    pub work: Option<Work>,
    /// The seeds citing it, one per work of the set.
    pub cited_by: Vec<u32>,
    pub count: u32,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ProbablyMissing {
    pub items: Vec<MissingWork>,
    pub incomplete: Option<Incomplete>,
}

/// A work around the set, and why: co-cited with it, sharing its
/// references, or both.
#[derive(Debug, Clone, uniffi::Record)]
pub struct RelatedWork {
    /// Its OpenAlex id.
    pub id: String,
    /// Every OpenAlex id merged into it (preprint and published versions).
    pub ids: Vec<String>,
    /// When looked up.
    pub work: Option<Work>,
    /// From 0 to 1: its co-citation strength (each citing work counting
    /// 1/√ of its references) and coupling count, each as a share of the
    /// highest among the works found, weighted 0.8 and 0.2.
    pub score: f64,
    /// Citing works read that cite it and at least one seed.
    pub co_cited_with: u32,
    /// The seeds' references it cites too (counted from two).
    pub shared_references: u32,
    /// The seeds linking it, one per work of the set.
    pub seeds: Vec<u32>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct RelatedWorks {
    pub items: Vec<RelatedWork>,
    /// What stopped it: `capped` when a much cited seed's citing works
    /// were sampled; `done` of `total` citing works read.
    pub incomplete: Option<Incomplete>,
}

/// What an analysis would cost.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct Estimate {
    /// Paid requests.
    pub requests: u64,
    pub usd: f64,
    /// Whether the counts are known: otherwise a floor (seeds not
    /// resolved), or with related works a ceiling (their citing works'
    /// pages are exact; coupling's pages and the lookups at most).
    pub exact: bool,
}

/// Analyses of a set of works.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Analysis {
    /// One per seed given, in order.
    pub seeds: Vec<Seed>,
    /// Works of the set (seeds merged) with known references: what "cited
    /// by k of n" counts out of.
    pub with_references: u32,
    pub retracted_references: Option<RetractedReferences>,
    pub odd_one_out: Option<OddOnesOut>,
    pub probably_missing: Option<ProbablyMissing>,
    /// Kinds asked for that aren't built yet (none, now).
    pub not_yet: Vec<AnalysisKind>,
    /// With `estimateOnly`: the cost of running it, in place of findings.
    pub estimate: Option<Estimate>,
    /// Every seed settled and every analysis run to the end.
    pub complete: bool,
    /// Resolving the seeds stopped early.
    pub incomplete: Option<Incomplete>,
    /// Why there's no analysis at all (no OpenAlex).
    pub failure: Option<Failure>,
    pub usage: Vec<ProviderUsage>,
    /// Works around the set, when asked for (`related`).
    #[uniffi(default = None)]
    pub related: Option<RelatedWorks>,
}

impl Analysis {
    pub(crate) fn new(result: Result<engine::Analysis, engine::EngineError>) -> Self {
        let a = match result {
            Ok(a) => a,
            Err(e) => {
                return Analysis {
                    seeds: Vec::new(),
                    with_references: 0,
                    retracted_references: None,
                    odd_one_out: None,
                    probably_missing: None,
                    not_yet: Vec::new(),
                    estimate: None,
                    complete: false,
                    incomplete: None,
                    failure: Some(Failure::from(&e)),
                    usage: Vec::new(),
                    related: None,
                };
            }
        };
        let incomplete = |i: &Option<engine::Incomplete>| i.as_ref().map(Incomplete::from);
        Analysis {
            seeds: a
                .seeds
                .iter()
                .enumerate()
                .map(|(i, s)| Seed::new(i, s))
                .collect(),
            with_references: a.with_references as u32,
            retracted_references: a
                .retracted_references
                .as_ref()
                .map(|s| RetractedReferences {
                    items: s
                        .items
                        .iter()
                        .map(|r| RetractedReference {
                            seed: r.seed as u32,
                            cites: Work::from_record(&r.cites),
                            source: r.source.clone(),
                        })
                        .collect(),
                    incomplete: incomplete(&s.incomplete),
                }),
            odd_one_out: a.odd_one_out.as_ref().map(|s| OddOnesOut {
                items: s
                    .items
                    .iter()
                    .map(|o| OddOne {
                        seed: o.seed as u32,
                        reason: o.reason.into(),
                    })
                    .collect(),
                incomplete: incomplete(&s.incomplete),
            }),
            probably_missing: a.probably_missing.as_ref().map(|s| ProbablyMissing {
                items: s
                    .items
                    .iter()
                    .map(|m| MissingWork {
                        id: m.id.to_string(),
                        ids: m.ids.iter().map(ToString::to_string).collect(),
                        work: m.record.as_ref().map(Work::from_record),
                        cited_by: m.cited_by.iter().map(|&i| i as u32).collect(),
                        count: m.count() as u32,
                    })
                    .collect(),
                incomplete: incomplete(&s.incomplete),
            }),
            not_yet: a.not_yet.iter().map(|&k| k.into()).collect(),
            estimate: a.estimate.map(|e| Estimate {
                requests: e.requests,
                usd: e.usd,
                exact: e.exact,
            }),
            complete: a.complete(),
            incomplete: incomplete(&a.incomplete),
            failure: None,
            usage: usage(&a.usage),
            related: a.related.as_ref().map(|s| RelatedWorks {
                items: s
                    .items
                    .iter()
                    .map(|r| RelatedWork {
                        id: r.id.to_string(),
                        ids: r.ids.iter().map(ToString::to_string).collect(),
                        work: r.record.as_ref().map(Work::from_record),
                        score: r.score,
                        co_cited_with: r.co_cited_with as u32,
                        shared_references: r.shared_references as u32,
                        seeds: r.seeds.iter().map(|&i| i as u32).collect(),
                    })
                    .collect(),
                incomplete: incomplete(&s.incomplete),
            }),
        }
    }
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
        // A request's option, not a tree's filter, but listed all the same.
        assert_eq!(
            SearchFilter::from_core(Filter::Since),
            Some(SearchFilter::Since)
        );
        // Query trees' fields, filters, relations and reaches.
        for f in TermField::ALL {
            let field = SearchField::from_core(f.filter()).unwrap();
            assert_eq!(TermField::from(field), *f);
        }
        for f in FilterValue::ALL {
            let value = SearchFilterValue::from_core(f).unwrap();
            assert_eq!(name(&value), f.filter().as_str());
            assert_eq!(FilterValue::from(value), *f);
            assert!(SearchFilter::from_core(f.filter()).is_some());
        }
        for r in Relation::ALL {
            let relation = SearchRelation::from_core(*r).unwrap();
            assert_eq!(name(relation), r.as_str());
            assert_eq!(Relation::from(relation), *r);
        }
        for b in BooleanSupport::ALL {
            assert_eq!(name(SearchBoolean::from_core(*b)), b.as_str());
        }
        // Citation links.
        for k in engine::AnalysisKind::ALL {
            let kind = AnalysisKind::from(*k);
            assert_eq!(name(kind), k.as_str());
            assert_eq!(engine::AnalysisKind::from(kind), *k);
        }
        for c in engine::Coverage::ALL {
            assert_eq!(name(Coverage::from(*c)), c.as_str());
        }
        for p in engine::SeedProblem::ALL {
            assert_eq!(name(SeedProblem::from(*p)), p.as_str());
        }
        for k in engine::IncompleteKind::ALL {
            assert_eq!(name(IncompleteKind::from(*k)), k.as_str());
        }
        for r in engine::OddReason::ALL {
            assert_eq!(name(OddReason::from(*r)), r.as_str());
        }
        // Failures.
        for k in aspero_core::ErrorKind::ALL {
            assert_eq!(name(ErrorKind::from(*k)), k.as_str());
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

    #[test]
    fn a_reached_cap_carries_its_figures() {
        let resets = Timestamp::from_second(1_794_700_800).unwrap();
        let e = engine::EngineError::Provider {
            provider: ProviderId::from_static("openalex"),
            error: aspero_core::Error::BudgetCapped {
                cap: 0.5,
                used: 0.5012,
                resets_at: Some(resets),
            },
        };
        let failure = Failure::from(&e);
        assert_eq!(failure.kind, ErrorKind::BudgetCapped);
        assert_eq!(failure.provider.as_deref(), Some("openalex"));
        assert_eq!(
            failure.budget_cap,
            Some(BudgetCap {
                cap: 0.5,
                used: 0.5012,
                resets_at: Some(time(resets)),
            })
        );
        // Other failures carry none.
        let other = engine::EngineError::Provider {
            provider: ProviderId::from_static("openalex"),
            error: aspero_core::Error::Timeout,
        };
        assert_eq!(Failure::from(&other).budget_cap, None);
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
