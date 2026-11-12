//! Checking an API key before keeping it.

use aspero_core::{Error, ErrorKind, ProviderId};

use crate::Budget;

/// Whether a provider accepts a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyStatus {
    /// Accepted, even if its budget is spent.
    Valid,
    Rejected,
    /// The provider couldn't be reached, so the key is untested.
    Unreachable,
}

impl KeyStatus {
    /// What a probe's failure, if any, says about the key: a spent budget
    /// was still accepted.
    pub fn of(failure: Option<&Error>) -> KeyStatus {
        match failure.map(Error::kind) {
            None | Some(ErrorKind::BudgetSpent) => KeyStatus::Valid,
            Some(ErrorKind::KeyRejected) => KeyStatus::Rejected,
            Some(_) => KeyStatus::Unreachable,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            KeyStatus::Valid => "valid",
            KeyStatus::Rejected => "rejected",
            KeyStatus::Unreachable => "unreachable",
        }
    }
}

/// What checking a key found.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct KeyCheck {
    pub status: KeyStatus,
    /// Why, in English, unless valid.
    pub message: Option<String>,
    /// What the key has left, as the provider said.
    pub budget: Option<(ProviderId, Budget)>,
}

/// Whether OpenAlex accepts `key`, with one free lookup (never cached) by
/// an engine of its own, and what the key has left. `pacing` shares
/// request slots with the caller's engine. Fails only if the HTTP client
/// can't be built.
#[cfg(feature = "openalex")]
pub async fn check_openalex_key(
    key: &str,
    mailto: Option<String>,
    pacing: Option<std::sync::Arc<dyn crate::Pacing>>,
) -> Result<KeyCheck, Error> {
    use aspero_core::{FieldSet, WorkId};

    use crate::{CallOptions, Outcome, StandardConfig, standard};

    /// A work every OpenAlex key can look up, free.
    const KNOWN_WORK: &str = "doi:10.1038/nature14539";

    if key.trim().is_empty() {
        return Ok(KeyCheck {
            status: KeyStatus::Rejected,
            message: Some("no key given".into()),
            budget: None,
        });
    }
    let mut openalex = crate::OpenAlexConfig::default();
    openalex.api_key = Some(crate::SecretString::from(key.trim().to_owned()));
    let config = StandardConfig {
        mailto,
        pacing,
        openalex: Some(openalex),
        ..StandardConfig::default()
    };
    let engine = standard(config)?.with_options(CallOptions::interactive());

    let id: WorkId = KNOWN_WORK.parse().expect("a valid identifier");
    let report = engine.lookup(&[id], FieldSet::DEFAULT).await;
    let failure = report.results[0]
        .attempts
        .iter()
        .find_map(|a| match &a.outcome {
            Outcome::Failed(e) => Some(e.clone()),
            _ => None,
        });
    let status = KeyStatus::of(failure.as_ref());
    Ok(KeyCheck {
        status,
        message: failure
            .filter(|_| status != KeyStatus::Valid)
            .map(|e| e.to_string()),
        budget: engine.budgets().into_iter().next(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spent_budget_is_a_valid_key() {
        let status = |e: Error| KeyStatus::of(Some(&e));
        assert_eq!(KeyStatus::of(None), KeyStatus::Valid);
        assert_eq!(
            status(Error::QuotaExhausted { resets_at: None }),
            KeyStatus::Valid
        );
        assert_eq!(
            status(Error::AuthRejected { message: None }),
            KeyStatus::Rejected
        );
        assert_eq!(status(Error::Timeout), KeyStatus::Unreachable);
    }
}
