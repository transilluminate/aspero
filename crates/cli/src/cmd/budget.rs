//! `aspero budget`: what's used and left of each provider's budget, asked
//! for free (OpenAlex's `/rate-limit`, with a key).

use aspero_engine::{Budget, Engine};
use serde_json::{Map, Value, json};

use super::{Failure, Out, print};
use crate::exit::Exit;

pub async fn run(engine: &Engine, out: Out) -> Result<Exit, Failure> {
    let budgets = engine.refresh_budgets().await;
    if out.format.is_human() {
        if budgets.is_empty() {
            println!(
                "no budget known: OpenAlex says it only to a key (set OPENALEX_API_KEY); \
                 without one, it allows $0.10 a day"
            );
        }
        for (provider, budget) in &budgets {
            println!("{provider}: {}", line(budget));
        }
    } else {
        let list: Vec<Value> = budgets
            .iter()
            .map(|(provider, b)| {
                let mut row = Map::new();
                row.insert("provider".into(), json!(provider));
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

/// "$0.0498 of $1.00 used today, resets 00:00 UTC; $5.00 prepaid left".
fn line(b: &Budget) -> String {
    let mut text = match (b.used(), b.limit, b.remaining) {
        (Some(used), Some(limit), _) => format!("${used:.4} of ${limit:.2} used today"),
        (_, _, Some(left)) => format!("${left:.4} left today"),
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
            line(&budget(Some(1.0), Some(0.9502), Some(0.0))),
            "$0.0498 of $1.00 used today, resets 00:00 UTC"
        );
        assert_eq!(
            line(&budget(Some(1.0), Some(0.0), Some(5.0))),
            "$1.0000 of $1.00 used today, resets 00:00 UTC; $5.00 prepaid left"
        );
        let remaining_only = budget(None, Some(0.5), None);
        assert_eq!(
            line(&remaining_only),
            "$0.5000 left today, resets 00:00 UTC"
        );
    }
}
