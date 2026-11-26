//! The MCP server as an app starts it: its own handle, a host implemented
//! as the foreign trait, and an agent over loopback HTTP. No network
//! beyond loopback: the engine tool called is the offline one.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use aspero_ffi::mcp::{
    HostImageType, HostTool, HostToolError, HostToolImage, HostToolResult, McpActivity, McpCaller,
    McpError, McpErrorKind, McpHoldings, McpHost, McpKey, McpServer, McpServerConfig,
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
    /// Tools beyond `history_recent`, for `hostToolsChanged()`.
    more: Mutex<Vec<HostTool>>,
}

/// A tool as an app declares it, its hints left to their defaults.
fn history_remove(scope: &str) -> HostTool {
    HostTool {
        name: "history_remove".into(),
        title: "Remove a work".into(),
        description: "Takes a work out of the user's history.".into(),
        input_schema: json!({"type": "object"}).to_string(),
        output_schema: None,
        scope: scope.into(),
        read_only: false,
        destructive: true,
        idempotent: false,
    }
}

#[async_trait::async_trait]
impl McpHost for App {
    fn tools(&self) -> Vec<HostTool> {
        let mut tools = vec![HostTool {
            name: "history_recent".into(),
            title: "Recent works".into(),
            description: "Works the user looked at lately.".into(),
            input_schema: json!({"type": "object", "properties": {"limit": {"type": "integer"}}, "required": ["limit"]}).to_string(),
            output_schema: None,
            scope: "history.read".into(),
            read_only: true,
            destructive: false,
            idempotent: true,
        }];
        tools.extend(self.more.lock().unwrap().iter().cloned());
        tools
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
            images: Vec::new(),
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

fn key(scopes: &[&str]) -> McpKey {
    McpKey {
        id: "k1".into(),
        name: "Test agent".into(),
        scopes: scopes.iter().map(|s| (*s).to_owned()).collect(),
        sha256: TOKEN_SHA256.into(),
        daily_budget_usd: 1.0,
    }
}

fn config(port: u16, scopes: &[&str]) -> McpServerConfig {
    McpServerConfig {
        port,
        keys: vec![key(scopes)],
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
            config(port, &["sources", "history.read"]),
            Some(app.clone() as Arc<dyn McpHost>),
            None,
        ))
        .unwrap();
    assert_eq!(server.port(), port);

    // A second server can't take the port.
    let taken = rt.block_on(McpServer::start(
        aspero.clone(),
        config(port, &[]),
        None,
        None,
    ));
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
        let answer = answer.unwrap();
        let tools = answer["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        // The eight engine tools, then the app's.
        assert_eq!(names.len(), 9);
        assert_eq!(names.last().unwrap(), &"history_recent");
        // Annotated as the app declared it.
        let hints = &tools.last().unwrap()["annotations"];
        assert_eq!(hints["readOnlyHint"], true, "{hints}");
        assert_eq!(hints["destructiveHint"], false, "{hints}");
        assert_eq!(hints["idempotentHint"], true, "{hints}");

        let call = json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"name": "history_recent", "arguments": {"limit": 5}}});
        let (_, answer) = rpc(&http, port, TOKEN, &mut session, call).await;
        let result = &answer.unwrap()["result"];
        assert_eq!(result["isError"], false);
        assert_eq!(result["structuredContent"]["tool"], "history_recent");
        // No images: its text alone, as before images could be sent.
        assert_eq!(
            result["content"],
            json!([{"type": "text", "text": "no recent works"}])
        );

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
            scopes: vec!["sources".into(), "history.read".into()],
        }
    );
    let status = server.status();
    assert!(status.running);
    assert_eq!(status.sessions, 1);
    assert_eq!(status.last_key_name.as_deref(), Some("Test agent"));
    assert!(status.keys[0].last_used_at.is_some());

    // A tool of the app's own scope, destructive: a malformed scope is
    // refused and the tools kept; a sound one is listed only to a key
    // holding exactly it.
    app.more
        .lock()
        .unwrap()
        .push(history_remove("history remove"));
    assert!(matches!(
        server.host_tools_changed(),
        Err(McpError::Setup { message }) if message.contains("its scope \"history remove\"")
    ));
    *app.more.lock().unwrap() = vec![history_remove("history.remove")];
    server.host_tools_changed().unwrap();
    let listed = |session: &mut Option<String>| {
        let list = json!({"jsonrpc": "2.0", "id": 7, "method": "tools/list"});
        let answer = rt
            .block_on(rpc(&http, port, TOKEN, session, list))
            .1
            .unwrap();
        answer["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "history_remove")
            .map(|t| t["annotations"].clone())
    };
    assert_eq!(listed(&mut session), None);
    server.set_keys(vec![key(&["history.read", "history.write"])]);
    assert_eq!(listed(&mut session), None);
    server.set_keys(vec![key(&["history.remove"])]);
    let hints = listed(&mut session).expect("listed with its scope");
    assert_eq!(hints["destructiveHint"], true, "{hints}");
    assert_eq!(hints["readOnlyHint"], false, "{hints}");
    // Revoked from the app's thread: refused at once.
    server.set_keys(vec![]);
    rt.block_on(async {
        let list = json!({"jsonrpc": "2.0", "id": 6, "method": "tools/list"});
        let (status, _) = rpc(&http, port, TOKEN, &mut session, list).await;
        assert_eq!(status, 401);
    });
    // A key holding a malformed scope is left out: its token opens nothing.
    server.set_keys(vec![key(&["history.remove", "history/read"])]);
    rt.block_on(async {
        let init = json!({"jsonrpc": "2.0", "id": 8, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "test", "version": "1"}}});
        let (status, _) = rpc(&http, port, TOKEN, &mut None, init).await;
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
    // The same as data, for the app to word.
    let fields: Vec<_> = activity
        .iter()
        .map(|a| {
            (
                a.error,
                a.error_message.as_deref(),
                a.query.as_deref(),
                a.count,
            )
        })
        .collect();
    assert_eq!(
        fields,
        [
            (None, None, None, None),
            (
                Some(McpErrorKind::InvalidArguments),
                Some("limit must be at least 1"),
                None,
                None
            ),
            (None, None, Some("title:sepsis"), None),
        ]
    );
    drop(server);
    drop(aspero);
    let _ = std::fs::remove_dir_all(dir);
}

/// The app's library, apart from its `McpHost`, which is as it was before
/// holdings were asked about.
struct Shelf;

#[async_trait::async_trait]
impl McpHoldings for Shelf {
    async fn held(&self, works: Vec<Vec<String>>) -> Vec<bool> {
        works
            .iter()
            .map(|ids| ids.iter().any(|id| id == "doi:10.1038/nature14539"))
            .collect()
    }
}

#[test]
fn activity_says_when_an_analysis_only_estimated() {
    let aspero = Aspero::new(Config::default()).unwrap();
    let app = Arc::new(App::default());
    let port = free_port();
    let rt = runtime();
    let server = rt
        .block_on(McpServer::start(
            aspero.clone(),
            config(port, &["sources"]),
            Some(app.clone() as Arc<dyn McpHost>),
            Some(Arc::new(Shelf) as Arc<dyn McpHoldings>),
        ))
        .unwrap();
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let mut session = None;
    rt.block_on(async {
        let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "test", "version": "1"}}});
        let (_, answer) = rpc(&http, port, TOKEN, &mut session, init).await;
        let said = answer.unwrap()["result"]["instructions"].clone();
        assert!(said.as_str().unwrap().contains("`held: true`"), "{said}");
        // Refused before anything is looked up (no network), and still
        // an estimate, as asked.
        for (id, estimate) in [(2, true), (3, false)] {
            let call = json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
                "params": {"name": "analyse", "arguments": {"ids": [], "estimate_only": estimate}}});
            let (_, answer) = rpc(&http, port, TOKEN, &mut session, call).await;
            assert_eq!(answer.unwrap()["result"]["isError"], true);
        }
    });
    rt.block_on(server.stop());
    for _ in 0..100 {
        if app.activity.lock().unwrap().len() >= 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let activity = app.activity.lock().unwrap().clone();
    let said: Vec<(&str, bool)> = activity
        .iter()
        .map(|a| (a.tool.as_str(), a.estimate_only))
        .collect();
    assert_eq!(said, [("analyse", true), ("analyse", false)]);

    // Holdings without a host of tools: the engine's tools alone.
    let port = free_port();
    let server = rt
        .block_on(McpServer::start(
            aspero.clone(),
            config(port, &["sources"]),
            None,
            Some(Arc::new(Shelf) as Arc<dyn McpHoldings>),
        ))
        .unwrap();
    rt.block_on(async {
        let mut session = None;
        let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "test", "version": "1"}}});
        rpc(&http, port, TOKEN, &mut session, init).await;
        let list = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
        let (_, answer) = rpc(&http, port, TOKEN, &mut session, list).await;
        assert_eq!(
            answer.unwrap()["result"]["tools"].as_array().unwrap().len(),
            8
        );
    });
    rt.block_on(server.stop());
}

/// An app drawing a page: a PNG and a JPEG, or, asked for `mislabelled`,
/// a JPEG it calls a PNG.
struct Drawer;

#[async_trait::async_trait]
impl McpHost for Drawer {
    fn tools(&self) -> Vec<HostTool> {
        vec![HostTool {
            name: "page_image".into(),
            title: "A page, drawn".into(),
            description: "A document's page as it's drawn.".into(),
            input_schema: json!({"type": "object"}).to_string(),
            output_schema: None,
            scope: "documents.read".into(),
            read_only: true,
            destructive: false,
            idempotent: true,
        }]
    }

    async fn call(
        &self,
        _: String,
        arguments: String,
        _: McpCaller,
    ) -> Result<HostToolResult, HostToolError> {
        let args: Value = serde_json::from_str(&arguments).unwrap();
        let png = HostToolImage {
            data: b"\x89PNG\r\n\x1a\nab".to_vec(),
            kind: HostImageType::Png,
        };
        let jpeg = HostToolImage {
            data: vec![0xFF, 0xD8, 0xFF, 0xE0],
            kind: if args["mislabelled"] == true {
                HostImageType::Png
            } else {
                HostImageType::Jpeg
            },
        };
        Ok(HostToolResult {
            structured: json!({"page": 3, "width": 1568, "height": 2030}).to_string(),
            summary: "page 3 of 12".into(),
            images: vec![png, jpeg],
        })
    }

    fn activity(&self, _: McpActivity) {}
}

/// An app's images reach the agent as MCP `image` content after its
/// text, base64, in order; one whose bytes aren't its type's fails the
/// call.
#[test]
fn an_apps_images_reach_the_agent_after_its_text() {
    let dir = std::env::temp_dir().join(format!("aspero-ffi-images-{}", std::process::id()));
    let aspero = Aspero::new(Config {
        cache_dir: Some(dir.to_string_lossy().into()),
        ..Config::default()
    })
    .unwrap();
    let port = free_port();
    let rt = runtime();
    let server = rt
        .block_on(McpServer::start(
            aspero,
            config(port, &["documents.read"]),
            Some(Arc::new(Drawer) as Arc<dyn McpHost>),
            None,
        ))
        .unwrap();
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let mut session = None;
    rt.block_on(async {
        let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "test", "version": "1"}}});
        rpc(&http, port, TOKEN, &mut session, init).await;
        let call = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": "page_image", "arguments": {}}});
        let (_, answer) = rpc(&http, port, TOKEN, &mut session, call).await;
        let result = &answer.unwrap()["result"];
        assert_eq!(result["isError"], false, "{result}");
        assert_eq!(
            result["content"],
            json!([
                {"type": "text", "text": "page 3 of 12"},
                {"type": "image", "data": "iVBORw0KGgphYg==", "mimeType": "image/png"},
                {"type": "image", "data": "/9j/4A==", "mimeType": "image/jpeg"},
            ])
        );
        assert_eq!(result["structuredContent"]["page"], 3);

        let call = json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"name": "page_image", "arguments": {"mislabelled": true}}});
        let (_, answer) = rpc(&http, port, TOKEN, &mut session, call).await;
        let result = &answer.unwrap()["result"];
        assert_eq!(result["isError"], true);
        assert_eq!(
            result["content"][0]["text"],
            "Error (failed): the app's page_image answered with image 2 labelled PNG, but its \
             bytes don't start with PNG's signature"
        );
    });
    rt.block_on(server.stop());
}
