//! Lookup results.

use aspero_core::ErrorCategory;
use aspero_engine::{Disagreement, InputLookup, LookupResult, Outcome, Served};
use serde_json::{Value, json};

use crate::work;

/// How a lookup ended, as its item's `status` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Status {
    Found,
    /// No enabled provider looks up this kind of identifier.
    Unsupported,
    NotFound,
    /// Every provider asked failed; the first failure's category.
    Failed(ErrorCategory),
}

impl Status {
    pub fn of(result: &LookupResult) -> Self {
        if result.record.is_some() {
            return Status::Found;
        }
        if result.attempts.is_empty() {
            return Status::Unsupported;
        }
        let mut failures = result.attempts.iter().filter_map(|a| match &a.outcome {
            Outcome::Failed(e) => Some(e.category()),
            _ => None,
        });
        match failures.next() {
            Some(first) if failures.count() + 1 == result.attempts.len() => Status::Failed(first),
            _ => Status::NotFound,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Status::Found => "found",
            Status::Unsupported => "unsupported",
            Status::NotFound => "not_found",
            Status::Failed(_) => "failed",
        }
    }
}

/// An input that isn't an identifier.
pub fn invalid(input: &str, error: &str) -> Value {
    json!({"input": input, "status": "invalid", "error": error})
}

/// One result. Attempts are included when nothing was found (so the caller
/// sees why) or with `full`.
pub fn item(input: &str, result: &LookupResult, full: bool) -> Value {
    let status = Status::of(result);
    let mut item = json!({"input": input, "id": result.id, "status": status.as_str()});
    if let Some(record) = &result.record {
        // Say when the answer didn't come live from the provider.
        let served = result
            .attempts
            .iter()
            .rfind(|a| matches!(a.outcome, Outcome::Found))
            .map(|a| a.served);
        if let Some(s) = served.filter(|s| *s != Served::Live) {
            item["served"] = json!(s.as_str());
        }
        item["work"] = work::work(record, full);
    }
    if status == Status::Unsupported {
        item["error"] = json!(format!(
            "no enabled provider looks up {} identifiers",
            result.id.kind()
        ));
    }
    if result.record.is_none() || full {
        let attempts: Vec<Value> = result
            .attempts
            .iter()
            .map(|a| match &a.outcome {
                Outcome::Found => json!({"provider": a.provider, "outcome": "found"}),
                Outcome::NotFound => json!({"provider": a.provider, "outcome": "not_found"}),
                Outcome::Failed(e) => {
                    json!({"provider": a.provider, "outcome": "failed", "error": e.to_string()})
                }
                _ => json!({"provider": a.provider, "outcome": "unknown"}),
            })
            .collect();
        item["attempts"] = json!(attempts);
    }
    item
}

/// One input's lookup, with whatever extras were asked for.
pub fn input(input: &str, lookup: &InputLookup, full: bool) -> Value {
    let looked = match lookup {
        InputLookup::Invalid(e) => return invalid(input, &e.to_string()),
        InputLookup::Looked(looked) => looked,
    };
    let mut out = item(input, &looked.result, full);
    if looked.with_punctuation {
        out["note"] = json!("found with the trailing punctuation kept");
    }
    if let Some(found) = &looked.abstract_text {
        work::set_abstract(&mut out["work"], found.as_ref());
    }
    if let Some(latest) = &looked.latest {
        out["latest"] = work::work(latest, false);
    }
    if !looked.disagreements.is_empty() {
        out["disagreements"] = disagreements(&looked.disagreements);
    }
    out
}

/// Where a record contradicts its DOI's registry: `[{field, value,
/// <registry>: registry_value}]`.
pub fn disagreements(found: &[Disagreement]) -> Value {
    found
        .iter()
        .map(|d| {
            json!({
                "field": d.field,
                "value": d.value,
                d.registry.as_str(): d.registry_value,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use aspero_core::{Error, ProviderId};
    use aspero_engine::Attempt;

    use super::*;

    fn result(outcomes: Vec<Outcome>) -> LookupResult {
        LookupResult {
            id: "doi:10.1234/x".parse().unwrap(),
            record: None,
            attempts: outcomes
                .into_iter()
                .map(|outcome| Attempt {
                    provider: ProviderId::from_static("crossref"),
                    outcome,
                    served: Served::Live,
                })
                .collect(),
        }
    }

    #[test]
    fn statuses_say_why_nothing_was_found() {
        assert_eq!(Status::of(&result(vec![])), Status::Unsupported);
        assert_eq!(
            Status::of(&result(vec![Outcome::NotFound])),
            Status::NotFound
        );
        let failed = || Outcome::Failed(Error::Timeout);
        assert_eq!(
            Status::of(&result(vec![failed(), Outcome::NotFound])),
            Status::NotFound
        );
        assert!(matches!(
            Status::of(&result(vec![failed(), failed()])),
            Status::Failed(_)
        ));
    }

    #[test]
    fn items_without_a_record_carry_their_attempts() {
        assert_eq!(
            item("10.1234/x", &result(vec![Outcome::NotFound]), false),
            json!({
                "input": "10.1234/x",
                "id": "doi:10.1234/x",
                "status": "not_found",
                "attempts": [{"provider": "crossref", "outcome": "not_found"}],
            })
        );
        let unsupported = item("10.1234/x", &result(vec![]), false);
        assert_eq!(unsupported["status"], "unsupported");
        assert!(unsupported["error"].as_str().unwrap().contains("doi"));
    }
}
