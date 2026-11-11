//! What calls return, as UniFFI records and enums: mirrors of the engine's
//! types, converted at the boundary so the engine stays free of FFI.
//!
//! Records carry what an app shows; `Work::json` has everything else, in
//! the JSON contract's shape. Identifiers are strings with their prefix
//! (`doi:10.1038/nature14539`), dates ISO 8601 as precise as known.

use std::time::SystemTime;

use aspero_core::{Agreement as CoreAgreement, SourceRecord, UpdateKind};
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
                    Some(Author {
                        name: p.display_name()?,
                        orcid: p.orcid.clone(),
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
            fetched_at: time(record.fetched_at),
            json: aspero_json::work(record, true).to_string(),
        }
    }

    /// A candidate's work, with every identifier and notice its merged
    /// records gave.
    fn from_candidate(c: &CoreCandidate) -> Self {
        let mut work = Work::from_record(&c.record);
        work.ids = c.ids.iter().map(ToString::to_string).collect();
        work.canonical = aspero_core::canonical(&c.ids).map(|id| id.to_string());
        if !c.updates.is_empty() {
            work.notices = c.updates.iter().map(Notice::from).collect();
        }
        work
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
        let freshness = result
            .attempts
            .iter()
            .rfind(|a| matches!(a.outcome, Outcome::Found))
            .map(|a| match a.served {
                Served::Cache => Freshness::Cache,
                Served::Stale => Freshness::Stale,
                _ => Freshness::Live,
            });
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
            error: (status == LookupStatus::Unsupported).then(|| {
                format!(
                    "no enabled provider looks up {} identifiers",
                    result.id.kind()
                )
            }),
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
}

/// A cited field the work disagrees with, and the work's value when worth
/// showing (year, journal, volume, first page).
#[derive(Debug, Clone, uniffi::Record)]
pub struct Difference {
    pub field: CitedField,
    pub actual: Option<String>,
}

impl From<&engine::Difference> for Difference {
    fn from(d: &engine::Difference) -> Self {
        let field = match d.field {
            engine::CitedField::Title => CitedField::Title,
            engine::CitedField::Authors => CitedField::Authors,
            engine::CitedField::Year => CitedField::Year,
            engine::CitedField::Journal => CitedField::Journal,
            engine::CitedField::Volume => CitedField::Volume,
            _ => CitedField::FirstPage,
        };
        Difference {
            field,
            actual: d.actual.clone(),
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
    PartlyMatches {
        differs: Vec<Difference>,
    },
    /// The closest work shares the title, but other details differ.
    Closest {
        differs: Vec<Difference>,
    },
    NoMatch,
    /// The identifiers exist; the claim gives nothing else to compare.
    IdExists,
    /// The identifiers exist and the citation agrees.
    IdAgrees,
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
    fn new(v: &engine::Verification) -> Self {
        let differs = || v.why.differs().iter().map(Into::into).collect();
        match &v.why {
            engine::Reason::Matches => Reason::Matches,
            engine::Reason::PartlyMatches { .. } => Reason::PartlyMatches { differs: differs() },
            engine::Reason::Closest { .. } => Reason::Closest { differs: differs() },
            engine::Reason::NoMatch => Reason::NoMatch,
            engine::Reason::IdExists => Reason::IdExists,
            engine::Reason::IdAgrees => Reason::IdAgrees,
            engine::Reason::IdPartlyMatches { .. } => {
                Reason::IdPartlyMatches { differs: differs() }
            }
            engine::Reason::IdElsewhere { cited } => Reason::IdElsewhere {
                cited: Candidate::from(&v.report.candidates[*cited]),
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
            why: Reason::new(v),
            reason: v.reason.clone(),
            notes: v.notes.iter().map(Notice::from).collect(),
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
    pub(crate) fn new(text: &str, m: &aspero_core::id::Mention) -> Self {
        Mention {
            id: m.id.to_string(),
            kind: m.id.kind().prefix().to_owned(),
            text: text[m.span.clone()].to_owned(),
            line: m.line as u64,
            column: m.column as u64,
            utf16: utf16_range(text, &m.span),
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
    pub(crate) fn new(list: &str, r: &aspero_core::Reference) -> Self {
        Reference {
            text: r.text.clone(),
            utf16: utf16_range(list, &r.span),
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

fn utf16_range(text: &str, span: &std::ops::Range<usize>) -> Utf16Range {
    let utf16 = |byte: usize| text[..byte].encode_utf16().count() as u64;
    Utf16Range {
        start: utf16(span.start),
        end: utf16(span.end),
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
    /// A previous page's `next`, with the same request otherwise.
    #[uniffi(default = None)]
    pub cursor: Option<String>,
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
                freshness: Some(match report.served {
                    Served::Cache => Freshness::Cache,
                    Served::Stale => Freshness::Stale,
                    _ => Freshness::Live,
                }),
                failure: None,
                usage: report
                    .usage
                    .map(|u| usage(&[(report.provider.clone(), u)]))
                    .unwrap_or_default(),
            },
        }
    }
}
