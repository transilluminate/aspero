//! Benchmark `verify` against citations with known answers.
//!
//!     cargo run -p aspero-engine --example verify_bench [-- bench/citations.jsonl]
//!
//! Each line of the benchmark file is
//! `{"why": …, "claim": <reference text or Claim object>, "expect": [verdicts…], "ids": [ids…],
//! "retracted": bool}`: the verdict must be one of `expect`; if `ids` is given
//! the matched work must carry one of them; if `retracted` is given, the
//! matched work must (not) be flagged as retracted.
//!
//! Runs live (`OPENALEX_API_KEY`, `ASPERO_MAILTO` from the environment)
//! through an HTTP tape, `target/bench-tape.sqlite` (or `ASPERO_HTTP_TAPE`):
//! each distinct request is paid for once, ever, and re-runs replay it, so
//! changing scoring and re-measuring is free and reproducible.
//! `ASPERO_HTTP_TAPE_MODE=replay-only` runs offline, failing what isn't on
//! the tape. With a tape, each run starts from an empty answer cache (in
//! memory), so it asks what a first run asks and its results don't depend
//! on the cache's age. `ASPERO_HTTP_TAPE=off` runs without a tape, through
//! a dedicated cache in `target/bench-cache` (re-runs within its lifetimes
//! free) as before. Exits non-zero if any case fails.
//!
//! Once the day's OpenAlex allowance is used up, prepaid credit is spent up
//! to $5 a day (`ASPERO_OPENALEX_CREDIT_CAP_USD`), not the library's $0.25,
//! so a run isn't stopped part-way.

use std::path::PathBuf;
use std::sync::Arc;

use aspero_core::{Claim, WorkId};
use aspero_engine::{HttpTape, SecretString, SqliteCache, StandardConfig, Verification, standard};
use serde::Deserialize;

/// The most prepaid credit a bench run spends in a day, unless
/// `ASPERO_OPENALEX_CREDIT_CAP_USD` says otherwise.
const BENCH_CREDIT_CAP_USD: f64 = 5.0;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    why: String,
    claim: ClaimSpec,
    expect: Vec<String>,
    #[serde(default)]
    ids: Vec<WorkId>,
    /// If given, whether the matched work must be flagged as retracted.
    #[serde(default)]
    retracted: Option<bool>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ClaimSpec {
    Text(String),
    Fields(Box<Claim>),
}

impl ClaimSpec {
    fn claim(&self) -> Claim {
        match self {
            ClaimSpec::Text(text) => Claim::from_input(text),
            ClaimSpec::Fields(claim) => (**claim).clone(),
        }
    }
}

/// Why a case failed, if it did.
fn judge(case: &Case, v: &Verification) -> Option<String> {
    let verdict = v.verdict.as_str();
    if !case.expect.iter().any(|e| e == verdict) {
        return Some(format!("expected {}, got {verdict}", case.expect.join("/")));
    }
    if let Some(want) = case.retracted {
        let got = v
            .best_match
            .is_some_and(|i| v.report.candidates[i].retraction().is_some());
        if got != want {
            return Some(format!("expected retracted={want}, got {got}"));
        }
    }
    if case.ids.is_empty() {
        return None;
    }
    let matched = v.best_match.map(|i| &v.report.candidates[i].ids);
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
        .map_or_else(|| root.join("bench/citations.jsonl"), PathBuf::from);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let cases: Vec<Case> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(n, l)| serde_json::from_str(l).unwrap_or_else(|e| panic!("line {}: {e}", n + 1)))
        .collect();

    let env = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    let tape_path = std::env::var("ASPERO_HTTP_TAPE")
        .unwrap_or_else(|_| root.join("target/bench-tape.sqlite").display().to_string());
    let tape = HttpTape::from_vars(
        Some(&tape_path),
        std::env::var("ASPERO_HTTP_TAPE_MODE").ok().as_deref(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let cache = match &tape {
        Some(_) => SqliteCache::open_in_memory(),
        None => SqliteCache::open(&root.join("target/bench-cache/cache.sqlite")),
    }
    .expect("bench cache");
    let mut config = StandardConfig::all(env("OPENALEX_API_KEY").map(SecretString::from));
    config.mailto = env("ASPERO_MAILTO");
    // Not the library's $0.25: a bench run shouldn't stop part-way once
    // the allowance is gone.
    config.openalex_daily_credit_cap_usd = Some(env("ASPERO_OPENALEX_CREDIT_CAP_USD").map_or(
        BENCH_CREDIT_CAP_USD,
        |v| {
            v.trim()
                .parse()
                .unwrap_or_else(|_| panic!("ASPERO_OPENALEX_CREDIT_CAP_USD: {v:?} isn't USD"))
        },
    ));
    config.cache = Some(Arc::new(cache));
    config.http_tape = tape;
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
        let confidence = v.best_match.map_or_else(
            || "  - ".into(),
            |i| format!("{:.2}", v.report.candidates[i].assessment.confidence),
        );
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
                    v.reason_text
                );
                for e in &v.report.errors {
                    println!("      error: {e}");
                }
            }
        }
    }
    println!(
        "\n{passed}/{} passed; ${spent:.4} spent on searches (replays are free)",
        cases.len()
    );
    if passed == cases.len() {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}
