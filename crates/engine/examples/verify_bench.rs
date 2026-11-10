//! Benchmark `verify` against citations with known answers.
//!
//!     cargo run -p aspero-engine --example verify_bench [-- bench/citations.jsonl]
//!
//! Each line of the benchmark file is
//! `{"why": …, "claim": <reference text or Claim object>, "expect": [verdicts…], "ids": [ids…]}`:
//! the verdict must be one of `expect`, and if `ids` is given the matched work
//! must carry one of them.
//!
//! Runs live (OPENALEX_API_KEY, ASPERO_MAILTO from the environment) through a
//! dedicated cache in `target/bench-cache`, so the first run costs a few cents
//! of OpenAlex budget and re-runs within the cache TTLs are free. That makes
//! it cheap to change scoring and re-measure. Exits non-zero if any case fails.

use std::path::PathBuf;
use std::sync::Arc;

use aspero_core::{Claim, WorkId};
use aspero_engine::{
    CrossrefConfig, DoiConfig, OpenAlexConfig, SecretString, SqliteCache, StandardConfig,
    Verification, standard,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    why: String,
    claim: ClaimSpec,
    expect: Vec<String>,
    #[serde(default)]
    ids: Vec<WorkId>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ClaimSpec {
    Text(String),
    Fields(Claim),
}

impl ClaimSpec {
    fn claim(&self) -> Claim {
        match self {
            ClaimSpec::Text(text) => Claim::from_input(text),
            ClaimSpec::Fields(claim) => claim.clone(),
        }
    }
}

/// Why a case failed, if it did.
fn judge(case: &Case, v: &Verification) -> Option<String> {
    let verdict = v.verdict.as_str();
    if !case.expect.iter().any(|e| e == verdict) {
        return Some(format!("expected {}, got {verdict}", case.expect.join("/")));
    }
    if case.ids.is_empty() {
        return None;
    }
    let matched = v.best.map(|i| &v.report.candidates[i].ids);
    let ok = matched.is_some_and(|ids| case.ids.iter().any(|want| ids.contains(want)));
    (!ok).then(|| {
        let wanted: Vec<String> = case.ids.iter().map(ToString::to_string).collect();
        format!("matched work lacks {}", wanted.join(" / "))
    })
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("bench/citations.jsonl"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let cases: Vec<Case> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(n, l)| serde_json::from_str(l).unwrap_or_else(|e| panic!("line {}: {e}", n + 1)))
        .collect();

    let env = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    let cache =
        SqliteCache::open(&root.join("target/bench-cache/cache.sqlite")).expect("bench cache");
    let mut config = StandardConfig::default();
    config.mailto = env("ASPERO_MAILTO");
    config.cache = Some(Arc::new(cache));
    let mut openalex = OpenAlexConfig::default();
    openalex.api_key = env("OPENALEX_API_KEY").map(SecretString::from);
    config.openalex = Some(openalex);
    config.crossref = Some(CrossrefConfig::default());
    config.doi = Some(DoiConfig::default());
    let engine = standard(config).expect("engine");

    let claims: Vec<Claim> = cases.iter().map(|c| c.claim.claim()).collect();
    engine.prefetch(&claims).await;

    let (mut passed, mut spent) = (0, 0.0);
    for (case, claim) in cases.iter().zip(&claims) {
        let v = engine.verify(claim, 5).await;
        spent += v
            .report
            .usage
            .iter()
            .filter_map(|(_, u)| u.cost)
            .sum::<f64>();
        let confidence = v
            .best
            .map(|i| format!("{:.2}", v.report.candidates[i].assessment.confidence))
            .unwrap_or_else(|| "  - ".into());
        match judge(case, &v) {
            None => {
                passed += 1;
                println!(
                    "PASS  {:<12} {confidence}  {}",
                    v.verdict.as_str(),
                    case.why
                );
            }
            Some(problem) => {
                println!(
                    "FAIL  {:<12} {confidence}  {}\n      {problem}\n      reason: {}",
                    v.verdict.as_str(),
                    case.why,
                    v.reason
                );
            }
        }
    }
    println!(
        "\n{passed}/{} passed; ${spent:.4} spent on searches",
        cases.len()
    );
    if passed == cases.len() {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}
