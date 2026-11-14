//! OpenAlex HTTP statuses and usage headers, mapped into aspero's contract.
//!
//! Observed (2026-11-10, 2026-11-14): every response, free lookups
//! included, carries `x-ratelimit-cost-usd`, `x-ratelimit-limit-usd` (the
//! daily allowance: 1 with a key, 0.1 without), `x-ratelimit-remaining-usd`
//! (left of it today), `x-ratelimit-prepaid-remaining-usd` and
//! `x-ratelimit-reset` (seconds to midnight UTC). 404s are an HTML page;
//! 400s are JSON `{error, message}`.
//!
//! `GET /rate-limit` (key required, as a Bearer header or `api_key`; 401
//! "Authentication required" without) says the same as JSON, costs
//! nothing (`daily_used_usd` unchanged across calls, checked 2026-11-14)
//! and carries no `x-ratelimit-*` headers.

use std::time::Duration;

use aspero_core::http::status_error;
use aspero_core::{Error, HttpResponse, Usage};

/// Error messages longer than this are cut; OpenAlex's 400 bodies list every
/// valid field name.
const MAX_MESSAGE: usize = 300;

pub(crate) fn usage(resp: &HttpResponse) -> Usage {
    let num = |name: &str| resp.header(name).and_then(|v| v.trim().parse::<f64>().ok());
    let mut usage = Usage::default();
    usage.cost = num("x-ratelimit-cost-usd");
    usage.remaining = num("x-ratelimit-remaining-usd");
    usage.limit = num("x-ratelimit-limit-usd");
    usage.prepaid_remaining = num("x-ratelimit-prepaid-remaining-usd");
    usage.resets_at = num("x-ratelimit-reset")
        .and_then(|secs| Duration::try_from_secs_f64(secs).ok())
        .and_then(|d| resp.received_at.checked_add(d).ok());
    usage
}

/// The budget `/rate-limit` reports: `{"rate_limit": {"daily_budget_usd",
/// "daily_remaining_usd", "prepaid_remaining_usd", "resets_at", …}}`.
pub(crate) fn rate_limit(resp: &HttpResponse) -> Result<Usage, Error> {
    #[derive(serde::Deserialize)]
    struct Body {
        rate_limit: RateLimit,
    }
    #[derive(serde::Deserialize)]
    struct RateLimit {
        daily_budget_usd: Option<f64>,
        daily_used_usd: Option<f64>,
        daily_remaining_usd: Option<f64>,
        prepaid_remaining_usd: Option<f64>,
        resets_at: Option<String>,
        resets_in_seconds: Option<f64>,
    }
    let body: Body = serde_json::from_slice(&resp.body).map_err(Error::decode)?;
    let r = body.rate_limit;
    let mut usage = Usage::default();
    usage.cost = Some(0.0);
    usage.limit = r.daily_budget_usd;
    usage.remaining = r
        .daily_remaining_usd
        .or_else(|| Some(r.daily_budget_usd? - r.daily_used_usd?));
    usage.prepaid_remaining = r.prepaid_remaining_usd;
    usage.resets_at = r
        .resets_at
        .and_then(|t| t.parse::<jiff::Timestamp>().ok())
        .or_else(|| {
            let d = Duration::try_from_secs_f64(r.resets_in_seconds?).ok()?;
            resp.received_at.checked_add(d).ok()
        });
    Ok(usage)
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
        // Whatever the status: the request that crosses the budget has come
        // back as a server error, the ones after it as 429s.
        _ if quota_exhausted(resp) => Error::QuotaExhausted {
            resets_at: usage(resp).resets_at,
        },
        401 | 403 => Error::AuthRejected { message },
        _ => status_error(resp, message),
    })
}

/// A failure means "budget spent" (rather than "slow down" or a server
/// fault) when OpenAlex says so in the body, or the remaining budget header
/// is zero.
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
