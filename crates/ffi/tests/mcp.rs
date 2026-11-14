//! The MCP server as an app starts it: its own handle, a host implemented
//! as the foreign trait, and an agent over loopback HTTP. No network
//! beyond loopback: the engine tool called is the offline one.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use aspero_ffi::mcp::{
    HostTool, HostToolError, HostToolResult, McpActivity, McpCaller, McpError, McpHost, McpKey,
    McpScope, McpServer, McpServerConfig,
};
use aspero_ffi::{Aspero, Config};
use serde_json::{Value, json};

/// A token, and its SHA-256 as the app would keep it (`shasum -a 256`).
const TOKEN: &str = "token-app";
const TOKEN_SHA256: &str = "930eb69d84548d91c02c9c71df25e004feb1a0e0bc49831ea47e0961c19b73c7";

#[derive(Default)]
struct App {
    callers: Mutex<Vec<McpCaller>>,
    activity: Mutex<Vec<McpActivity>>,
}

#[async_trait::async_trait]
impl McpHost for App {
    fn tools(&self) -> Vec<HostTool> {
        vec![HostTool {
            name: "history_recent".into(),
            title: "Recent works".into(),
            description: "Works the user looked at lately.".into(),
            input_schema: json!({"type": "object", "properties": {"limit": {"type": "integer"}}, "required": ["limit"]}).to_string(),
            output_schema: None,
            scope: McpScope::ReadHistory,
        }]
    }

    async fn call(
        &self,
        name: String,
        arguments: String,
        caller: McpCaller,
    ) -> Result<HostToolResult, HostToolError> {
        self.callers.lock().unwrap().push(caller);
        let args: Value = serde_json::from_str(&arguments).unwrap();
        if args["limit"] == 0 {
            return Err(HostToolError::InvalidArguments {
                message: "limit must be at least 1".into(),
            });
        }
        Ok(HostToolResult {
            structured: json!({"tool": name, "works": []}).to_string(),
            summary: "no recent works".into(),
        })
    }

    fn activity(&self, activity: McpActivity) {
        self.activity.lock().unwrap().push(activity);
    }
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// JSON-RPC over HTTP, the answer read from the event stream.
async fn rpc(
    http: &reqwest::Client,
    port: u16,
    token: &str,
    session: &mut Option<String>,
    body: Value,
) -> (u16, Option<Value>) {
    let mut request = http
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .bearer_auth(token)
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json")
        .body(body.to_string());
    if let Some(id) = session {
        request = request
            .header("mcp-session-id", id.as_str())
            .header("mcp-protocol-version", "2025-06-18");
    }
    let response = request.send().await.unwrap();
    let status = response.status().as_u16();
    if let Some(id) = response.headers().get("mcp-session-id") {
        *session = Some(id.to_str().unwrap().into());
    }
    let text = response.text().await.unwrap();
    let answer = text
        .lines()
        .filter_map(|l| l.strip_prefix("data:"))
        .filter_map(|d| serde_json::from_str::<Value>(d.trim()).ok())
        .find(|v| v.get("id").is_some());
    (status, answer)
}

fn config(port: u16, scopes: Vec<McpScope>) -> McpServerConfig {
    McpServerConfig {
        port,
        keys: vec![McpKey {
            id: "k1".into(),
            name: "Test agent".into(),
            scopes,
            sha256: TOKEN_SHA256.into(),
            daily_budget_usd: 1.0,
        }],
        session_budget_usd: 0.25,
        engine_tools: true,
    }
}

#[test]
fn an_app_serves_its_tools_to_an_agent() {
    let dir = std::env::temp_dir().join(format!("aspero-ffi-mcp-{}", std::process::id()));
    let aspero = Aspero::new(Config {
        cache_dir: Some(dir.to_string_lossy().into()),
        ..Config::default()
    })
    .unwrap();
    let app = Arc::new(App::default());
    let port = free_port();
    let rt = runtime();
    let server = rt
        .block_on(McpServer::start(
            aspero.clone(),
            config(port, vec![McpScope::Sources, McpScope::ReadHistory]),
            Some(app.clone() as Arc<dyn McpHost>),
        ))
        .unwrap();
    assert_eq!(server.port(), port);

    // A second server can't take the port.
    let taken = rt.block_on(McpServer::start(aspero.clone(), config(port, vec![]), None));
    assert_eq!(taken.err(), Some(McpError::PortInUse { port }));

    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let mut session = None;
    rt.block_on(async {
        let (status, _) = rpc(&http, port, "wrong", &mut None, json!({})).await;
        assert_eq!(status, 401);
        let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "test", "version": "1"}}});
        let (status, _) = rpc(&http, port, TOKEN, &mut session, init).await;
        assert_eq!(status, 200);
        let list = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
        let (_, answer) = rpc(&http, port, TOKEN, &mut session, list).await;
        let names: Vec<String> = answer.unwrap()["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(names.len(), 8);
        assert_eq!(names.last().unwrap(), "history_recent");

        let call = json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"name": "history_recent", "arguments": {"limit": 5}}});
        let (_, answer) = rpc(&http, port, TOKEN, &mut session, call).await;
        let result = &answer.unwrap()["result"];
        assert_eq!(result["isError"], false);
        assert_eq!(result["structuredContent"]["tool"], "history_recent");

        let call = json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": {"name": "history_recent", "arguments": {"limit": 0}}});
        let (_, answer) = rpc(&http, port, TOKEN, &mut session, call).await;
        let result = &answer.unwrap()["result"];
        assert_eq!(result["isError"], true);
        assert_eq!(
            result["content"][0]["text"],
            "Error (invalid_arguments): limit must be at least 1"
        );

        // The engine's offline tool, through the app's own handle.
        let call = json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call",
            "params": {"name": "search_supports", "arguments": {"query": {"title": "sepsis"}}}});
        let (_, answer) = rpc(&http, port, TOKEN, &mut session, call).await;
        assert_eq!(
            answer.unwrap()["result"]["structuredContent"]["supported"],
            true
        );
    });

    assert_eq!(
        app.callers.lock().unwrap()[0],
        McpCaller {
            key_id: "k1".into(),
            key_name: "Test agent".into(),
            scopes: vec![McpScope::Sources, McpScope::ReadHistory],
        }
    );
    let status = server.status();
    assert!(status.running);
    assert_eq!(status.sessions, 1);
    assert_eq!(status.last_key_name.as_deref(), Some("Test agent"));
    assert!(status.keys[0].last_used_at.is_some());

    // Revoked from the app's thread: refused at once.
    server.set_keys(vec![]);
    rt.block_on(async {
        let list = json!({"jsonrpc": "2.0", "id": 6, "method": "tools/list"});
        let (status, _) = rpc(&http, port, TOKEN, &mut session, list).await;
        assert_eq!(status, 401);
    });

    rt.block_on(server.stop());
    assert!(!server.status().running);
    std::net::TcpListener::bind(("127.0.0.1", port)).expect("the port is free");
    for _ in 0..100 {
        if app.activity.lock().unwrap().len() >= 3 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let activity = app.activity.lock().unwrap().clone();
    let said: Vec<&str> = activity.iter().map(|a| a.summary.as_str()).collect();
    assert_eq!(
        said,
        [
            "no recent works",
            "error: limit must be at least 1",
            "checked whether a search is supported"
        ]
    );
    drop(server);
    drop(aspero);
    let _ = std::fs::remove_dir_all(dir);
}
