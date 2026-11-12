//! Crossref HTTP statuses mapped into aspero's contract.
//!
//! Observed (2026-11-10): 404s are `text/plain` "Resource not found."; 400s
//! are JSON `{status: "failed", message: [{type, value, message}]}`. Pool and
//! limits are advertised in `x-api-pool`, `x-rate-limit-*` and
//! `x-concurrency-limit`.

use aspero_core::http::status_error;
use aspero_core::{Error, HttpResponse};

const MAX_MESSAGE: usize = 300;

pub(crate) fn check(resp: &HttpResponse) -> Result<(), Error> {
    if resp.is_success() {
        return Ok(());
    }
    let message = message(resp);
    Err(match resp.status {
        400 => Error::BadRequest(message.unwrap_or_else(|| "rejected by Crossref".into())),
        401 | 403 => Error::AuthRejected { message },
        _ => status_error(resp, message),
    })
}

/// The validation messages from a JSON error body, joined.
fn message(resp: &HttpResponse) -> Option<String> {
    let json: serde_json::Value = serde_json::from_slice(&resp.body).ok()?;
    let text = match json.get("message")? {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|i| i.get("message").and_then(|m| m.as_str()))
            .collect::<Vec<_>>()
            .join("; "),
        _ => return None,
    };
    Some(match text.char_indices().nth(MAX_MESSAGE) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text,
    })
}
