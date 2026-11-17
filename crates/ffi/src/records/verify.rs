//! Verifying citations: the claim, its verdict and why, and the
//! candidates scored against it.

use aspero_core::Agreement as CoreAgreement;
use aspero_engine::{self as engine, Candidate as CoreCandidate};
use aspero_json::InputClaim;

use super::{Failure, Notice, ProviderUsage, Work, count};

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
    /// The site the claim was read from: an article page's host
    /// ("jamanetwork.com", as `claimFromPage` fills it) or address. A hint
    /// of the publisher for a claim without identifiers, never compared
    /// as the journal; `Verification.chosenBySite` says when it decided.
    #[uniffi(default = None)]
    pub site: Option<String>,
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
            site: c.site.clone(),
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
            site: self.site,
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
    /// A book's publisher, named with its year when both differ: perhaps
    /// another edition.
    Publisher,
}

/// A cited field the work disagrees with, and the work's value when worth
/// showing (year, journal, volume, first page).
#[derive(Debug, Clone, uniffi::Record)]
pub struct Difference {
    pub field: CitedField,
    pub actual: Option<String>,
    /// The citation's value, as compared: the year, journal, volume or
    /// first page read from it, or the title, authors or journal given.
    /// `nil` when free text was searched for the work's own.
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
            engine::CitedField::Publisher => CitedField::Publisher,
            engine::CitedField::FirstPage | _ => CitedField::FirstPage,
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

/// Why a verdict was reached. "The work" is the verification's `bestMatch`;
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
    pub(super) fn new(why: &engine::Reason, candidates: &[CoreCandidate]) -> Self {
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
            CoreAgreement::Unknown => Agreement::Unknown,
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
    pub claim_key: Option<String>,
    pub verdict: Verdict,
    /// Why, as data, for the app to word.
    pub reason: Reason,
    /// One sentence in English, `reason` and `notes`: for logs and agents,
    /// not for display in an app UI.
    pub reason_text: String,
    /// About `bestMatch`, whatever the verdict: its retraction or
    /// withdrawal, and its newer version.
    pub notes: Vec<Notice>,
    /// The claim as read: identifiers as written (with the other readings
    /// tried in `alternativeIds`), and from free text the year, journal
    /// (`container`, where it can be told), volume and first page. `nil`
    /// for an invalid claim.
    pub read: Option<Claim>,
    /// The work the verdict is about: the match, or the best guess.
    pub best_match: Option<Candidate>,
    /// With `full`: every candidate, most confident first.
    pub candidates: Vec<Candidate>,
    /// Providers that failed, searching or looking up the claim's
    /// identifiers; what they'd have found is unknown.
    pub errors: Vec<Failure>,
    /// Whether the claim's `site` decided which work `bestMatch` is: of
    /// works matching about as well, the page's publisher's ("chosen as
    /// the page's publisher's"). It never changes the verdict.
    #[uniffi(default = false)]
    pub chosen_by_site: bool,
}

impl Verification {
    pub(crate) fn new(
        index: usize,
        claim: &InputClaim,
        v: &engine::Verification,
        full: bool,
    ) -> Self {
        Verification {
            index: count(index),
            input: claim.input.clone(),
            claim_key: key(claim),
            verdict: v.verdict.into(),
            reason: Reason::new(&v.reason, &v.report.candidates),
            reason_text: v.reason_text.clone(),
            notes: v.notes.iter().map(Notice::from).collect(),
            read: Some(Claim::read(&v.read, key(claim))),
            best_match: v
                .best_match
                .map(|i| Candidate::from(&v.report.candidates[i])),
            candidates: if full {
                v.report.candidates.iter().map(Candidate::from).collect()
            } else {
                Vec::new()
            },
            errors: v.report.failures().iter().map(Failure::from).collect(),
            chosen_by_site: v.chosen_by_site,
        }
    }

    pub(crate) fn invalid(index: usize, claim: &InputClaim, why: &str) -> Self {
        Verification {
            index: count(index),
            input: claim.input.clone(),
            claim_key: key(claim),
            verdict: Verdict::Invalid,
            reason: Reason::InvalidClaim {
                why: why.to_owned(),
            },
            reason_text: why.to_owned(),
            notes: Vec::new(),
            read: None,
            best_match: None,
            candidates: Vec::new(),
            errors: Vec::new(),
            chosen_by_site: false,
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
