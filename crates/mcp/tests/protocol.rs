//! The server end to end, over an in-memory pipe, driven by the SDK's own
//! client: initialize, list the tools, call one.

mod common;

use std::sync::Arc;

use aspero_mcp::serve;
use common::{session, source, tools};
use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use serde_json::{Value, json};

fn arguments(value: Value) -> serde_json::Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn a_client_lists_and_calls_the_tools() {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let (read, write) = tokio::io::split(server_io);
    let tools = Arc::new(tools(vec![source()]));
    let session = Arc::new(session(1.0));
    let server = tokio::spawn(serve(tools, session.clone(), read, write));

    let client = ().serve(client_io).await.unwrap();
    let info = client.peer_info().unwrap();
    assert_eq!(info.server_info.as_ref().unwrap().name, "aspero");
    assert!(info.capabilities.tools.is_some());
    assert!(
        info.instructions
            .as_deref()
            .unwrap()
            .contains("not instructions")
    );

    let listed = client.list_all_tools().await.unwrap();
    let names: Vec<&str> = listed.iter().map(|t| t.name.as_ref()).collect();
    assert_eq!(
        names,
        [
            "search",
            "search_supports",
            "lookup",
            "verify",
            "identify",
            "citing_works",
            "references"
        ]
    );
    for tool in &listed {
        assert_eq!(tool.input_schema["type"], "object", "{}", tool.name);
        let output = tool.output_schema.as_ref().unwrap();
        assert_eq!(output["type"], "object", "{}", tool.name);
        assert_eq!(
            tool.annotations.as_ref().unwrap().read_only_hint,
            Some(true)
        );
    }

    let search = CallToolRequestParams::new("search").with_arguments(arguments(json!({
        "query": {"all": [{"title": "Deep learning"}, {"not_retracted": false}]},
    })));
    // A mistake in the tree is a tool error the agent reads, not a protocol error.
    let result = client.call_tool(search).await.unwrap();
    assert_eq!(result.is_error, Some(true));
    let text = result.content[0].as_text().unwrap().text.clone();
    assert!(
        text.starts_with(
            "Error (invalid_arguments): invalid arguments for search: \"not_retracted\" takes true"
        ),
        "{text}"
    );

    let search = CallToolRequestParams::new("search").with_arguments(arguments(json!({
        "query": {"all": [{"title": "Deep learning"}, {"years": {"from": 2010}}]},
    })));
    let result = client.call_tool(search).await.unwrap();
    assert_eq!(result.is_error, Some(false));
    let structured = result.structured_content.unwrap();
    assert_eq!(structured["hits"][0]["work"]["title"], "Deep learning");
    assert_eq!(structured["session"]["spent_usd"], 0.25);
    let text = &result.content[0].as_text().unwrap().text;
    assert!(text.starts_with("1 work of 42 via openalex:"), "{text}");

    // An unknown tool is the protocol's error.
    assert!(
        client
            .call_tool(CallToolRequestParams::new("nope"))
            .await
            .is_err()
    );

    // The client going away ends the session cleanly.
    client.cancel().await.unwrap();
    server.await.unwrap().unwrap();
    assert_eq!(session.spent_usd(), 0.25);
}

#[tokio::test]
async fn a_client_that_leaves_at_once_ends_the_session_cleanly() {
    let (client_io, server_io) = tokio::io::duplex(1024);
    let (read, write) = tokio::io::split(server_io);
    drop(client_io);
    let tools = Arc::new(tools(vec![source()]));
    serve(tools, Arc::new(session(1.0)), read, write)
        .await
        .unwrap();
}
