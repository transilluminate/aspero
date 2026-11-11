//! Verdicts.

use aspero_engine::Verification;
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
        }),
    );
    if let Some(best) = v.best {
        item["match"] = work::candidate(&v.report.candidates[best], full);
    }
    if !v.report.errors.is_empty() {
        let errors: Vec<String> = v.report.errors.iter().map(ToString::to_string).collect();
        item["errors"] = json!(errors);
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
