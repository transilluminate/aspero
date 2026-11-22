//! `analyse`: the engine's analyses of a set of works (`Engine::analyse`),
//! as `aspero analyse` gives them, with works cut to what an agent uses.

use aspero_engine::AnalysisKind;
use serde_json::{Map, Value, json};

use crate::Session;
use crate::args::{AnalyseArgs, Kind};
use crate::tools::{Answer, ToolError, Tools};

/// Most works in a set.
pub const MAX_SEEDS: usize = 200;
/// Probably-missing and related works listed unless the caller asks for
/// more.
pub(crate) const DEFAULT_ANALYSE_LIMIT: u32 = 10;
/// What an analysis may spend unless the caller says, in USD: about 200
/// OpenAlex list calls, more than a 50-work set's related works take.
pub(crate) const DEFAULT_ANALYSE_USD: f64 = 0.02;

/// What a call may spend, from its arguments: nothing for an estimate,
/// else its `budget`.
pub(crate) fn cap(arguments: &Value) -> f64 {
    if arguments["estimate_only"].as_bool() == Some(true) {
        return 0.0;
    }
    arguments["budget"]
        .as_f64()
        .unwrap_or(DEFAULT_ANALYSE_USD)
        .max(0.0)
}

impl Tools {
    /// Runs the analyses within `a.budget` and what's left of the
    /// session's, so the call never takes the session past its budget.
    pub(crate) async fn analyse(&self, session: &Session, a: AnalyseArgs) -> Answer {
        if a.ids.is_empty() || a.ids.len() > MAX_SEEDS {
            return Err(ToolError::invalid(format!(
                "`ids` takes 1 to {MAX_SEEDS} identifiers, not {}",
                a.ids.len()
            ))
            .into());
        }
        if a.kinds.is_empty() {
            return Err(ToolError::invalid(
                "`kinds` is empty: leave it out for retracted_references, odd_one_out and \
                 probably_missing",
            )
            .into());
        }
        if a.limit == 0 || a.limit > crate::MAX_LIMIT {
            return Err(ToolError::invalid(format!(
                "`limit` is 1 to {}, not {}",
                crate::MAX_LIMIT,
                a.limit
            ))
            .into());
        }
        // NaN too.
        if a.budget.is_nan() || a.budget < 0.0 {
            return Err(ToolError::invalid("`budget` is zero or more US dollars").into());
        }
        let kinds: Vec<AnalysisKind> = a
            .kinds
            .iter()
            .map(|k| match k {
                Kind::RetractedReferences => AnalysisKind::RetractedReferences,
                Kind::OddOneOut => AnalysisKind::OddOneOut,
                Kind::ProbablyMissing => AnalysisKind::ProbablyMissing { limit: a.limit },
                Kind::Related => AnalysisKind::Related { limit: a.limit },
            })
            .collect();
        let budget = a.budget.min(session.left_usd());
        let analysis = self
            .engine()
            .analyse(&a.ids, &kinds, Some(budget), a.estimate_only)
            .await?;
        let mut doc = aspero_json::graph::analysis(&analysis, false);
        slim(&mut doc);
        Ok((doc, analysis.usage))
    }
}

/// Cuts every work in an analysis to `canonical_id`, `title`, `year` and
/// `cited_by_count`: the findings say why each is there, and `lookup`
/// gives the rest.
fn slim(v: &mut Value) {
    match v {
        Value::Object(map) => {
            for key in ["work", "cites"] {
                if let Some(work) = map.get_mut(key).filter(|w| w.is_object()) {
                    *work = slim_work(work);
                }
            }
            map.values_mut().for_each(slim);
        }
        Value::Array(items) => items.iter_mut().for_each(slim),
        _ => {}
    }
}

fn slim_work(work: &Value) -> Value {
    let mut out = Map::new();
    for key in ["canonical_id", "title"] {
        if let Some(v) = work.get(key) {
            out.insert(key.into(), v.clone());
        }
    }
    let year = work["issued"]
        .as_str()
        .and_then(|d| d.get(..4))
        .and_then(|y| y.parse::<i32>().ok());
    if let Some(year) = year {
        out.insert("year".into(), json!(year));
    }
    if let Some(n) = work.get("cited_by_count") {
        out.insert("cited_by_count".into(), n.clone());
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn works_in_an_analysis_keep_their_id_title_year_and_count() {
        let work = json!({
            "source": "openalex",
            "ids": ["doi:10.1/a", "openalex:W1"],
            "canonical_id": "doi:10.1/a",
            "title": "T",
            "authors": [{"name": "A", "orcid": "0000-0001"}],
            "issued": "2019-05-01",
            "cited_by_count": 7,
            "url": "https://x",
        });
        let mut doc = json!({
            "seeds": [{"index": 0, "work": work}],
            "retracted_references": {"items": [{"seed": 0, "cites": work, "source": "openalex"}]},
        });
        slim(&mut doc);
        let lean =
            json!({"canonical_id": "doi:10.1/a", "title": "T", "year": 2019, "cited_by_count": 7});
        assert_eq!(doc["seeds"][0]["work"], lean);
        assert_eq!(doc["retracted_references"]["items"][0]["cites"], lean);
        assert_eq!(
            doc["retracted_references"]["items"][0]["source"],
            "openalex"
        );
    }

    #[test]
    fn an_estimate_holds_nothing_and_a_run_its_budget() {
        assert_eq!(cap(&json!({"estimate_only": true, "budget": 1.0})), 0.0);
        assert_eq!(cap(&json!({"budget": 0.5})), 0.5);
        assert_eq!(cap(&json!({})), DEFAULT_ANALYSE_USD);
    }
}
