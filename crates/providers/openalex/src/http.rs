//! OpenAlex HTTP statuses and usage headers, mapped into aspero's contract.
//!
//! Observed (2026-11-10): every response carries `x-ratelimit-cost-usd`,
//! `x-ratelimit-remaining-usd` and `x-ratelimit-reset` (seconds to midnight
//! UTC). 404s are an HTML page; 400s are JSON `{error, message}`.

use std::time::Duration;

use aspero_core::{Error, HttpResponse, Usage};

/// Error messages longer than this are cut; OpenAlex's 400 bodies list every
/// valid field name.
const MAX_MESSAGE: usize = 300;

pub(crate) fn usage(resp: &HttpResponse) -> Usage {
    let num = |name: &str| resp.header(name).and_then(|v| v.trim().parse::<f64>().ok());
    let mut usage = Usage::default();
    usage.cost = num("x-ratelimit-cost-usd");
    usage.remaining = num("x-ratelimit-remaining-usd");
    usage.resets_at = num("x-ratelimit-reset")
        .and_then(|secs| Duration::try_from_secs_f64(secs).ok())
        .and_then(|d| resp.received_at.checked_add(d).ok());
    usage
}

/// `Ok` for 2xx; otherwise the matching [`Error`]. 404 is `NotFound`, which
/// lookups turn into an absent record.
pub(crate) fn check(resp: &HttpResponse) -> Result<(), Error> {
    if resp.is_success() {
        return Ok(());
    }
    let message = message(resp);
    Err(match resp.status {
        400 => Error::BadRequest(message.unwrap_or_else(|| "rejected by OpenAlex".into())),
        401 | 403 => Error::AuthRejected { message },
        404 => Error::NotFound,
        429 if quota_exhausted(resp) => Error::QuotaExhausted {
            resets_at: usage(resp).resets_at,
        },
        429 => Error::RateLimited {
            retry_after: resp.retry_after(),
        },
        status => Error::Server { status, message },
    })
}

/// A 429 means "budget spent" rather than "slow down" when OpenAlex says so
/// in the body, or the remaining budget header is zero.
fn quota_exhausted(resp: &HttpResponse) -> bool {
    let body_says = resp.text().contains("not_enough_credits");
    let budget_spent = resp
        .header("x-ratelimit-remaining-usd")
        .and_then(|v| v.trim().parse::<f64>().ok())
        .is_some_and(|r| r <= 0.0);
    body_says || budget_spent
}

/// The JSON `message` (or `error`) from an error body; nothing for HTML.
fn message(resp: &HttpResponse) -> Option<String> {
    let json: serde_json::Value = serde_json::from_slice(&resp.body).ok()?;
    let text = ["message", "error"]
        .iter()
        .find_map(|k| json.get(k).and_then(|v| v.as_str()))?;
    Some(match text.char_indices().nth(MAX_MESSAGE) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_owned(),
    })
}
