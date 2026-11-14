//! The Streamable HTTP server end to end, on loopback: a scripted engine,
//! a fake host, and JSON-RPC over HTTP as an agent sends it.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use aspero_mcp::Scope::{ReadHistory, Sources, WriteHistory};
use aspero_mcp::http::{
    Activity, Host, HostError, HostOutput, HostTool, HttpConfig, HttpServer, Key, Ledger,
    MemoryLedger, StartError, sha256_hex,
};
use aspero_mcp::{Caller, Scope, Tools};
use async_trait::async_trait;
use common::{SEARCH_COST, source, tools};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const FULL: &str = "token-full";
const READER: &str = "token-reader";

/// A free port: bound, read and let go, as an app's settings would pick.
fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn key(id: &str, token: &str, scopes: &[Scope], daily_budget_usd: f64) -> Key {
    Key {
        id: id.into(),
        name: format!("{id} agent"),
        scopes: scopes.to_vec(),
        sha256: sha256_hex(token),
        daily_budget_usd,
    }
}

fn keys() -> Vec<Key> {
    vec![
        key("full", FULL, &[Sources, ReadHistory, WriteHistory], 100.0),
        key("reader", READER, &[ReadHistory], 100.0),
    ]
}

fn tool(name: &str, scope: Scope, required: &[&str]) -> HostTool {
    HostTool {
        name: name.into(),
        title: format!("The {name} tool"),
        description: format!("Does {name}."),
        input_schema: json!({
            "type": "object",
            "properties": {"q": {"type": "string"}, "doi": {"type": "string"}},
            "required": required,
        })
        .to_string(),
        output_schema: Some(json!({"type": "object"}).to_string()),
        scope,
    }
}

/// The app, faked: a few tools, every call and activity kept.
#[derive(Default)]
struct FakeHost {
    tools: Mutex<Vec<HostTool>>,
    calls: Mutex<Vec<(String, Value, Caller)>>,
    activity: Mutex<Vec<Activity>>,
}

impl FakeHost {
    fn new() -> Arc<Self> {
        let host = FakeHost::default();
        *host.tools.lock().unwrap() = vec![
            tool("history_search", ReadHistory, &["q"]),
            tool("history_add", WriteHistory, &["doi"]),
            tool("history_fail", ReadHistory, &[]),
            tool("history_slow", ReadHistory, &[]),
            tool("history_bad", ReadHistory, &[]),
        ];
        Arc::new(host)
    }

    fn calls(&self) -> Vec<(String, Value, Caller)> {
        self.calls.lock().unwrap().clone()
    }

    /// The activity so far, once there's at least `n` (it comes from a
    /// thread of its own).
    async fn activity(&self, n: usize) -> Vec<Activity> {
        for _ in 0..200 {
            let got = self.activity.lock().unwrap().clone();
            if got.len() >= n {
                return got;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!(
            "expected {n} activities: {:?}",
            self.activity.lock().unwrap()
        );
    }
}

#[async_trait]
impl Host for FakeHost {
    fn tools(&self) -> Vec<HostTool> {
        self.tools.lock().unwrap().clone()
    }

    async fn call(
        &self,
        name: String,
        arguments: String,
        caller: Caller,
    ) -> Result<HostOutput, HostError> {
        let args: Value = serde_json::from_str(&arguments).unwrap();
        self.calls
            .lock()
            .unwrap()
            .push((name.clone(), args.clone(), caller));
        match name.as_str() {
            "history_fail" => Err(HostError::Refused("not today".into())),
            "history_slow" => {
                tokio::time::sleep(Duration::from_secs(10)).await;
                unreachable!("timed out first")
            }
            "history_bad" => Ok(HostOutput {
                structured: "[1, 2]".into(),
                summary: "a list".into(),
            }),
            _ => Ok(HostOutput {
                structured: json!({"found": 2, "q": args["q"]}).to_string(),
                summary: "found 2 works in history\nfirst: Deep learning".into(),
            }),
        }
    }

    fn activity(&self, activity: Activity) {
        self.activity.lock().unwrap().push(activity);
    }
}

fn config(port: u16) -> HttpConfig {
    let mut config = HttpConfig::default();
    config.port = port;
    config.keys = keys();
    config.session_budget_usd = 10.0;
    config.host_timeout = Duration::from_millis(300);
    config
}

async fn start_with(
    config: HttpConfig,
    host: Option<Arc<FakeHost>>,
    ledger: Arc<dyn Ledger>,
) -> Result<HttpServer, StartError> {
    let tools: Arc<Tools> = Arc::new(tools(vec![source()]));
    HttpServer::start(tools, config, host.map(|h| h as Arc<dyn Host>), ledger).await
}

async fn start(host: &Arc<FakeHost>) -> HttpServer {
    start_with(
        config(free_port()),
        Some(host.clone()),
        Arc::new(MemoryLedger::default()),
    )
    .await
    .unwrap()
}

/// An agent: JSON-RPC over HTTP POSTs, answers read from the SSE stream.
struct Agent {
    http: reqwest::Client,
    url: String,
    token: Option<String>,
    session: Option<String>,
    id: u64,
}

impl Agent {
    fn new(port: u16, token: &str) -> Self {
        Agent {
            http: reqwest::Client::builder().no_proxy().build().unwrap(),
            url: format!("http://127.0.0.1:{port}/mcp"),
            token: Some(token.into()),
            session: None,
            id: 0,
        }
    }

    async fn post(&self, body: &Value) -> reqwest::Response {
        let mut request = self
            .http
            .post(&self.url)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .body(body.to_string());
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        if let Some(session) = &self.session {
            request = request
                .header("mcp-session-id", session)
                .header("mcp-protocol-version", "2025-06-18");
        }
        request.send().await.unwrap()
    }

    /// A request's HTTP status, and its JSON-RPC answer if it had one.
    async fn request(&mut self, method: &str, params: Value) -> (u16, Option<Value>) {
        self.id += 1;
        let body = json!({"jsonrpc": "2.0", "id": self.id, "method": method, "params": params});
        let response = self.post(&body).await;
        let status = response.status().as_u16();
        if let Some(session) = response.headers().get("mcp-session-id") {
            self.session = Some(session.to_str().unwrap().into());
        }
        let text = response.text().await.unwrap();
        let answer = text
            .lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .filter_map(|d| serde_json::from_str::<Value>(d.trim()).ok())
            .find(|v| v.get("id").is_some())
            .or_else(|| serde_json::from_str(&text).ok());
        (status, answer)
    }

    async fn rpc(&mut self, method: &str, params: Value) -> Value {
        match self.request(method, params).await {
            (200, Some(answer)) => answer,
            other => panic!("{method}: {other:?}"),
        }
    }

    async fn initialize(&mut self) -> Value {
        let answer = self
            .rpc(
                "initialize",
                json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "test-agent", "version": "1.0"},
                }),
            )
            .await;
        assert!(self.session.is_some(), "a session: {answer}");
        let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        assert_eq!(self.post(&note).await.status().as_u16(), 202);
        answer["result"].clone()
    }

    async fn tools(&mut self) -> Vec<String> {
        let answer = self.rpc("tools/list", json!({})).await;
        answer["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_owned())
            .collect()
    }

    /// A tool call's result (`content`, `structuredContent`, `isError`).
    async fn call(&mut self, name: &str, arguments: Value) -> Value {
        let answer = self
            .rpc("tools/call", json!({"name": name, "arguments": arguments}))
            .await;
        answer
            .get("result")
            .cloned()
            .unwrap_or_else(|| panic!("{name}: {answer}"))
    }

    /// The session's GET stream, where the server says things unasked.
    async fn listen(&self) -> Stream {
        let mut request = self
            .http
            .get(&self.url)
            .header("accept", "text/event-stream")
            .header("mcp-session-id", self.session.as_deref().unwrap())
            .header("mcp-protocol-version", "2025-06-18");
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        let response = request.send().await.unwrap();
        assert_eq!(response.status().as_u16(), 200);
        Stream {
            response,
            buffer: String::new(),
        }
    }
}

/// A session's SSE stream, read as it comes.
struct Stream {
    response: reqwest::Response,
    buffer: String,
}

const LIST_CHANGED: &str = "notifications/tools/list_changed";

impl Stream {
    /// The method of the next message within `wait`, skipping priming
    /// events and pings; `None` if none comes.
    async fn next(&mut self, wait: Duration) -> Option<String> {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            while let Some(end) = self.buffer.find("\n\n") {
                let event: String = self.buffer.drain(..end + 2).collect();
                let method = event
                    .lines()
                    .filter_map(|l| l.strip_prefix("data:"))
                    .filter_map(|d| serde_json::from_str::<Value>(d.trim()).ok())
                    .find_map(|v| v["method"].as_str().map(String::from));
                if method.is_some() {
                    return method;
                }
            }
            match tokio::time::timeout_at(deadline, self.response.chunk()).await {
                Ok(Ok(Some(bytes))) => self
                    .buffer
                    .push_str(&String::from_utf8_lossy(&bytes).replace("\r\n", "\n")),
                _ => return None,
            }
        }
    }

    /// Whether `notifications/tools/list_changed` comes, soon.
    async fn told(&mut self) -> bool {
        self.next(Duration::from_secs(5)).await.as_deref() == Some(LIST_CHANGED)
    }

    /// Whether nothing comes for a while.
    async fn quiet(&mut self) -> bool {
        self.next(Duration::from_millis(300)).await.is_none()
    }
}

fn text(result: &Value) -> &str {
    result["content"][0]["text"].as_str().unwrap()
}

fn is_error(result: &Value) -> bool {
    result["isError"] == true
}

const ENGINE: [&str; 7] = [
    "search",
    "search_supports",
    "lookup",
    "verify",
    "identify",
    "citing_works",
    "references",
];

#[tokio::test(flavor = "multi_thread")]
async fn a_key_lists_only_the_tools_its_scopes_allow() {
    let host = FakeHost::new();
    let server = start(&host).await;

    let mut full = Agent::new(server.port(), FULL);
    let info = full.initialize().await;
    assert_eq!(info["serverInfo"]["name"], "aspero");
    let listed = full.tools().await;
    let mut expected: Vec<&str> = ENGINE.to_vec();
    expected.extend([
        "history_search",
        "history_add",
        "history_fail",
        "history_slow",
        "history_bad",
    ]);
    assert_eq!(listed, expected);

    let mut reader = Agent::new(server.port(), READER);
    reader.initialize().await;
    assert_eq!(
        reader.tools().await,
        [
            "history_search",
            "history_fail",
            "history_slow",
            "history_bad"
        ]
    );
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn engine_and_host_tools_answer_and_the_host_hears_of_each() {
    let host = FakeHost::new();
    let server = start(&host).await;
    let mut agent = Agent::new(server.port(), FULL);
    agent.initialize().await;

    let result = agent.call("search", json!({"text": "Deep learning"})).await;
    assert!(!is_error(&result), "{result}");
    assert_eq!(
        result["structuredContent"]["hits"][0]["work"]["title"],
        "Deep learning"
    );
    assert_eq!(
        result["structuredContent"]["session"]["spent_usd"],
        SEARCH_COST
    );

    let result = agent.call("history_search", json!({"q": "sepsis"})).await;
    assert!(!is_error(&result), "{result}");
    assert_eq!(
        result["structuredContent"],
        json!({"found": 2, "q": "sepsis"})
    );
    assert!(text(&result).starts_with("found 2 works in history"));
    let calls = host.calls();
    let (name, args, caller) = &calls[0];
    assert_eq!(name, "history_search");
    assert_eq!(args, &json!({"q": "sepsis"}));
    assert_eq!(caller.key_id.as_deref(), Some("full"));
    assert_eq!(caller.key_name.as_deref(), Some("full agent"));
    assert_eq!(caller.scopes, [Sources, ReadHistory, WriteHistory]);
    assert_eq!(caller.client.as_deref(), Some("test-agent"));

    let result = agent.call("history_fail", json!({})).await;
    assert!(is_error(&result));
    assert_eq!(text(&result), "Error (refused): not today");

    let result = agent.call("history_bad", json!({})).await;
    assert!(is_error(&result));
    assert!(
        text(&result).contains("other than a JSON object"),
        "{result}"
    );

    let activity = host.activity(4).await;
    let said: Vec<(&str, &str, bool, f64)> = activity
        .iter()
        .map(|a| (a.tool.as_str(), a.summary.as_str(), a.is_error, a.cost_usd))
        .collect();
    assert_eq!(
        said,
        [
            ("search", "searched ‘Deep learning’", false, SEARCH_COST),
            ("history_search", "found 2 works in history", false, 0.0),
            ("history_fail", "error: not today", true, 0.0),
            (
                "history_bad",
                "error: the app's history_bad answered with something other than a JSON object",
                true,
                0.0
            ),
        ]
    );
    assert!(activity.iter().all(|a| a.key_name == "full agent"));
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_key_without_the_scope_is_refused_before_the_host_is_asked() {
    let host = FakeHost::new();
    let server = start(&host).await;
    let mut reader = Agent::new(server.port(), READER);
    reader.initialize().await;

    let result = reader.call("history_add", json!({"doi": "10.1/x"})).await;
    assert!(is_error(&result));
    assert_eq!(
        text(&result),
        "Error (forbidden): this key can't use history_add: it lacks the write_history scope"
    );
    let result = reader
        .call("search", json!({"text": "Deep learning"}))
        .await;
    assert!(is_error(&result));
    assert!(
        text(&result).starts_with(
            "Error (forbidden): this key can't use search: it lacks the sources scope"
        ),
        "{result}"
    );
    assert!(host.calls().is_empty());
    let activity = host.activity(2).await;
    assert!(activity.iter().all(|a| a.is_error));
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn required_fields_are_checked_and_a_slow_host_times_out() {
    let host = FakeHost::new();
    let server = start(&host).await;
    let mut agent = Agent::new(server.port(), FULL);
    agent.initialize().await;

    let result = agent.call("history_search", json!({"x": 1})).await;
    assert!(is_error(&result));
    assert_eq!(
        text(&result),
        "Error (invalid_arguments): invalid arguments for history_search: missing `q`"
    );
    assert!(host.calls().is_empty());

    let started = std::time::Instant::now();
    let result = agent.call("history_slow", json!({})).await;
    assert!(is_error(&result));
    assert_eq!(
        text(&result),
        "Error (timeout): the app didn't answer history_slow within 0.3 s"
    );
    assert!(started.elapsed() < Duration::from_secs(5));
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn host_tools_may_not_share_a_name() {
    let host = FakeHost::new();
    host.tools
        .lock()
        .unwrap()
        .push(tool("lookup", ReadHistory, &[]));
    let clash = start_with(
        config(free_port()),
        Some(host.clone()),
        Arc::new(MemoryLedger::default()),
    )
    .await;
    assert_eq!(
        clash.err(),
        Some(StartError::ToolNameClash {
            name: "lookup".into()
        })
    );

    let host = FakeHost::new();
    let server = start(&host).await;
    // Twice: refused, and the tools stay as they were.
    host.tools
        .lock()
        .unwrap()
        .push(tool("history_add", ReadHistory, &[]));
    assert_eq!(
        server.host_tools_changed(),
        Err(StartError::ToolNameClash {
            name: "history_add".into()
        })
    );
    let mut agent = Agent::new(server.port(), READER);
    agent.initialize().await;
    assert_eq!(agent.tools().await.len(), 4);
    // A new tool, listed once the host says so.
    {
        let mut tools = host.tools.lock().unwrap();
        tools.pop();
        tools.push(tool("history_count", ReadHistory, &[]));
    }
    server.host_tools_changed().unwrap();
    assert!(agent.tools().await.contains(&"history_count".to_owned()));
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn without_engine_tools_only_the_hosts_are_served() {
    let host = FakeHost::new();
    let mut config = config(free_port());
    config.engine_tools = false;
    let server = start_with(
        config,
        Some(host.clone()),
        Arc::new(MemoryLedger::default()),
    )
    .await
    .unwrap();
    let mut agent = Agent::new(server.port(), FULL);
    agent.initialize().await;
    assert_eq!(agent.tools().await.len(), 5);
    let (status, answer) = agent
        .request(
            "tools/call",
            json!({"name": "search", "arguments": {"text": "x"}}),
        )
        .await;
    assert_eq!(status, 200);
    assert!(
        answer.unwrap()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("no tool named")
    );
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn requests_need_a_key_and_a_revoked_key_is_refused_at_once() {
    let host = FakeHost::new();
    let server = start(&host).await;
    let port = server.port();

    let mut nobody = Agent::new(port, FULL);
    nobody.token = None;
    let (status, _) = nobody.request("initialize", json!({})).await;
    assert_eq!(status, 401);
    let mut guesser = Agent::new(port, "token-wrong");
    let (status, answer) = guesser.request("initialize", json!({})).await;
    assert_eq!((status, answer), (401, None));

    let mut agent = Agent::new(port, FULL);
    agent.initialize().await;
    assert_eq!(server.status().sessions, 1);
    // Another key can't use this key's session.
    let mut other = Agent::new(port, READER);
    other.session = agent.session.clone();
    assert_eq!(other.request("tools/list", json!({})).await.0, 404);

    // Revoked: the next request is refused, and the session ends.
    server.set_keys(vec![key("reader", READER, &[ReadHistory], 1.0)]);
    let (status, _) = agent.request("tools/list", json!({})).await;
    assert_eq!(status, 401);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(server.status().sessions, 0);

    // Rotated: the old token no longer works, the new one does.
    server.set_keys(vec![key("full", "token-new", &[Sources], 1.0)]);
    let mut old = Agent::new(port, FULL);
    assert_eq!(old.request("initialize", json!({})).await.0, 401);
    let mut new = Agent::new(port, "token-new");
    new.initialize().await;
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn browsers_and_other_hosts_are_refused() {
    let host = FakeHost::new();
    let server = start(&host).await;
    let port = server.port();
    let agent = Agent::new(port, FULL);
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}});
    let response = agent
        .http
        .post(&agent.url)
        .bearer_auth(FULL)
        .header("origin", "https://evil.example")
        .header("content-type", "application/json")
        .body(init.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 403);

    // A name that resolves here (DNS rebinding) is still the wrong host.
    let raw = |host: String| async move {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        let request = format!(
            "POST /mcp HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {FULL}\r\n\
             Content-Length: 0\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut answer = String::new();
        stream.read_to_string(&mut answer).await.unwrap();
        answer.lines().next().unwrap_or("").to_owned()
    };
    assert_eq!(
        raw(format!("evil.example:{port}")).await,
        "HTTP/1.1 403 Forbidden"
    );
    assert_eq!(raw("127.0.0.1:1".into()).await, "HTTP/1.1 403 Forbidden");
    // The right host gets as far as the protocol (no body: a 4xx of its own).
    assert_ne!(
        raw(format!("localhost:{port}")).await,
        "HTTP/1.1 403 Forbidden"
    );
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_port_in_use_fails_and_stop_frees_it() {
    let host = FakeHost::new();
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    let refused = start_with(
        config(port),
        Some(host.clone()),
        Arc::new(MemoryLedger::default()),
    )
    .await;
    assert_eq!(refused.err(), Some(StartError::PortInUse { port }));
    drop(taken);

    let server = start_with(
        config(port),
        Some(host.clone()),
        Arc::new(MemoryLedger::default()),
    )
    .await
    .unwrap();
    assert_eq!(server.port(), port);
    let mut agent = Agent::new(port, FULL);
    agent.initialize().await;
    let status = server.status();
    assert!(status.running);
    assert_eq!(status.sessions, 1);
    assert_eq!(status.last_key_name.as_deref(), Some("full agent"));
    assert!(status.last_request_at.is_some());
    assert_eq!(status.keys.len(), 2);
    assert!(status.keys[0].last_used_at.is_some());
    assert!(status.keys[1].last_used_at.is_none());

    server.stop().await;
    assert!(!server.status().running);
    std::net::TcpListener::bind(("127.0.0.1", port)).expect("the port is free");
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_spends_its_budget_and_no_more() {
    let host = FakeHost::new();
    let mut config = config(free_port());
    config.session_budget_usd = 2.0 * SEARCH_COST;
    let server = start_with(
        config,
        Some(host.clone()),
        Arc::new(MemoryLedger::default()),
    )
    .await
    .unwrap();
    let mut agent = Agent::new(server.port(), FULL);
    agent.initialize().await;
    let search = json!({"text": "Deep learning"});
    for _ in 0..2 {
        assert!(!is_error(&agent.call("search", search.clone()).await));
    }
    let result = agent.call("search", search.clone()).await;
    assert!(is_error(&result));
    assert!(
        text(&result).starts_with("Error (budget_spent): this session's budget is spent"),
        "{result}"
    );
    assert!(text(&result).contains("the session budget in the app's settings"));
    let supports = json!({"query": {"title": "x"}});
    assert!(!is_error(&agent.call("search_supports", supports).await));

    // A new session starts afresh.
    let mut again = Agent::new(server.port(), FULL);
    again.initialize().await;
    assert!(!is_error(&again.call("search", search).await));
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_keys_daily_budget_outlasts_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.sqlite");
    let ledger =
        || -> Arc<dyn Ledger> { Arc::new(aspero_engine::SqliteCache::open(&path).unwrap()) };
    let mut config = config(free_port());
    config.keys = vec![key("full", FULL, &[Sources], 2.0 * SEARCH_COST)];
    let search = json!({"text": "Deep learning"});

    let server = start_with(config.clone(), None, ledger()).await.unwrap();
    let mut agent = Agent::new(server.port(), FULL);
    agent.initialize().await;
    for _ in 0..2 {
        assert!(!is_error(&agent.call("search", search.clone()).await));
    }
    server.stop().await;

    let server = start_with(config, None, ledger()).await.unwrap();
    let mut agent = Agent::new(server.port(), FULL);
    agent.initialize().await;
    let result = agent.call("search", search).await;
    assert!(is_error(&result));
    assert!(
        text(&result).starts_with(
            "Error (budget_spent): this key's daily budget is spent ($0.5000 of $0.50 today, UTC)"
        ),
        "{result}"
    );
    // Free tools still work.
    let supports = json!({"query": {"title": "x"}});
    assert!(!is_error(&agent.call("search_supports", supports).await));
    server.stop().await;
}

const READ_TOOLS: [&str; 4] = [
    "history_search",
    "history_fail",
    "history_slow",
    "history_bad",
];

#[tokio::test(flavor = "multi_thread")]
async fn a_scope_change_tells_the_keys_sessions_and_relists() {
    let host = FakeHost::new();
    let server = start(&host).await;
    let port = server.port();

    let mut reader = Agent::new(port, READER);
    let info = reader.initialize().await;
    assert_eq!(info["capabilities"]["tools"]["listChanged"], true, "{info}");
    assert_eq!(reader.tools().await, READ_TOOLS);
    let mut heard = reader.listen().await;
    let mut full = Agent::new(port, FULL);
    full.initialize().await;
    let mut full_heard = full.listen().await;

    // The same keys again: nothing to say.
    server.set_keys(keys());
    assert!(heard.quiet().await);

    // Writing allowed: the reader's session hears, and lists the write tool.
    let with_reader = |scopes: &[Scope]| {
        vec![
            key("full", FULL, &[Sources, ReadHistory, WriteHistory], 100.0),
            key("reader", READER, scopes, 100.0),
        ]
    };
    server.set_keys(with_reader(&[ReadHistory, WriteHistory]));
    assert!(heard.told().await);
    assert!(heard.quiet().await, "told once");
    assert!(
        full_heard.quiet().await,
        "another key's session hears nothing"
    );
    let listed = reader.tools().await;
    assert!(listed.contains(&"history_add".to_owned()), "{listed:?}");
    let result = reader.call("history_add", json!({"doi": "10.1/x"})).await;
    assert!(!is_error(&result), "{result}");

    // The same scopes in another order: no change.
    server.set_keys(with_reader(&[WriteHistory, ReadHistory]));
    assert!(heard.quiet().await);

    // And taken away again.
    server.set_keys(with_reader(&[ReadHistory]));
    assert!(heard.told().await);
    assert_eq!(reader.tools().await, READ_TOOLS);
    let result = reader.call("history_add", json!({"doi": "10.1/x"})).await;
    assert!(is_error(&result), "{result}");

    // A key removed still ends its sessions.
    server.set_keys(vec![key(
        "full",
        FULL,
        &[Sources, ReadHistory, WriteHistory],
        100.0,
    )]);
    assert_eq!(reader.request("tools/list", json!({})).await.0, 401);
    assert_eq!(full.tools().await.len(), ENGINE.len() + 5);
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_change_waits_for_the_client_to_listen() {
    let host = FakeHost::new();
    let server = start(&host).await;
    let mut reader = Agent::new(server.port(), READER);
    reader.initialize().await;

    server.set_keys(vec![key(
        "reader",
        READER,
        &[ReadHistory, WriteHistory],
        1.0,
    )]);
    // No stream open yet: kept, and said once there is one.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut heard = reader.listen().await;
    assert!(heard.told().await);
    assert!(reader.tools().await.contains(&"history_add".to_owned()));
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn new_host_tools_are_told_to_every_session() {
    let host = FakeHost::new();
    let server = start(&host).await;
    let mut reader = Agent::new(server.port(), READER);
    reader.initialize().await;
    let mut reader_heard = reader.listen().await;
    let mut full = Agent::new(server.port(), FULL);
    full.initialize().await;
    let mut full_heard = full.listen().await;

    host.tools
        .lock()
        .unwrap()
        .push(tool("history_count", ReadHistory, &[]));
    server.host_tools_changed().unwrap();
    assert!(reader_heard.told().await);
    assert!(full_heard.told().await);
    assert!(reader.tools().await.contains(&"history_count".to_owned()));
    assert!(full.tools().await.contains(&"history_count".to_owned()));

    // A clash keeps the tools, and tells no one.
    host.tools
        .lock()
        .unwrap()
        .push(tool("lookup", ReadHistory, &[]));
    assert!(server.host_tools_changed().is_err());
    assert!(reader_heard.quiet().await);
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn clients_that_never_listen_hold_up_no_one() {
    let host = FakeHost::new();
    let server = start(&host).await;
    let mut agents = Vec::new();
    for _ in 0..3 {
        let mut agent = Agent::new(server.port(), READER);
        agent.initialize().await;
        agents.push(agent);
    }
    // Many changes, no streams open: neither call waits on a client.
    let started = std::time::Instant::now();
    for i in 0..200 {
        let scopes: &[Scope] = if i % 2 == 0 {
            &[ReadHistory, WriteHistory]
        } else {
            &[ReadHistory]
        };
        server.set_keys(vec![key("reader", READER, scopes, 1.0)]);
        server.host_tools_changed().unwrap();
    }
    assert!(started.elapsed() < Duration::from_secs(2));
    // The sessions still answer, by the last scopes.
    for agent in &mut agents {
        assert_eq!(agent.tools().await, READ_TOOLS);
    }
    server.stop().await;
}
