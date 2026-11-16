//! OpenAlex HTTP statuses and usage headers, mapped into aspero's contract.
//!
//! Observed (2026-11-10, 2026-11-14): every response, free lookups
//! included, carries `x-ratelimit-cost-usd`, `x-ratelimit-limit-usd` (the
//! daily allowance: 1 with a key, 0.1 without), `x-ratelimit-remaining-usd`
//! (left of it today), `x-ratelimit-prepaid-remaining-usd` and
//! `x-ratelimit-reset` (seconds to midnight UTC). 404s are an HTML page;
//! 400s are JSON `{error, message}`. The prepaid balance in headers comes
//! from whichever server answered, some minutes behind: requests made
//! together said 9.84 and 9.66 within a second (2026-11-16), so it's
//! never taken alone as the balance (see the engine's `PrepaidDay`).
//!
//! `GET /rate-limit` (key required, as a Bearer header or `api_key`; 401
//! "Authentication required" without) says the same as JSON, costs
//! nothing (`daily_used_usd` unchanged across calls, checked 2026-11-14)
//! and carries no `x-ratelimit-*` headers.

use std::time::Duration;

use aspero_core::http::status_error;
use aspero_core::{Error, HttpResponse, Usage};
use jiff::Timestamp;

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
/// "daily_remaining_usd", "prepaid_remaining_usd", "prepaid_expires_at",
/// "resets_at", …}}`.
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
        prepaid_balance_usd: Option<f64>,
        /// A string or whatever else; never failing the rest.
        prepaid_expires_at: Option<serde_json::Value>,
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
    // The credit bought, so this balance is OpenAlex's own word.
    usage.prepaid_bought = r.prepaid_balance_usd;
    usage.prepaid_expires_at = r.prepaid_expires_at.as_ref().and_then(|v| match v {
        serde_json::Value::String(s) => date(s),
        // Unix milliseconds, as a JavaScript Date's value.
        serde_json::Value::Number(n) => Timestamp::from_millisecond(n.as_i64()?).ok(),
        _ => None,
    });
    usage.resets_at = r
        .resets_at
        .and_then(|t| t.parse::<Timestamp>().ok())
        .or_else(|| {
            let d = Duration::try_from_secs_f64(r.resets_in_seconds?).ok()?;
            resp.received_at.checked_add(d).ok()
        });
    Ok(usage)
}

/// A date as `/rate-limit` gives one: ISO 8601 (`resets_at`), or, for
/// `prepaid_expires_at`, a JavaScript `Date` printed as a string ("Sun Feb
/// 14 2027 12:08:05 GMT+0000 (Coordinated Universal Time)", seen
/// 2026-11-16). `None` for anything else.
pub(crate) fn date(s: &str) -> Option<Timestamp> {
    let s = s.trim();
    if let Ok(t) = s.parse::<Timestamp>() {
        return Some(t);
    }
    // The zone's name in brackets says nothing the offset doesn't.
    let s = s.find(" (").map_or(s, |at| &s[..at]);
    jiff::fmt::strtime::parse("%a %b %d %Y %H:%M:%S GMT%z", s)
        .and_then(|t| t.to_timestamp())
        .ok()
}

/// `Ok` for 2xx; otherwise the matching [`Error`]. 404 is `NotFound`, which
/// lookups turn into an absent record. Sent without a key (`keyed` false),
/// a 401 or 403 rejected no credentials: it's something OpenAlex does only
/// with a key, such as a cursor past the first page (401 "Paging past the
/// first page with cursor= needs a free API key", 2026-11-14), so it's
/// `UnsupportedQuery`, which doesn't stop the provider for the engine's
/// life as a rejected key does.
pub(crate) fn check(resp: &HttpResponse, keyed: bool) -> Result<(), Error> {
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
        401 | 403 if !keyed => Error::UnsupportedQuery(format!(
            "OpenAlex needs an API key for this{}",
            message.map(|m| format!(": {m}")).unwrap_or_default()
        )),
        401 | 403 => Error::AuthRejected { message },
        _ => status_error(resp, message),
    })
}

/// A failure means "budget spent" (rather than "slow down" or a server
/// fault) when OpenAlex says so in the body, or the remaining allowance
/// header is zero with no prepaid credit left beside it. OpenAlex spends
/// credit once the allowance is used up, so a failure with the allowance
/// at zero but credit left (a per-second 429, a server fault) is that
/// failure, not a spent budget, which would stop every call until the
/// reset.
fn quota_exhausted(resp: &HttpResponse) -> bool {
    let num = |name: &str| resp.header(name).and_then(|v| v.trim().parse::<f64>().ok());
    let body_says = resp.text().contains("not_enough_credits");
    let allowance_spent = num("x-ratelimit-remaining-usd").is_some_and(|r| r <= 0.0);
    let credit_left = num("x-ratelimit-prepaid-remaining-usd").is_some_and(|p| p > 0.0);
    body_says || (allowance_spent && !credit_left)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_read_as_javascript_prints_them_or_as_iso() {
        let at = |s: &str| date(s).map(|t| t.to_string());
        let expiry = Some("2027-02-14T12:08:05Z".to_owned());
        assert_eq!(
            at("Sun Feb 14 2027 12:08:05 GMT+0000 (Coordinated Universal Time)"),
            expiry
        );
        assert_eq!(at("Sun Feb 14 2027 12:08:05 GMT+0000"), expiry);
        assert_eq!(
            at("Sun Feb 14 2027 13:08:05 GMT+0100 (Central European Standard Time)"),
            expiry
        );
        assert_eq!(at("2027-02-14T12:08:05.000Z"), expiry);
        assert_eq!(at("2027-02-14T12:08:05Z"), expiry);
        for nonsense in ["", "soon", "Invalid Date", "Feb 14 2027"] {
            assert_eq!(at(nonsense), None, "{nonsense}");
        }
    }
}
