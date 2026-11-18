//! The Streamable HTTP server end to end, on loopback: a scripted engine,
//! a fake host, and JSON-RPC over HTTP as an agent sends it.

// Exact constants: scores and spends rounded or summed from binary-exact values.
#![allow(clippy::float_cmp)]

mod common;

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use aspero_core::testing::Events;
use aspero_mcp::http::{
    ACTIVITY_QUEUE, Activity, Host, HostError, HostOutput, HostTool, HttpConfig, HttpServer, Key,
    Ledger, MemoryLedger, StartError, sha256_hex,
};
use aspero_mcp::{Caller, ToolErrorKind, Tools};
use async_trait::async_trait;
use common::{SEARCH_COST, source, tools};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const FULL: &str = "token-full";
const READER: &str = "token-reader";

// Scopes: aspero's own, and the ones this host declares.
const SOURCES: &str = "sources";
const READ: &str = "history.read";
const WRITE: &str = "history.write";
const REMOVE: &str = "history.remove";

/// A free port: bound, read and let go, as an app's settings would pick.
fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn key(id: &str, token: &str, scopes: &[&str], daily_budget_usd: f64) -> Key {
    Key {
        id: id.into(),
        name: format!("{id} agent"),
        scopes: scopes.iter().map(|s| (*s).to_owned()).collect(),
        sha256: sha256_hex(token),
        daily_budget_usd,
    }
}

fn keys() -> Vec<Key> {
    vec![
        key("full", FULL, &[SOURCES, READ, WRITE], 100.0),
        key("reader", READER, &[READ], 100.0),
    ]
}

/// A host tool under `scope`, with the hints a store's tools would
/// declare: reading changes nothing, removing destroys, writing adds.
fn tool(name: &str, scope: &str, required: &[&str]) -> HostTool {
    let (read_only, destructive, idempotent) = match scope {
        READ => (true, false, true),
        REMOVE => (false, true, false),
        _ => (false, false, false),
    };
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
        scope: scope.into(),
        read_only,
        destructive,
        idempotent,
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
            tool("history_search", READ, &["q"]),
            tool("history_add", WRITE, &["doi"]),
            tool("history_fail", READ, &[]),
            tool("history_slow", READ, &[]),
            tool("history_bad", READ, &[]),
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
    assert_eq!(caller.scopes, [SOURCES, READ, WRITE]);
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
        "Error (forbidden): this key can't use history_add: it lacks the history.write scope"
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
    assert!(
        activity
            .iter()
            .all(|a| a.error == Some(ToolErrorKind::Forbidden) && a.count.is_none())
    );
    assert_eq!(
        activity[1].error_message.as_deref(),
        Some("this key can't use search: it lacks the sources scope")
    );
    // What the agent asked, though refused; the app words its own tools.
    assert_eq!(activity[1].query.as_deref(), Some("Deep learning"));
    assert_eq!(activity[0].query, None);
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn activity_says_what_was_asked_how_many_came_back_and_what_went_wrong() {
    let host = FakeHost::new();
    let server = start(&host).await;
    let mut agent = Agent::new(server.port(), FULL);
    agent.initialize().await;

    let calls = [
        ("search", json!({"text": "Deep learning"})),
        (
            "search",
            json!({"query": {"all": [{"title": "Deep learning"}, {"years": {"from": 2015}}]}}),
        ),
        (
            "verify",
            json!({"citations": [
                "LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015. doi:10.1038/nature14539",
                {"title": "Attention is all you need", "authors": ["Vaswani A"], "year": 2017, "arxiv": "1706.03762"},
            ]}),
        ),
        (
            "lookup",
            json!({"ids": ["10.1038/nature14539", "arXiv:1706.03762", "10.1/none"]}),
        ),
        ("search", json!({"text": "Deep learning", "limit": 0})),
        ("history_search", json!({"q": "sepsis"})),
        ("history_fail", json!({})),
    ];
    for (name, arguments) in &calls {
        agent.call(name, arguments.clone()).await;
    }
    let activity = host.activity(calls.len()).await;
    let fields: Vec<_> = activity
        .iter()
        .map(|a| (a.tool.as_str(), a.query.as_deref(), a.count, a.error))
        .collect();
    assert_eq!(
        fields,
        [
            ("search", Some("Deep learning"), Some(1), None),
            (
                "search",
                Some("title:(Deep learning) AND years:2015–"),
                Some(1),
                None
            ),
            (
                "verify",
                Some(
                    "LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015. \
                     doi:10.1038/nature14539; Vaswani A. Attention is all you need. 2017. \
                     1706.03762"
                ),
                Some(2),
                None
            ),
            (
                "lookup",
                Some("10.1038/nature14539; arXiv:1706.03762; 10.1/none"),
                Some(3),
                None
            ),
            (
                "search",
                Some("Deep learning"),
                None,
                Some(ToolErrorKind::InvalidArguments)
            ),
            ("history_search", None, None, None),
            ("history_fail", None, None, Some(ToolErrorKind::Refused)),
        ]
    );
    // The summaries are as they were; the message is whole, without
    // "error: ".
    let bad = &activity[4];
    assert!(bad.is_error);
    assert_eq!(bad.summary, "error: `limit` is 1 to 50, not 0");
    assert_eq!(
        bad.error_message.as_deref(),
        Some("`limit` is 1 to 50, not 0")
    );
    assert_eq!(activity[0].summary, "searched ‘Deep learning’");
    assert_eq!(activity[2].summary, "verified 2 citations");
    assert_eq!(activity[6].error_message.as_deref(), Some("not today"));
    assert!(
        activity
            .iter()
            .all(|a| a.is_error == a.error.is_some() && a.is_error == a.error_message.is_some())
    );
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
        "Error (timeout): the app didn't answer history_slow within 0.3 s, so whether it \
         did anything is unknown (it may yet): check before calling it again"
    );
    assert!(started.elapsed() < Duration::from_secs(5));
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn host_tools_may_not_share_a_name() {
    let host = FakeHost::new();
    host.tools.lock().unwrap().push(tool("lookup", READ, &[]));
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
        .push(tool("history_add", READ, &[]));
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
        tools.push(tool("history_count", READ, &[]));
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
    server.set_keys(vec![key("reader", READER, &[READ], 1.0)]);
    let (status, _) = agent.request("tools/list", json!({})).await;
    assert_eq!(status, 401);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(server.status().sessions, 0);

    // Rotated: the old token no longer works, the new one does.
    server.set_keys(vec![key("full", "token-new", &[SOURCES], 1.0)]);
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
async fn a_stop_cut_short_finishes_when_called_again() {
    use futures::FutureExt;
    let host = FakeHost::new();
    let server = start(&host).await;
    let port = server.port();
    let mut agent = Agent::new(port, FULL);
    agent.initialize().await;
    // Dropped at its first wait (closing the session), as an app's task
    // can be: no new session opens meanwhile. On a fast machine it may
    // finish in that one poll, which leaves nothing to cut short; the
    // library's unit test cuts one short for certain.
    let _ = server.stop().now_or_never();
    let mut late = Agent::new(port, FULL);
    late.http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18",
        "capabilities": {},
        "clientInfo": {"name": "late", "version": "1.0"},
    }});
    let opened = late
        .http
        .post(&late.url)
        .bearer_auth(FULL)
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json")
        .body(init.to_string())
        .send()
        .await
        .is_ok_and(|r| r.status().is_success());
    assert!(!opened, "a session opened while stopping");
    // Stopping again finishes the job.
    server.stop().await;
    assert!(!server.status().running);
    std::net::TcpListener::bind(("127.0.0.1", port)).expect("the port is free");
    server.stop().await;
}

/// An app whose activity callback is stuck until let go.
#[derive(Default)]
struct StuckHost {
    open: Mutex<bool>,
    opened: std::sync::Condvar,
    heard: std::sync::atomic::AtomicUsize,
}

#[async_trait]
impl Host for StuckHost {
    fn tools(&self) -> Vec<HostTool> {
        vec![tool("history_search", READ, &[])]
    }

    async fn call(&self, _: String, _: String, _: Caller) -> Result<HostOutput, HostError> {
        Ok(HostOutput {
            structured: "{}".into(),
            summary: "nothing".into(),
        })
    }

    fn activity(&self, _: Activity) {
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.opened.wait(open).unwrap();
        }
        self.heard
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn activity_the_app_is_slow_to_take_is_dropped_past_a_queue() {
    let host = Arc::new(StuckHost::default());
    let server = HttpServer::start(
        Arc::new(tools(vec![source()])),
        config(free_port()),
        Some(host.clone() as Arc<dyn Host>),
        Arc::new(MemoryLedger::default()),
    )
    .await
    .unwrap();
    let mut agent = Agent::new(server.port(), FULL);
    agent.initialize().await;
    let calls = ACTIVITY_QUEUE + 50;
    for _ in 0..calls {
        assert!(!is_error(&agent.call("history_search", json!({})).await));
    }
    *host.open.lock().unwrap() = true;
    host.opened.notify_all();
    server.stop().await;
    // Once the app has heard all it's going to: the queue, and the one it
    // was stuck on.
    let heard = || host.heard.load(std::sync::atomic::Ordering::Relaxed);
    let mut before = usize::MAX;
    while heard() != before {
        before = heard();
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let heard = heard();
    assert!(heard <= ACTIVITY_QUEUE + 1, "{heard} of {calls}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_key_has_sixteen_sessions_open_at_most() {
    let host = FakeHost::new();
    let server = start(&host).await;
    let port = server.port();
    let mut open = Vec::new();
    for _ in 0..16 {
        let mut agent = Agent::new(port, FULL);
        agent.initialize().await;
        open.push(agent);
    }
    let more = Agent::new(port, FULL);
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18",
        "capabilities": {},
        "clientInfo": {"name": "more", "version": "1.0"},
    }});
    let refused = more.post(&init).await;
    assert_eq!(refused.status().as_u16(), 429);
    assert!(
        refused
            .text()
            .await
            .unwrap()
            .starts_with("this key has 16 MCP sessions open, the most it may"),
    );
    // Another key's are its own.
    Agent::new(port, READER).initialize().await;
    // One ended, another opens.
    let ended = open.pop().unwrap();
    let deleted = ended
        .http
        .delete(&ended.url)
        .bearer_auth(FULL)
        .header("mcp-session-id", ended.session.as_deref().unwrap())
        .header("mcp-protocol-version", "2025-06-18")
        .send()
        .await
        .unwrap();
    assert!(deleted.status().is_success(), "{deleted:?}");
    Agent::new(port, FULL).initialize().await;
    assert_eq!(server.status().sessions, 17);
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
    config.keys = vec![key("full", FULL, &[SOURCES], 2.0 * SEARCH_COST)];
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

/// Each key's spend today, in the status, read from the ledger: what it
/// spent before a restart, not other days'; two keys' spend apart.
#[tokio::test(flavor = "multi_thread")]
async fn the_status_says_what_each_key_spent_today() {
    let ledger = Arc::new(MemoryLedger::default());
    // Before a restart, and yesterday.
    ledger.add("full", &today(), 0.5).unwrap();
    ledger.add("other", "2000-01-01", 3.0).unwrap();
    let mut config = config(free_port());
    config.keys = vec![
        key("full", FULL, &[SOURCES], 100.0),
        key("other", READER, &[SOURCES], 100.0),
        key("idle", "token-idle", &[SOURCES], 100.0),
    ];
    let server = start_with(config, None, ledger.clone()).await.unwrap();
    let search = json!({"text": "Deep learning"});
    let mut full = Agent::new(server.port(), FULL);
    full.initialize().await;
    for _ in 0..2 {
        assert!(!is_error(&full.call("search", search.clone()).await));
    }
    let mut other = Agent::new(server.port(), READER);
    other.initialize().await;
    assert!(!is_error(&other.call("search", search).await));

    let status = server.status();
    let spent: Vec<(&str, f64)> = status
        .keys
        .iter()
        .map(|k| (k.key_id.as_str(), k.spent_today_usd))
        .collect();
    assert_eq!(
        spent,
        [
            ("full", 0.5 + 2.0 * SEARCH_COST),
            ("other", SEARCH_COST),
            ("idle", 0.0)
        ]
    );
    // The figure the budget is checked against.
    assert_eq!(ledger.spent("other", &today()).unwrap(), SEARCH_COST);
    // The UTC day, ending at the next midnight UTC.
    assert_eq!(status.spend_day, today());
    let ends = status.spend_resets_at.to_zoned(jiff::tz::TimeZone::UTC);
    assert_eq!(ends.date().yesterday().unwrap().to_string(), today());
    assert_eq!((ends.hour(), ends.minute(), ends.second()), (0, 0, 0));
    assert!(status.spend_resets_at > jiff::Timestamp::now());
    server.stop().await;
}

/// UTC today, as the ledger keeps it.
fn today() -> String {
    jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .date()
        .to_string()
}

/// `n` searches at once, each in a session of its own for the key `FULL`
/// (its daily budget `daily_usd`), on a source whose searches take a
/// while: how many succeeded, and what the key spent.
async fn searches_at_once(n: usize, daily_usd: f64) -> (usize, f64) {
    let slow = common::Scripted::new("openalex", vec![common::deep_learning()])
        .slow(Duration::from_millis(100));
    let tools = Arc::new(tools(vec![Arc::new(slow)]));
    let ledger = Arc::new(MemoryLedger::default());
    let mut config = config(free_port());
    config.keys = vec![key("full", FULL, &[SOURCES], daily_usd)];
    let server = HttpServer::start(tools, config, None, ledger.clone())
        .await
        .unwrap();
    let mut agents = Vec::new();
    for _ in 0..n {
        let mut agent = Agent::new(server.port(), FULL);
        agent.initialize().await;
        agents.push(agent);
    }
    let calls = agents
        .iter_mut()
        .map(|a| a.call("search", json!({"text": "Deep learning"})));
    let results = futures::future::join_all(calls).await;
    for r in results.iter().filter(|r| is_error(r)) {
        assert!(
            text(r).starts_with("Error (budget_spent): this key's daily budget is spent"),
            "{r}"
        );
    }
    server.stop().await;
    let done = results.iter().filter(|r| !is_error(r)).count();
    (done, ledger.spent("full", &today()).unwrap())
}

#[tokio::test(flavor = "multi_thread")]
async fn calls_at_once_pass_a_keys_budget_by_one_call_at_most() {
    // A budget under one search's estimate: one runs, alone.
    assert_eq!(searches_at_once(10, 0.0005).await, (1, SEARCH_COST));
    // Searches costing far more than estimated (the test source's): at
    // most four paid calls run at once, so the key passes its budget by
    // four calls' cost at most.
    let (done, spent) = searches_at_once(12, 4.0 * SEARCH_COST).await;
    assert!(done >= 4, "{done}");
    assert!(spent <= 8.0 * SEARCH_COST, "{spent}");
}

/// A ledger that can't write while `failing`.
#[derive(Default)]
struct FlakyLedger {
    kept: MemoryLedger,
    failing: std::sync::atomic::AtomicBool,
}

impl Ledger for FlakyLedger {
    fn spent(&self, key_id: &str, day: &str) -> Result<f64, String> {
        self.kept.spent(key_id, day)
    }

    fn add(&self, key_id: &str, day: &str, usd: f64) -> Result<(), String> {
        if self.failing.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("database is locked".into());
        }
        self.kept.add(key_id, day, usd)
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn spend_the_ledger_cant_record_still_counts_and_is_written_later() {
    let ledger = Arc::new(FlakyLedger::default());
    ledger
        .failing
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let mut config = config(free_port());
    config.keys = vec![key("full", FULL, &[SOURCES], SEARCH_COST)];
    let server = start_with(config, None, ledger.clone()).await.unwrap();
    let mut agent = Agent::new(server.port(), FULL);
    agent.initialize().await;
    let search = json!({"text": "Deep learning"});
    assert!(!is_error(&agent.call("search", search.clone()).await));
    assert_eq!(ledger.spent("full", &today()).unwrap(), 0.0);
    // Not on record, but in the status, as in the budget's check.
    assert_eq!(server.status().keys[0].spent_today_usd, SEARCH_COST);
    // Not on record, but spent: the budget is.
    let result = agent.call("search", search.clone()).await;
    assert!(is_error(&result), "{result}");
    assert!(
        text(&result).starts_with("Error (budget_spent): this key's daily budget is spent"),
        "{result}"
    );
    // Written once the ledger can.
    ledger
        .failing
        .store(false, std::sync::atomic::Ordering::Relaxed);
    assert!(is_error(&agent.call("search", search).await));
    assert_eq!(ledger.spent("full", &today()).unwrap(), SEARCH_COST);
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_call_spends_from_the_keys_budget_too() {
    let ledger = Arc::new(MemoryLedger::default());
    let mut config = config(free_port());
    config.keys = vec![key("full", FULL, &[SOURCES], 100.0)];
    let server = start_with(config, None, ledger.clone()).await.unwrap();
    let mut agent = Agent::new(server.port(), FULL);
    agent.initialize().await;
    // Not identified, after a paid search for the top of the page.
    let page = "A study of nothing in particular\nAnon Ymous\n\nNothing to see here.";
    let result = agent
        .call(
            "identify",
            json!({"text": page, "title_hint": "A study of nothing in particular"}),
        )
        .await;
    assert!(is_error(&result));
    assert!(
        text(&result).starts_with("Error (not_found): not identified"),
        "{result}"
    );
    assert!(text(&result).contains("Session: $0.2500 spent"), "{result}");
    let today = jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .date()
        .to_string();
    assert_eq!(ledger.spent("full", &today).unwrap(), SEARCH_COST);
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
    let with_reader = |scopes: &[&str]| {
        vec![
            key("full", FULL, &[SOURCES, READ, WRITE], 100.0),
            key("reader", READER, scopes, 100.0),
        ]
    };
    server.set_keys(with_reader(&[READ, WRITE]));
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
    server.set_keys(with_reader(&[WRITE, READ]));
    assert!(heard.quiet().await);

    // And taken away again.
    server.set_keys(with_reader(&[READ]));
    assert!(heard.told().await);
    assert_eq!(reader.tools().await, READ_TOOLS);
    let result = reader.call("history_add", json!({"doi": "10.1/x"})).await;
    assert!(is_error(&result), "{result}");

    // One scope swapped for another, as many as before: told.
    server.set_keys(with_reader(&["notes.read"]));
    assert!(heard.told().await);
    assert!(reader.tools().await.is_empty());
    server.set_keys(with_reader(&[READ]));
    assert!(heard.told().await);

    // A key removed still ends its sessions.
    server.set_keys(vec![key("full", FULL, &[SOURCES, READ, WRITE], 100.0)]);
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

    server.set_keys(vec![key("reader", READER, &[READ, WRITE], 1.0)]);
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
        .push(tool("history_count", READ, &[]));
    server.host_tools_changed().unwrap();
    assert!(reader_heard.told().await);
    assert!(full_heard.told().await);
    assert!(reader.tools().await.contains(&"history_count".to_owned()));
    assert!(full.tools().await.contains(&"history_count".to_owned()));

    // A clash keeps the tools, and tells no one.
    host.tools.lock().unwrap().push(tool("lookup", READ, &[]));
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
        let scopes: &[&str] = if i % 2 == 0 { &[READ, WRITE] } else { &[READ] };
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

/// #144: removing is a scope of its own. Writing doesn't imply it (nor it
/// writing); a key gaining it is told, and its tools say they destroy.
#[tokio::test(flavor = "multi_thread")]
async fn removing_needs_its_own_scope() {
    let host = FakeHost::new();
    host.tools
        .lock()
        .unwrap()
        .push(tool("history_remove", REMOVE, &["doi"]));
    let server = start(&host).await;
    let port = server.port();
    let with_writer = |scopes: &[&str]| {
        vec![
            key("full", FULL, &[SOURCES, READ, WRITE], 100.0),
            key("reader", READER, scopes, 100.0),
        ]
    };
    server.set_keys(with_writer(&[READ, WRITE]));

    // Adding allowed, removing not: unlisted, and refused if called.
    let mut writer = Agent::new(port, READER);
    writer.initialize().await;
    let listed = writer.tools().await;
    assert!(listed.contains(&"history_add".to_owned()), "{listed:?}");
    assert!(!listed.contains(&"history_remove".to_owned()), "{listed:?}");
    let result = writer
        .call("history_remove", json!({"doi": "10.1/x"}))
        .await;
    assert!(is_error(&result));
    assert_eq!(
        text(&result),
        "Error (forbidden): this key can't use history_remove: it lacks the history.remove scope"
    );
    // The full key has history.write but not history.remove either.
    let mut full = Agent::new(port, FULL);
    full.initialize().await;
    assert!(!full.tools().await.contains(&"history_remove".to_owned()));

    // Granted: told, listed as destructive, and called.
    let mut heard = writer.listen().await;
    server.set_keys(with_writer(&[READ, WRITE, REMOVE]));
    assert!(heard.told().await);
    let answer = writer.rpc("tools/list", json!({})).await;
    let remove = answer["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "history_remove")
        .cloned()
        .expect("listed once granted");
    assert_eq!(remove["annotations"]["destructiveHint"], true, "{remove}");
    assert_eq!(remove["annotations"]["readOnlyHint"], false, "{remove}");
    let result = writer
        .call("history_remove", json!({"doi": "10.1/x"}))
        .await;
    assert!(!is_error(&result), "{result}");

    // Removing alone doesn't allow adding.
    server.set_keys(with_writer(&[REMOVE]));
    assert!(heard.told().await);
    let result = writer.call("history_add", json!({"doi": "10.1/x"})).await;
    assert!(is_error(&result), "{result}");
    let result = writer
        .call("history_remove", json!({"doi": "10.1/x"}))
        .await;
    assert!(!is_error(&result), "{result}");
    server.stop().await;
}

/// #153: a host tool's scope is the host's own name, and a key needs
/// exactly it: not a prefix, another case, a neighbour or `sources`. Nor
/// does it imply another, `sources` included.
#[tokio::test(flavor = "multi_thread")]
async fn a_key_needs_exactly_the_tools_scope() {
    let host = FakeHost::new();
    host.tools
        .lock()
        .unwrap()
        .push(tool("notes_add", "notes.write", &[]));
    let server = start(&host).await;
    let port = server.port();
    let as_reader = |scopes: &[&str]| {
        server.set_keys(vec![key("reader", READER, scopes, 100.0)]);
    };
    let mut agent = Agent::new(port, READER);
    for near in [
        &[SOURCES][..],
        &["notes"],
        &["notes.write.all"],
        &["NOTES.WRITE"],
        &["notes.read", READ, WRITE, REMOVE],
    ] {
        as_reader(near);
        agent.initialize().await;
        assert!(
            !agent.tools().await.contains(&"notes_add".to_owned()),
            "{near:?}"
        );
        let result = agent.call("notes_add", json!({})).await;
        assert_eq!(
            text(&result),
            "Error (forbidden): this key can't use notes_add: it lacks the notes.write scope",
            "{near:?}"
        );
    }
    // Exactly it: listed and called, and nothing else comes with it.
    as_reader(&["notes.write"]);
    agent.initialize().await;
    assert_eq!(agent.tools().await, ["notes_add"]);
    let result = agent.call("notes_add", json!({})).await;
    assert!(!is_error(&result), "{result}");
    let result = agent.call("search", json!({"text": "x"})).await;
    assert!(
        text(&result).contains("it lacks the sources scope"),
        "{result}"
    );
    // The host hears the key's scopes as the app gave them.
    let calls = host.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].2.scopes, ["notes.write"]);
    // The activity names the scope, as the agent read it.
    let activity = host.activity(7).await;
    assert_eq!(
        activity[0].error_message.as_deref(),
        Some("this key can't use notes_add: it lacks the notes.write scope")
    );
    server.stop().await;
}

/// A scope no key may hold, or no tool declare, is refused where it's
/// given: a key's at start (and left out of `set_keys`), a tool's at start
/// or when the host's tools change. One no tool declares yet grants
/// nothing.
#[tokio::test(flavor = "multi_thread")]
async fn unknown_and_malformed_scopes_are_refused() {
    let ledger = || Arc::new(MemoryLedger::default());
    for bad in ["", "history read", "Sources", "history/read", "notes:write"] {
        let mut config = config(free_port());
        config.keys.push(key("bad", "token-bad", &[READ, bad], 1.0));
        let refused = start_with(config, Some(FakeHost::new()), ledger()).await;
        match refused.err() {
            Some(StartError::Setup(e)) => {
                assert!(
                    e.starts_with(&format!("key \"bad\": its scope {bad:?} ")),
                    "{e}"
                );
            }
            other => panic!("{bad:?}: {other:?}"),
        }
    }
    for bad in ["sources", "SOURCES", "", "a b", "notes/write", "é"] {
        let host = FakeHost::new();
        host.tools.lock().unwrap().push(tool("notes_add", bad, &[]));
        let refused = start_with(config(free_port()), Some(host), ledger()).await;
        match refused.err() {
            Some(StartError::Setup(e)) => {
                assert!(
                    e.starts_with(&format!("host tool \"notes_add\": its scope {bad:?} ")),
                    "{e}"
                );
            }
            other => panic!("{bad:?}: {other:?}"),
        }
    }

    let host = FakeHost::new();
    let server = start(&host).await;
    // A malformed key is left out: its token opens nothing.
    server.set_keys(vec![key("reader", READER, &[READ, "history read"], 1.0)]);
    let mut agent = Agent::new(server.port(), READER);
    assert_eq!(agent.request("initialize", json!({})).await.0, 401);
    // A key with a scope no tool declares: nothing listed for it.
    server.set_keys(vec![key("reader", READER, &["notes.write"], 1.0)]);
    agent.initialize().await;
    assert!(agent.tools().await.is_empty());
    // A tool claiming aspero's own scope: refused, the tools kept.
    host.tools
        .lock()
        .unwrap()
        .push(tool("history_sources", SOURCES, &[]));
    match server.host_tools_changed() {
        Err(StartError::Setup(e)) => assert!(e.contains("aspero's own"), "{e}"),
        other => panic!("{other:?}"),
    }
    let mut full = Agent::new(server.port(), FULL);
    server.set_keys(keys());
    full.initialize().await;
    assert_eq!(full.tools().await.len(), ENGINE.len() + 5);
    server.stop().await;
}

/// A host tool's annotations are the hints its host declared, whatever
/// its scope; a read-only tool is never destructive.
#[tokio::test(flavor = "multi_thread")]
async fn annotations_follow_the_declared_hints() {
    let host = FakeHost::new();
    let hinted = |name: &str, read_only, destructive, idempotent| HostTool {
        read_only,
        destructive,
        idempotent,
        ..tool(name, "notes.any", &[])
    };
    host.tools.lock().unwrap().extend([
        hinted("notes_mcp_defaults", false, true, false),
        hinted("notes_add", false, false, false),
        hinted("notes_tag", false, false, true),
        hinted("notes_read", true, false, true),
        hinted("notes_read_says_destructive", true, true, false),
    ]);
    let server = start(&host).await;
    server.set_keys(vec![key(
        "reader",
        READER,
        &["notes.any", READ, WRITE],
        1.0,
    )]);
    let mut agent = Agent::new(server.port(), READER);
    agent.initialize().await;
    let answer = agent.rpc("tools/list", json!({})).await;
    let hints = |name: &str| {
        let tool = answer["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("{name} listed"));
        let a = &tool["annotations"];
        assert_eq!(a["openWorldHint"], false, "{tool}");
        assert_eq!(a["title"], format!("The {name} tool"), "{tool}");
        (
            a["readOnlyHint"].as_bool().unwrap(),
            a["destructiveHint"].as_bool().unwrap(),
            a["idempotentHint"].as_bool().unwrap(),
        )
    };
    assert_eq!(hints("notes_mcp_defaults"), (false, true, false));
    assert_eq!(hints("notes_add"), (false, false, false));
    assert_eq!(hints("notes_tag"), (false, false, true));
    assert_eq!(hints("notes_read"), (true, false, true));
    assert_eq!(hints("notes_read_says_destructive"), (true, false, false));
    // As the old fixed scopes' tools were: reading, adding.
    assert_eq!(hints("history_search"), (true, false, true));
    assert_eq!(hints("history_add"), (false, false, false));
    server.stop().await;
}

/// Every event in this test binary, from the first call: the server's
/// tasks run on other threads, so the subscriber is global.
fn global_events() -> Events {
    static EVENTS: OnceLock<Events> = OnceLock::new();
    EVENTS
        .get_or_init(|| {
            let events = Events::default();
            tracing::subscriber::set_global_default(events.clone()).unwrap();
            events
        })
        .clone()
}

/// A `tool` event per call (DESIGN "Diagnostics events"), engine tools
/// and the host's, by the key's name; what was asked only at debug.
#[tokio::test(flavor = "multi_thread")]
async fn each_tool_call_is_logged_with_the_keys_name_and_no_query() {
    let events = global_events();
    let host = FakeHost::new();
    let mut config = config(free_port());
    config.keys = vec![key("events", "token-events", &[SOURCES, READ], 100.0)];
    let server = start_with(config, Some(host), Arc::new(MemoryLedger::default()))
        .await
        .unwrap();
    let mut agent = Agent::new(server.port(), "token-events");
    agent.initialize().await;
    agent
        .call("search", json!({"text": "Lactate clearance"}))
        .await;
    agent
        .call("history_search", json!({"q": "septic shock"}))
        .await;
    agent.call("history_fail", json!({})).await;
    // A name no tool has is the client's text: `unknown` at info.
    let unknown = json!({"name": "find Biomarkers on sepsis", "arguments": {}});
    agent.rpc("tools/call", unknown).await;
    server.stop().await;

    let logged: Vec<_> = events
        .named("aspero::mcp", "tool")
        .into_iter()
        .filter(|e| e.field("key") == Some("events agent"))
        .collect();
    let said: Vec<(&str, &str, &str)> = logged
        .iter()
        .map(|e| {
            (
                e.field("tool").unwrap(),
                e.field("outcome").unwrap(),
                e.field("cost_usd").unwrap(),
            )
        })
        .collect();
    assert_eq!(
        said,
        [
            ("search", "ok", "0.250000"),
            ("history_search", "ok", "0.000000"),
            ("history_fail", "refused", "0.000000"),
            ("unknown", "unknown_tool", "0.000000"),
        ]
    );
    for e in &logged {
        assert_eq!(e.names(), ["tool", "key", "outcome", "ms", "cost_usd"]);
        assert_eq!(e.level, tracing::Level::INFO);
    }
    let all = events.all();
    let ours = all.iter().filter(|e| e.target.starts_with("aspero"));
    let (info, debug): (Vec<_>, Vec<_>) = ours.partition(|e| e.level <= tracing::Level::INFO);
    let says = |e: &&aspero_core::testing::Captured, text: &str| {
        e.name.contains(text) || e.fields.iter().any(|(_, v)| v.contains(text))
    };
    for text in ["Lactate", "septic", "Biomarkers"] {
        assert!(!info.iter().any(|e| says(e, text)), "{text} at info");
        assert!(debug.iter().any(|e| says(e, text)), "{text} not at debug");
    }
}
