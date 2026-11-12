//! Verdicts.

use aspero_engine::{Reason, Verification};
use serde_json::{Value, json};

use crate::{merged, work};

/// A claim that couldn't be read, after its echo.
pub fn invalid_claim(echo: Value, reason: &str) -> Value {
    merged(echo, json!({"verdict": "invalid", "reason": reason}))
}

/// One verification, after the claim's echo (input, key, claim).
pub fn verification(echo: Value, v: &Verification, full: bool) -> Value {
    let mut item = merged(
        echo,
        json!({
            "verdict": v.verdict.as_str(),
            "reason": v.reason,
            "why": why(v, full),
            "read": v.read,
        }),
    );
    if let Some(best) = v.best {
        item["match"] = work::candidate(&v.report.candidates[best], full);
    }
    let failures = v.report.failures();
    if !failures.is_empty() {
        let errors: Vec<String> = failures.iter().map(ToString::to_string).collect();
        item["errors"] = json!(errors);
        let kinds: Vec<&str> = failures.iter().map(|e| e.kind().as_str()).collect();
        item["error_kinds"] = json!(kinds);
    }
    if full {
        let all: Vec<Value> = v
            .report
            .candidates
            .iter()
            .map(|c| work::candidate(c, true))
            .collect();
        item["candidates"] = json!(all);
    }
    item
}

/// The reason as data: `{"code", "differs": [{"field", "actual", "cited"}],
/// "cited": candidate}`, so a front end can word it itself.
fn why(v: &Verification, full: bool) -> Value {
    let mut out = json!({"code": v.why.as_str()});
    let differs: Vec<Value> = v
        .why
        .differs()
        .iter()
        .map(|d| {
            let mut out = json!({"field": d.field.as_str()});
            if let Some(actual) = &d.actual {
                out["actual"] = json!(actual);
            }
            if let Some(cited) = &d.cited {
                out["cited"] = json!(cited);
            }
            out
        })
        .collect();
    if !differs.is_empty() {
        out["differs"] = json!(differs);
    }
    if let Reason::IdElsewhere { cited } = v.why {
        out["cited"] = work::candidate(&v.report.candidates[cited], full);
    }
    out
}
