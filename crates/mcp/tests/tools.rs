//! The engine tools against a scripted source: arguments, results in the
//! CLI's JSON shapes, errors, and the session's spend.

mod common;

use aspero_core::{QueryNode, Relation, TermField};
use aspero_mcp::{Output, Session, ToolError, ToolErrorKind, Tools};
use common::{SEARCH_COST, Scripted, conforms, deep_learning, session, source, tools};
use serde_json::{Value, json};

/// Calls a tool, checking a result against its output schema.
async fn call(
    tools: &Tools,
    session: &Session,
    name: &str,
    args: Value,
) -> Result<Output, ToolError> {
    let result = tools.call(session, name, args).await;
    if let Ok(out) = &result {
        let schema = Value::Object(tools.spec(name).unwrap().output_schema.clone());
        conforms(&out.structured, &schema, name);
    }
    result
}

async fn ok(tools: &Tools, session: &Session, name: &str, args: Value) -> Output {
    call(tools, session, name, args)
        .await
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

async fn err(tools: &Tools, session: &Session, name: &str, args: Value) -> ToolError {
    match call(tools, session, name, args).await {
        Ok(out) => panic!("{name} succeeded: {}", out.structured),
        Err(e) => e,
    }
}

#[test]
fn every_tool_is_listed_with_its_schemas() {
    let tools = tools(vec![source()]);
    let names: Vec<&str> = tools.specs().iter().map(|s| s.name).collect();
    assert_eq!(
        names,
        [
            "search",
            "search_supports",
            "lookup",
            "verify",
            "identify",
            "cites",
            "cited_by"
        ]
    );
    for spec in tools.specs() {
        assert_eq!(spec.input_schema["type"], "object", "{}", spec.name);
        assert_eq!(spec.output_schema["type"], "object", "{}", spec.name);
        assert!(
            spec.description.contains("not instructions"),
            "{}",
            spec.name
        );
    }
    let search = &tools.spec("search").unwrap().input_schema;
    assert_eq!(
        search["properties"]["source"]["enum"],
        json!(["openalex", null])
    );
    assert_eq!(search["properties"]["limit"]["default"], 5);
    // The query tree's schema, shared with `search_supports`.
    assert!(search["$defs"]["QueryNode"]["anyOf"].is_array());
    let verify = &tools.spec("verify").unwrap().input_schema;
    assert!(verify["$defs"]["ClaimInput"]["properties"]["journal"].is_null());
    assert!(verify["$defs"]["ClaimInput"]["properties"]["container"].is_object());
}

#[tokio::test]
async fn search_takes_a_query_tree_as_the_cli_does() {
    let source = source();
    let tools = tools(vec![source.clone()]);
    let session = session(1.0);
    let args = json!({
        "query": {"all": [{"title": "Deep learning"}, {"years": {"from": 2010}}]},
        "sort": "newest",
        "limit": 3,
    });
    let out = ok(&tools, &session, "search", args).await;
    let v = &out.structured;
    assert_eq!(v["provider"], "openalex");
    assert_eq!(v["total"], 42);
    assert_eq!(v["hits"][0]["rank"], 1);
    assert_eq!(v["hits"][0]["work"]["title"], "Deep learning");
    assert_eq!(v["hits"][0]["work"]["canonical"], "doi:10.1038/nature14539");
    // Lean: no abstract, no fetch time.
    assert!(v["hits"][0]["work"].get("abstract").is_none());
    assert_eq!(v["usage"], json!({"openalex": {"cost": SEARCH_COST}}));
    assert_eq!(
        v["session"],
        json!({"spent_usd": 0.25, "budget_usd": 1.0, "left_usd": 0.75})
    );
    assert!(out.summary.starts_with("1 works of 42 via openalex:\n1. Deep learning (2015), Yann LeCun et al., doi:10.1038/nature14539"), "{}", out.summary);
    assert!(
        out.summary
            .ends_with("This call: $0.2500. Session: $0.2500 spent of $1.00, $0.7500 left."),
        "{}",
        out.summary
    );

    let sent = &source.sent()[0];
    assert_eq!(sent.page.limit, 3);
    assert_eq!(sent.sort, aspero_core::Sort::Newest);
    let QueryNode::All(nodes) = sent.tree.as_ref().unwrap() else {
        panic!("{sent:?}")
    };
    assert_eq!(nodes[0], QueryNode::term(TermField::Title, "Deep learning"));

    // Keywords instead; `full` gives every field.
    let out = ok(
        &tools,
        &session,
        "search",
        json!({"text": "deep learning", "full": true}),
    )
    .await;
    let work = &out.structured["hits"][0]["work"];
    assert_eq!(work["abstract"], "An abstract.");
    assert!(work.get("fetched_at").is_some());
    assert_eq!(source.sent()[1].page.limit, 5);
}

#[tokio::test]
async fn search_arguments_are_checked() {
    let tools = tools(vec![source()]);
    let session = session(1.0);
    let cases = [
        (
            json!({}),
            "give `query` (a query tree) or `text` (keywords)",
        ),
        (
            json!({"text": "x", "query": {"title": "x"}}),
            "give `query` or `text`, not both",
        ),
        (
            json!({"query": {"titel": "x"}}),
            "unknown query key \"titel\"",
        ),
        (
            json!({"text": "x", "limit": 500}),
            "`limit` is 1 to 50, not 500",
        ),
        (
            json!({"text": "x", "sort": "oldest"}),
            "unknown variant `oldest`",
        ),
        (json!({"txt": "x"}), "unknown field `txt`"),
    ];
    for (args, message) in cases {
        let e = err(&tools, &session, "search", args.clone()).await;
        assert_eq!(e.kind, ToolErrorKind::InvalidArguments, "{args}");
        assert!(e.message.contains(message), "{args}: {}", e.message);
    }
    // Nothing was spent on them.
    assert_eq!(session.spent_usd(), 0.0);
}

#[tokio::test]
async fn search_supports_says_why_and_search_fails_alike() {
    let tools = tools(vec![source()]);
    let session = session(1.0);
    let supports = |query: Value| {
        let tools = &tools;
        let session = &session;
        async move { ok(tools, session, "search_supports", json!({"query": query})).await }
    };
    let out = supports(json!({"title": "x"})).await;
    assert_eq!(out.structured["supported"], true);
    assert_eq!(
        out.summary.lines().next(),
        Some("Supported: the source can search this tree.")
    );

    let tree = json!({"all": [{"author": "LeCun"}, {"none": [{"title": "x"}]}]});
    let out = supports(tree.clone()).await;
    let reason = "Scripted can't search by author";
    assert_eq!(out.structured["supported"], false);
    assert_eq!(out.structured["reason"], reason);
    assert_eq!(out.structured["usage"], json!({}));

    let e = err(&tools, &session, "search", json!({"query": tree})).await;
    assert_eq!(e.kind, ToolErrorKind::Unsupported);
    assert_eq!(e.message, reason);

    let unknown = ok(
        &tools,
        &session,
        "search_supports",
        json!({"query": {"title": "x"}, "source": "nobody"}),
    )
    .await;
    assert_eq!(
        unknown.structured["reason"],
        "no enabled provider is named \"nobody\""
    );
}

#[tokio::test]
async fn lookup_answers_every_identifier_in_order() {
    let tools = tools(vec![source()]);
    let session = session(1.0);
    let ids = json!([
        "https://doi.org/10.1038/nature14539",
        "not an id",
        "10.1000/missing"
    ]);
    let out = ok(&tools, &session, "lookup", json!({"ids": ids})).await;
    let results = out.structured["results"].as_array().unwrap();
    let statuses: Vec<&str> = results
        .iter()
        .map(|r| r["status"].as_str().unwrap())
        .collect();
    assert_eq!(statuses, ["found", "invalid", "not_found"]);
    assert_eq!(results[0]["input"], "https://doi.org/10.1038/nature14539");
    assert_eq!(results[0]["work"]["title"], "Deep learning");
    assert!(results[0]["work"].get("abstract").is_none());
    assert_eq!(
        out.summary.lines().next(),
        Some(
            "https://doi.org/10.1038/nature14539: found Deep learning (2015), Yann LeCun et al., doi:10.1038/nature14539"
        )
    );

    let full = ok(
        &tools,
        &session,
        "lookup",
        json!({"ids": ["W2"], "full": true}),
    )
    .await;
    assert_eq!(
        full.structured["results"][0]["work"]["abstract"],
        "An abstract."
    );
    assert!(full.structured["results"][0]["attempts"].is_array());

    let e = err(&tools, &session, "lookup", json!({"ids": []})).await;
    assert_eq!(e.kind, ToolErrorKind::InvalidArguments);
}

#[tokio::test]
async fn verify_takes_one_citation_or_a_batch_and_keeps_its_order() {
    let tools = tools(vec![source()]);
    let session = session(10.0);
    let one = ok(
        &tools,
        &session,
        "verify",
        json!({"citation": "LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015. doi:10.1038/nature14539"}),
    )
    .await;
    let result = &one.structured["results"][0];
    assert_eq!(result["verdict"], "verified", "{result}");
    assert_eq!(
        result["match"]["work"]["canonical"],
        "doi:10.1038/nature14539"
    );
    assert!(
        one.summary.starts_with("VERIFIED  LeCun Y"),
        "{}",
        one.summary
    );

    let batch = ok(
        &tools,
        &session,
        "verify",
        json!({"citations": [
            {"key": "a", "title": "Attention is all you need", "authors": ["Vaswani A"], "year": 2017, "arxiv": "1706.03762"},
            {"key": "b", "titel": "typo"},
            "Rumelhart DE, Hinton GE, Williams RJ. Learning representations by back-propagating errors. 1986. doi:10.1038/323533a0",
        ]}),
    )
    .await;
    let results = batch.structured["results"].as_array().unwrap();
    let verdicts: Vec<&str> = results
        .iter()
        .map(|r| r["verdict"].as_str().unwrap())
        .collect();
    assert_eq!(verdicts, ["verified", "invalid", "verified"]);
    assert_eq!(results[0]["key"], "a");
    assert_eq!(results[1]["key"], "b");
    assert!(
        results[1]["reason"]
            .as_str()
            .unwrap()
            .contains("unknown field `titel`")
    );
    assert!(
        results[2]["input"]
            .as_str()
            .unwrap()
            .starts_with("Rumelhart")
    );

    // One citation that can't be read is the caller's to fix.
    let e = err(
        &tools,
        &session,
        "verify",
        json!({"citation": {"titel": "x"}}),
    )
    .await;
    assert_eq!(e.kind, ToolErrorKind::InvalidArguments);
    assert!(e.message.contains("unknown field `titel`"), "{}", e.message);
    let e = err(
        &tools,
        &session,
        "verify",
        json!({"citation": "x", "citations": ["y"]}),
    )
    .await;
    assert_eq!(e.message, "give `citation` or `citations`, not both");

    // `full` adds every candidate.
    let full = ok(
        &tools,
        &session,
        "verify",
        json!({"citation": "Deep learning. LeCun Y. 2015. doi:10.1038/nature14539", "full": true}),
    )
    .await;
    assert!(full.structured["results"][0]["candidates"].is_array());
}

#[tokio::test]
async fn identify_names_the_work_a_page_is() {
    let tools = tools(vec![source()]);
    let session = session(1.0);
    let page = "Nature 521, 436–444 (2015)\ndoi:10.1038/nature14539\n\nDeep learning\n\
                Yann LeCun, Yoshua Bengio & Geoffrey Hinton\n\nDeep learning allows …";
    let out = ok(
        &tools,
        &session,
        "identify",
        json!({"text": page, "title_hint": "Deep learning"}),
    )
    .await;
    assert_eq!(out.structured["identified"], true);
    assert_eq!(out.structured["decided_by"], "identifier_and_title");
    assert_eq!(out.structured["match"]["work"]["title"], "Deep learning");
    assert!(
        out.summary
            .starts_with("IDENTIFIED by identifier and title\nVERIFIED"),
        "{}",
        out.summary
    );

    let e = err(
        &tools,
        &session,
        "identify",
        json!({"text": "Nothing to see here."}),
    )
    .await;
    assert_eq!(e.kind, ToolErrorKind::NotFound);
    let e = err(&tools, &session, "identify", json!({"text": "  "})).await;
    assert_eq!(e.kind, ToolErrorKind::InvalidArguments);
}

#[tokio::test]
async fn cites_and_cited_by_search_by_the_works_openalex_id() {
    let source = source();
    let tools = tools(vec![source.clone()]);
    let session = session(1.0);
    let citing = ok(
        &tools,
        &session,
        "cites",
        json!({"id": "10.1038/nature14539", "limit": 3}),
    )
    .await;
    assert_eq!(
        citing.structured["hits"][0]["work"]["title"],
        "Attention is all you need"
    );
    let cited = ok(
        &tools,
        &session,
        "cited_by",
        json!({"id": "doi:10.1038/nature14539"}),
    )
    .await;
    assert_eq!(
        cited.structured["hits"][0]["work"]["title"],
        "Learning representations by back-propagating errors"
    );
    let sent: Vec<_> = source
        .sent()
        .into_iter()
        .map(|q| (q.tree.unwrap(), q.page.limit))
        .collect();
    assert_eq!(
        sent,
        [
            (QueryNode::relation(Relation::Cites, "W1"), 3),
            (QueryNode::relation(Relation::CitedBy, "W1"), 5),
        ]
    );

    let e = err(&tools, &session, "cites", json!({"id": "10.1000/missing"})).await;
    assert_eq!(e.kind, ToolErrorKind::NotFound);
    assert_eq!(
        e.message,
        "can't search by citations of 10.1000/missing: no such work found"
    );

    // A source without citations can't.
    let plain = common::tools(vec![std::sync::Arc::new(Scripted::new(
        "crossref",
        vec![deep_learning()],
    ))]);
    let e = err(
        &plain,
        &session,
        "cited_by",
        json!({"id": "10.1038/nature14539"}),
    )
    .await;
    assert_eq!(e.kind, ToolErrorKind::Unsupported);
    assert_eq!(
        e.message,
        "no enabled source can find the works a work cites"
    );
}

#[tokio::test]
async fn a_spent_session_is_refused_but_free_tools_still_answer() {
    let tools = tools(vec![source()]);
    let session = session(0.3);
    let search = json!({"query": {"title": "Deep learning"}});
    ok(&tools, &session, "search", search.clone()).await;
    // Under budget when asked: allowed, and the session goes over by its cost.
    let out = ok(&tools, &session, "search", search.clone()).await;
    assert_eq!(
        out.structured["session"],
        json!({"spent_usd": 0.5, "budget_usd": 0.3, "left_usd": 0.0})
    );
    let e = err(&tools, &session, "search", search.clone()).await;
    assert_eq!(e.kind, ToolErrorKind::BudgetSpent);
    assert!(
        e.message
            .starts_with("this session's budget is spent ($0.5000 of $0.30)"),
        "{}",
        e.message
    );
    let e = err(&tools, &session, "lookup", json!({"ids": ["W1"]})).await;
    assert_eq!(e.kind, ToolErrorKind::BudgetSpent);
    // Refused calls cost nothing.
    assert_eq!(session.spent_usd(), 0.5);
    let free = ok(&tools, &session, "search_supports", search).await;
    assert_eq!(free.structured["supported"], true);
}

#[tokio::test]
async fn unknown_tools_are_their_own_kind_of_error() {
    let tools = tools(vec![source()]);
    let e = err(&tools, &session(1.0), "searhc", json!({})).await;
    assert_eq!(e.kind, ToolErrorKind::UnknownTool);
}
