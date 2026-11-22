//! The engine tools against OpenAlex itself, answering with recorded
//! responses (the provider's fixtures): set analyses over real reference
//! lists.

mod common;

use std::sync::Arc;

use aspero_core::ResiliencePolicy;
use aspero_core::testing::{ReplayClient, fixture};
use aspero_engine::Engine;
use aspero_mcp::Tools;
use aspero_openalex::{OpenAlex, OpenAlexConfig};
use common::session;
use serde_json::{Value, json};

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../providers/openalex/tests/fixtures"
);

/// The engine tools over OpenAlex without a key, its answers the recorded
/// responses `names`, in order.
fn replaying(names: &[&str]) -> (Tools, Arc<ReplayClient>) {
    let http = Arc::new(ReplayClient::new(
        names.iter().map(|n| fixture(FIXTURES, n)),
    ));
    let openalex = OpenAlex::new(OpenAlexConfig::default(), http.clone());
    let mut policy = ResiliencePolicy::default();
    policy.min_interval = std::time::Duration::ZERO;
    let engine = Engine::builder()
        .provider(Arc::new(openalex), policy, 0)
        .build();
    (Tools::new(engine), http)
}

/// The analyse tool's answer for `args`, checked against its output
/// schema.
async fn analyse(tools: &Tools, session: &aspero_mcp::Session, args: Value) -> aspero_mcp::Output {
    let out = tools.call(session, "analyse", args).await.unwrap();
    let schema = Value::Object(tools.spec("analyse").unwrap().output_schema.clone());
    common::conforms(&out.structured, &schema, "analyse");
    out
}

/// Deep learning (2015) and Attention is all you need (2017), as OpenAlex
/// has them: 62 and 35 references, none shared, neither citing the other.
const PAIR: [&str; 2] = ["W2919115771", "W2626778328"];

#[tokio::test]
async fn an_estimate_is_free_and_says_what_a_run_would_cost() {
    // Seeds resolve by OpenAlex's free single-work lookup.
    let (tools, http) = replaying(&["work_by_openalex"]);
    // Free, so even a spent session may ask.
    let spent = session(0.0);
    let out = analyse(
        &tools,
        &spent,
        json!({"ids": ["W2919115771"], "estimate_only": true}),
    )
    .await;
    let v = &out.structured;
    assert_eq!(http.requests().len(), 1);
    assert_eq!(v["seeds"][0]["status"], "resolved");
    assert_eq!(v["seeds"][0]["references"], 62);
    assert_eq!(
        v["seeds"][0]["work"],
        json!({"canonical_id": "doi:10.1038/nature14539", "title": "Deep learning", "year": 2015, "cited_by_count": 89528})
    );
    // Its 62 references checked for retractions: one list call.
    assert_eq!(
        v["estimate"],
        json!({"requests": 1, "cost_usd": 0.0001, "exact": true})
    );
    assert!(v.get("retracted_references").is_none());
    assert_eq!(out.cost_usd, 0.0);
    assert!(
        out.summary
            .contains("Estimate (exact): 1 paid requests, $0.0001. Nothing was run."),
        "{}",
        out.summary
    );
}

#[tokio::test]
async fn a_pair_is_analysed_from_its_recorded_reference_lists() {
    // Both seeds in one batch lookup ($0.0001), and nothing more: no
    // reference is cited by both, so none is probably missing.
    let (tools, http) = replaying(&["works_batch_openalex"]);
    let session = session(1.0);
    let out = analyse(
        &tools,
        &session,
        json!({"ids": PAIR, "kinds": ["odd_one_out", "probably_missing"]}),
    )
    .await;
    let v = &out.structured;
    assert_eq!(http.requests().len(), 1);
    let references: Vec<&Value> = v["seeds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| &s["references"])
        .collect();
    assert_eq!(references, [&json!(62), &json!(35)]);
    assert_eq!(v["with_references"], 2);
    assert_eq!(
        v["odd_one_out"],
        json!({"complete": true, "incomplete": [], "items": [
            {"seed": 0, "reason": "no_links"},
            {"seed": 1, "reason": "no_links"},
        ]})
    );
    assert_eq!(
        v["probably_missing"],
        json!({"complete": true, "incomplete": [], "items": []})
    );
    assert_eq!(v["complete"], true);
    assert_eq!(v["usage"]["openalex"]["cost_usd"], 0.0001);
    assert_eq!(session.spent_usd(), 0.0001);
    assert!(
        out.summary.starts_with(
            "2 of 2 works resolved, 2 with known references.\nOdd ones out (2):\n  seed 0: no links"
        ),
        "{}",
        out.summary
    );
}

#[tokio::test]
async fn an_analysis_runs_within_what_the_session_has_left() {
    // Half a list call left: the seeds' batch lookup isn't asked, and the
    // result says so. (Nothing is queued: a request would panic.)
    let (tools, http) = replaying(&[]);
    let session = session(0.00005);
    let out = analyse(
        &tools,
        &session,
        json!({"ids": PAIR, "kinds": ["odd_one_out"]}),
    )
    .await;
    let v = &out.structured;
    assert!(http.requests().is_empty());
    assert_eq!(v["seeds"][0]["reason"], "over_budget");
    assert_eq!(v["complete"], false);
    assert_eq!(v["incomplete"][0]["kind"], "budget");
    assert_eq!(session.spent_usd(), 0.0);
    assert!(
        out.summary
            .contains("Resolving the seeds stopped: budget, 0 of 2."),
        "{}",
        out.summary
    );

    // So does the call's own `budget`.
    let session = common::session(1.0);
    let out = analyse(&tools, &session, json!({"ids": PAIR, "budget": 0})).await;
    assert_eq!(out.structured["complete"], false);
    assert!(http.requests().is_empty());
}

#[tokio::test]
async fn a_spent_session_is_refused_an_analysis() {
    let (tools, http) = replaying(&[]);
    let e = tools
        .call(&session(0.0), "analyse", json!({"ids": PAIR}))
        .await
        .unwrap_err();
    assert_eq!(e.kind, aspero_mcp::ToolErrorKind::BudgetSpent);
    assert!(http.requests().is_empty());
    for (args, says) in [
        (
            json!({"ids": []}),
            "`ids` takes 1 to 200 identifiers, not 0",
        ),
        (json!({"ids": PAIR, "kinds": []}), "`kinds` is empty"),
        (
            json!({"ids": PAIR, "budget": -1}),
            "`budget` is zero or more",
        ),
        (
            json!({"ids": PAIR, "kinds": ["cocitation"]}),
            "unknown variant `cocitation`",
        ),
    ] {
        let e = tools
            .call(&session(1.0), "analyse", args)
            .await
            .unwrap_err();
        assert_eq!(e.kind, aspero_mcp::ToolErrorKind::InvalidArguments);
        assert!(e.message.contains(says), "{}", e.message);
    }
}
