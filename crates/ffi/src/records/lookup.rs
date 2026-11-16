//! Looking up identifiers (`lookup`), and the work a document is
//! (`identify`).

use aspero_engine::{
    self as engine, Disagreement as CoreDisagreement, InputLookup, LookupResult, Outcome, Served,
};
use aspero_json::Status;

use super::{ErrorKind, ProviderUsage, Verification, Work};

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
    /// For failures: what went wrong, in English.
    pub error: Option<String>,
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
    pub served: Option<Freshness>,
    pub attempts: Vec<Attempt>,
    /// Why the input isn't an identifier, or isn't supported.
    pub error: Option<String>,
    /// The DOI missed with its trailing punctuation stripped, and was
    /// found with it kept.
    pub found_with_trailing_punctuation: bool,
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
                    served: None,
                    attempts: Vec::new(),
                    error: Some(e.to_string()),
                    found_with_trailing_punctuation: false,
                    latest: None,
                    disagreements: Vec::new(),
                };
            }
            InputLookup::Looked(looked) => looked,
        };
        let mut lookup = Lookup::from_result(input, &looked.result);
        lookup.found_with_trailing_punctuation = looked.found_with_trailing_punctuation;
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
        let served = result.served().map(Freshness::from);
        let attempts = result
            .attempts
            .iter()
            .map(|a| {
                #[allow(clippy::match_same_arms)] // the wildcard: see below
                let (outcome, failure) = match &a.outcome {
                    Outcome::Found => (AttemptOutcome::Found, None),
                    Outcome::Failed(e) => (AttemptOutcome::Failed, Some(e)),
                    Outcome::NotFound => (AttemptOutcome::NotFound, None),
                    // `Outcome` is non-exhaustive: an outcome added later
                    // found nothing usable here, and it isn't a failure the
                    // app could word, so it shows as not found.
                    _ => (AttemptOutcome::NotFound, None),
                };
                Attempt {
                    provider: a.provider.to_string(),
                    outcome,
                    kind: failure.map(|e| e.kind().into()),
                    error: failure.map(ToString::to_string),
                }
            })
            .collect();
        let work = result.record.as_ref().map(Work::from_record);
        Lookup {
            input: input.to_owned(),
            id: Some(result.id.to_string()),
            status,
            work,
            served,
            attempts,
            error: result.unsupported(),
            found_with_trailing_punctuation: false,
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
