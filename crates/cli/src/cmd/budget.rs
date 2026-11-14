//! `aspero budget`: what's used and left of each provider's budget, asked
//! for free (OpenAlex's `/rate-limit` with a key; without one, a lookup of
//! one work, which is free too and says the budget in its headers).

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
            println!("{provider}: {}", line(budget, no_key));
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
                put("limit", b.limit);
                put("used", b.used());
                put("remaining", b.remaining);
                put("prepaid_remaining", b.prepaid_remaining);
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

/// "$0.0498 of $1.00 used today, resets 00:00 UTC; $5.00 prepaid left",
/// "(no key)" after "today" when OpenAlex has none.
fn line(b: &Budget, no_key: bool) -> String {
    let key = if no_key { " (no key)" } else { "" };
    let mut text = match (b.used(), b.limit, b.remaining) {
        (Some(used), Some(limit), _) => format!("${used:.4} of ${limit:.2} used today{key}"),
        (_, _, Some(left)) => format!("${left:.4} left today{key}"),
        _ => "budget unknown".to_owned(),
    };
    if let Some(t) = b.resets_at {
        let at = t.to_zoned(jiff::tz::TimeZone::UTC);
        text.push_str(&format!(", resets {} UTC", at.strftime("%H:%M")));
    }
    if let Some(prepaid) = b.prepaid_remaining.filter(|p| *p > 0.0) {
        text.push_str(&format!("; ${prepaid:.2} prepaid left"));
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
        assert_eq!(
            line(&budget(Some(1.0), Some(0.9502), Some(0.0)), false),
            "$0.0498 of $1.00 used today, resets 00:00 UTC"
        );
        assert_eq!(
            line(&budget(Some(1.0), Some(0.0), Some(5.0)), false),
            "$1.0000 of $1.00 used today, resets 00:00 UTC; $5.00 prepaid left"
        );
        let remaining_only = budget(None, Some(0.5), None);
        assert_eq!(
            line(&remaining_only, false),
            "$0.5000 left today, resets 00:00 UTC"
        );
        assert_eq!(
            line(&budget(Some(0.1), Some(0.0847), None), true),
            "$0.0153 of $0.10 used today (no key), resets 00:00 UTC"
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
            line(budget, true).starts_with("$0.0226 of $0.10 used today (no key), resets "),
            "{}",
            line(budget, true)
        );
    }
}
