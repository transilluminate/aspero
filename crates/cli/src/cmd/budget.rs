//! `aspero budget`: what's used and left of each provider's budget, asked
//! for free (OpenAlex's `/rate-limit` with a key; without one, a lookup of
//! one work, which is free too and says the budget in its headers).

use std::fmt::Write as _;

use aspero_core::{FieldSet, WorkId};
use aspero_engine::{Budget, CallOptions, Engine};
use serde_json::{Map, Value, json};

use super::{Failure, Out, print};
use crate::exit::Exit;

/// A work looked up, alone (free), to read the budget without a key.
const PROBE: &str = "W2919115771";

pub async fn run(engine: &Engine, out: Out) -> Result<Exit, Failure> {
    let (budgets, keyless) = budgets(engine).await;
    if out.format.is_human() {
        if budgets.is_empty() {
            println!(
                "no budget known: OpenAlex didn't say (set OPENALEX_API_KEY for $1.00 a day; \
                 without one, it allows $0.10)"
            );
        }
        for (provider, budget) in &budgets {
            let no_key = keyless && provider.as_str() == "openalex";
            println!(
                "{provider}: {}",
                line(budget, no_key, caps(engine, provider))
            );
        }
    } else {
        let list: Vec<Value> = budgets
            .iter()
            .map(|(provider, b)| {
                let mut row = Map::new();
                row.insert("provider".into(), json!(provider));
                if keyless && provider.as_str() == "openalex" {
                    row.insert("key".into(), json!(false));
                }
                let mut put = |k: &str, v: Option<f64>| {
                    if let Some(v) = v {
                        row.insert(k.into(), json!(v));
                    }
                };
                let (allowance_cap, credit_cap) = caps(engine, provider);
                put("limit_usd", b.limit);
                put("spent_usd", b.used());
                put("remaining_usd", b.remaining);
                put("allowance_cap_usd", allowance_cap);
                put("prepaid_spent_today_usd", b.prepaid_spent_today);
                put("credit_cap_usd", credit_cap);
                put("prepaid_remaining_usd", b.prepaid_remaining);
                if let Some(t) = b.prepaid_expires_at {
                    row.insert("prepaid_expires_at".into(), json!(t.to_string()));
                }
                if let Some(t) = b.resets_at {
                    row.insert("resets_at".into(), json!(t.to_string()));
                }
                row.insert("seen_at".into(), json!(b.seen_at.to_string()));
                Value::Object(row)
            })
            .collect();
        print(&json!({ "budgets": list }));
    }
    Ok(Exit::Ok)
}

/// Each provider's budget, refreshed: with an OpenAlex key, its free
/// `/rate-limit`; without one (`true`), which `/rate-limit` won't answer,
/// one free lookup of a single work, past the cache, whose response says
/// the budget. (The FFI's `refresh_budgets` asks nothing without a key.)
async fn budgets(engine: &Engine) -> (Vec<(aspero_core::ProviderId, Budget)>, bool) {
    let mut budgets = engine.refresh_budgets().await;
    let keyless = engine
        .providers()
        .find(|p| p.id().as_str() == "openalex")
        .is_some_and(|p| !p.capabilities().budget_status);
    let known = budgets.iter().any(|(p, _)| p.as_str() == "openalex");
    if keyless && !known && !engine.options().offline {
        let probe: WorkId = PROBE.parse().expect("an OpenAlex id");
        engine
            .with_options(CallOptions::fresh())
            .lookup(&[probe], FieldSet::DEFAULT)
            .await;
        budgets = engine.budgets();
    }
    (budgets, keyless)
}

/// The engine's caps on `provider`: its allowance's, and its credit's.
fn caps(engine: &Engine, provider: &aspero_core::ProviderId) -> (Option<f64>, Option<f64>) {
    (
        engine.daily_cap(provider),
        engine.daily_credit_cap(provider),
    )
}

/// "free allowance $0.0498 of $1.00 used today (cap $0.50), resets 00:00
/// UTC; credit $0.0300 spent today (cap $0.25), $4.97 left, expires
/// 2027-02-14", "(no key)" after "today" when OpenAlex has none. Both
/// figures count everything using the key.
fn line(
    b: &Budget,
    no_key: bool,
    (allowance_cap, credit_cap): (Option<f64>, Option<f64>),
) -> String {
    let key = if no_key { " (no key)" } else { "" };
    let mut text = match (b.used(), b.limit, b.remaining) {
        (Some(used), Some(limit), _) => {
            format!("free allowance ${used:.4} of ${limit:.2} used today{key}")
        }
        (_, _, Some(left)) => format!("free allowance ${left:.4} left today{key}"),
        _ => "budget unknown".to_owned(),
    };
    if let Some(cap) = allowance_cap {
        let _ = write!(text, " (cap ${cap:.2})");
    }
    if let Some(t) = b.resets_at {
        let at = t.to_zoned(jiff::tz::TimeZone::UTC);
        let _ = write!(text, ", resets {} UTC", at.strftime("%H:%M"));
    }
    let balance = b.prepaid_remaining.filter(|p| *p > 0.0);
    let spent = b.prepaid_spent_today.filter(|s| *s > 0.0);
    if balance.is_none() && spent.is_none() {
        if let Some(cap) = credit_cap {
            let _ = write!(text, "; no prepaid credit (cap ${cap:.2} a day)");
        }
        return text;
    }
    let _ = write!(
        text,
        "; credit ${:.4} spent today",
        b.prepaid_spent_today.unwrap_or(0.0)
    );
    if let Some(cap) = credit_cap {
        let _ = write!(text, " (cap ${cap:.2})");
    }
    let _ = write!(text, ", ${:.2} left", balance.unwrap_or(0.0));
    if let Some(t) = b.prepaid_expires_at {
        let on = t.to_zoned(jiff::tz::TimeZone::UTC);
        let _ = write!(text, ", expires {}", on.strftime("%Y-%m-%d"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budget(limit: Option<f64>, remaining: Option<f64>, prepaid: Option<f64>) -> Budget {
        let mut b = Budget::new(jiff::Timestamp::UNIX_EPOCH);
        b.limit = limit;
        b.remaining = remaining;
        b.prepaid_remaining = prepaid;
        b.resets_at = Some("2026-11-15T00:00:00Z".parse().unwrap());
        b
    }

    #[test]
    fn lines_say_what_is_used_of_the_day() {
        const NONE: (Option<f64>, Option<f64>) = (None, None);
        assert_eq!(
            line(&budget(Some(1.0), Some(0.9502), Some(0.0)), false, NONE),
            "free allowance $0.0498 of $1.00 used today, resets 00:00 UTC"
        );
        assert_eq!(
            line(
                &budget(Some(1.0), Some(0.9502), Some(0.0)),
                false,
                (Some(0.5), Some(0.25))
            ),
            "free allowance $0.0498 of $1.00 used today (cap $0.50), resets 00:00 UTC; \
             no prepaid credit (cap $0.25 a day)"
        );
        let mut credit = budget(Some(1.0), Some(0.0), Some(4.97));
        credit.prepaid_spent_today = Some(0.03);
        credit.prepaid_expires_at = Some("2027-02-14T12:08:05Z".parse().unwrap());
        assert_eq!(
            line(&credit, false, (None, Some(0.25))),
            "free allowance $1.0000 of $1.00 used today, resets 00:00 UTC; \
             credit $0.0300 spent today (cap $0.25), $4.97 left, expires 2027-02-14"
        );
        assert_eq!(
            line(&budget(Some(1.0), Some(0.0), Some(5.0)), false, NONE),
            "free allowance $1.0000 of $1.00 used today, resets 00:00 UTC; \
             credit $0.0000 spent today, $5.00 left"
        );
        let remaining_only = budget(None, Some(0.5), None);
        assert_eq!(
            line(&remaining_only, false, NONE),
            "free allowance $0.5000 left today, resets 00:00 UTC"
        );
        assert_eq!(
            line(&budget(Some(0.1), Some(0.0847), None), true, NONE),
            "free allowance $0.0153 of $0.10 used today (no key), resets 00:00 UTC"
        );
    }

    /// Without a key, one free lookup (past the cache), as OpenAlex
    /// answered it keyless (2026-11-14), says the budget.
    #[tokio::test]
    async fn without_a_key_one_free_lookup_says_the_budget() {
        use aspero_core::testing::{ReplayClient, response};
        use aspero_engine::OpenAlexConfig;
        use aspero_openalex::OpenAlex;
        use std::sync::Arc;

        let headers = [
            ("x-ratelimit-cost-usd", "0"),
            ("x-ratelimit-limit-usd", "0.1"),
            ("x-ratelimit-prepaid-remaining-usd", "0"),
            ("x-ratelimit-remaining-usd", "0.0774"),
            ("x-ratelimit-reset", "45389"),
        ];
        let body = r#"{"id": "https://openalex.org/W2919115771", "title": "A work"}"#;
        let http = Arc::new(ReplayClient::new([response(200, &headers, body)]));
        let mut policy = aspero_core::ResiliencePolicy::default();
        policy.max_retries = 0;
        let engine = Engine::builder()
            .provider(
                Arc::new(OpenAlex::new(OpenAlexConfig::default(), http.clone())),
                policy,
                10,
            )
            .build();
        let (budgets, keyless) = budgets(&engine).await;
        assert!(keyless);
        let requests = http.requests();
        assert_eq!(requests.len(), 1, "one request");
        assert!(
            requests[0].url.contains("/works/W2919115771"),
            "{}",
            requests[0].url
        );
        let [(provider, budget)] = budgets.as_slice() else {
            panic!("{budgets:?}");
        };
        assert_eq!(provider.as_str(), "openalex");
        assert_eq!((budget.limit, budget.remaining), (Some(0.1), Some(0.0774)));
        assert!(
            line(budget, true, (None, None))
                .starts_with("free allowance $0.0226 of $0.10 used today (no key), resets "),
            "{}",
            line(budget, true, (None, None))
        );
    }
}
