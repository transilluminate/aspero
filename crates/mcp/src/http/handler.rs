//! One HTTP session's handler: the engine tools and the host's, filtered
//! by the key's scopes, under the session's and the key's budgets.

use std::borrow::Cow;
use std::sync::{Arc, OnceLock};

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, InitializeRequestParams,
    InitializeResult, ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler};
use serde_json::{Map, Value};

use super::Shared;
use super::auth::Credential;
use super::host::{Activity, HostError};
use crate::scope::SOURCES;
use crate::server::{
    INSTRUCTIONS, engine_result, error_text, server_info, tool_event, tool_input, with_call_id,
};
use crate::session::usd;
use crate::{Caller, Session, ToolError, ToolErrorKind, tools};

const HOST_INSTRUCTIONS: &str = "Tools provided by the app that embeds aspero.";

/// The key a request came with, put in its HTTP extensions once
/// authenticated.
#[derive(Debug, Clone)]
pub(crate) struct Authed(pub Arc<Credential>);

pub(crate) struct Handler {
    shared: Arc<Shared>,
    /// Opened by `initialize`, for its key and client: budgets attach to
    /// sessions, so sessionless requests (the 2026-07-28 protocol's) call
    /// no tools.
    session: OnceLock<Session>,
}

impl Handler {
    pub fn new(shared: Arc<Shared>) -> Self {
        Handler {
            shared,
            session: OnceLock::new(),
        }
    }

    /// The tools `key` may call, engine tools first.
    fn listed(&self, key: &Credential) -> Vec<Tool> {
        let scopes = &key.key.scopes;
        let mut tools = Vec::new();
        if self.shared.engine_tools && holds(scopes, SOURCES) {
            tools.extend(self.shared.engine_listed.iter().cloned());
        }
        tools.extend(
            self.shared
                .catalog()
                .iter()
                .filter(|l| holds(scopes, &l.scope))
                .map(|l| l.tool.clone()),
        );
        tools
    }

    async fn engine_call(
        &self,
        session: &Session,
        key: &Credential,
        name: &str,
        arguments: Map<String, Value>,
    ) -> (Result<CallToolResult, ErrorData>, Activity) {
        let k = &key.key;
        let said = engine_summary(name, &arguments);
        let query = engine_query(name, &arguments);
        let arguments = Value::Object(arguments);
        let spends = self.shared.tools.spec(name).is_some_and(|s| s.spends);
        // The key's sessions' paid calls in flight hold their estimates, as
        // a session's do.
        let paid = self.shared.paid(&k.id);
        let held = if !holds(&k.scopes, SOURCES) {
            Err(lacks(name, SOURCES))
        } else if spends {
            let estimate = tools::estimate(name, &arguments);
            paid.hold(estimate, || self.daily_left(key)).await.map(Some)
        } else {
            Ok(None)
        };
        let (result, cost) = match held {
            Err(e) => (Err(e), 0.0),
            Ok(held) => {
                let result = self.shared.tools.call(session, name, arguments).await;
                // Spent all the same when the call failed after spending.
                let cost = match &result {
                    Ok(output) => output.cost_usd,
                    Err(e) => e.cost_usd,
                };
                if cost > 0.0 {
                    self.shared.ledger_add(&k.id, cost);
                }
                // Let go once the spend is on record.
                drop(held);
                (result, cost)
            }
        };
        let outcome = match &result {
            Ok(output) => Ok((said, engine_count(name, &output.structured))),
            Err(e) => Err(e),
        };
        let activity = activity(k, name, query, outcome, cost);
        (engine_result(session, result), activity)
    }

    /// What's left of the key's daily budget; refuses a paid call once it's
    /// spent (or its spend can't be read).
    fn daily_left(&self, key: &Credential) -> Result<f64, ToolError> {
        let k = &key.key;
        match self.shared.key_spent(&k.id) {
            Ok(spent) if spent < k.daily_budget_usd => Ok(k.daily_budget_usd - spent),
            Ok(spent) => Err(ToolError::new(
                ToolErrorKind::BudgetSpent,
                format!(
                    "this key's daily budget is spent (${:.4} of ${:.2} today, UTC); free \
                     tools (search_supports) still work. It renews at midnight UTC, or ask \
                     the user to raise the key's daily budget in the app's settings",
                    usd(spent),
                    k.daily_budget_usd
                ),
            )),
            Err(e) => Err(ToolError::new(
                ToolErrorKind::BudgetSpent,
                format!("this key's spend today couldn't be read ({e}); try again shortly"),
            )),
        }
    }

    async fn host_call(
        &self,
        key: &Credential,
        context: &RequestContext<RoleServer>,
        name: &str,
        scope: &str,
        required: &[String],
        arguments: Map<String, Value>,
    ) -> (CallToolResult, Activity) {
        let k = &key.key;
        // The app words its own tools: no query or count.
        let fail = |e: ToolError| (tool_error(&e), activity(k, name, None, Err(&e), 0.0));
        if !holds(&k.scopes, scope) {
            return fail(lacks(name, scope));
        }
        let missing: Vec<&str> = required
            .iter()
            .filter(|f| !arguments.contains_key(f.as_str()))
            .map(String::as_str)
            .collect();
        if !missing.is_empty() {
            return fail(ToolError::new(
                ToolErrorKind::InvalidArguments,
                format!(
                    "invalid arguments for {name}: missing {}",
                    missing
                        .iter()
                        .map(|f| format!("`{f}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ));
        }
        let Some(host) = &self.shared.host else {
            return fail(ToolError::new(
                ToolErrorKind::Failed,
                format!("{name} has no host to call"),
            ));
        };
        let caller = caller(k, context);
        let json = Value::Object(arguments).to_string();
        let timeout = self.shared.host_timeout;
        let answer = tokio::time::timeout(timeout, host.call(name.into(), json, caller)).await;
        match answer {
            // Given up on here, but the app may finish (a write committed):
            // a retry could do it twice.
            Err(_) => fail(ToolError::new(
                ToolErrorKind::Timeout,
                format!(
                    "the app didn't answer {name} within {} s, so whether it did anything \
                     is unknown (it may yet): check before calling it again",
                    timeout.as_secs_f64()
                ),
            )),
            Ok(Err(e)) => fail(ToolError::new(host_kind(&e), host_message(&e))),
            Ok(Ok(output)) => match serde_json::from_str::<Value>(&output.structured) {
                Ok(structured @ Value::Object(_)) => {
                    let first = output
                        .summary
                        .lines()
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_owned();
                    let mut result =
                        CallToolResult::success(vec![ContentBlock::text(output.summary)]);
                    result.structured_content = Some(structured);
                    (result, activity(k, name, None, Ok((first, None)), 0.0))
                }
                _ => fail(ToolError::new(
                    ToolErrorKind::Failed,
                    format!("the app's {name} answered with something other than a JSON object"),
                )),
            },
        }
    }
}

impl ServerHandler for Handler {
    fn get_info(&self) -> ServerConfig {
        server_info(if self.shared.engine_tools {
            INSTRUCTIONS
        } else {
            HOST_INSTRUCTIONS
        })
    }

    /// The revisions with sessions (`initialize`): budgets attach to them.
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(ProtocolVersion::known_up_to(
            &ProtocolVersion::LATEST_WITH_INITIALIZE,
        ))
    }

    // rmcp's ServerHandler method, async by the trait.
    #[allow(clippy::unused_async_trait_impl)]
    async fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, ErrorData> {
        context.peer.set_peer_info(request.clone());
        let result = self.negotiate_initialize(&request)?;
        let key = authed(&context)?;
        // The key and client as they opened it; a call reads its key's
        // scopes as they are then.
        let session = Session::new(caller(&key.key, &context), self.shared.session_budget_usd)
            .with_raise_hint("ask the user to raise the session budget in the app's settings");
        // Initialized again, the session stays as it was.
        let _ = self.session.set(session);
        // Told when the key's scopes or the host's tools change.
        self.shared.listen(&key, context.peer.clone());
        Ok(result)
    }

    /// The tools the request's key may call, by its scopes and the host's
    /// tools now.
    // rmcp's ServerHandler method, async by the trait.
    #[allow(clippy::unused_async_trait_impl)]
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let key = authed(&context)?;
        Ok(ListToolsResult::with_all_items(self.listed(&key)))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        if self.shared.engine_tools
            && let Some(t) = self.shared.engine_listed.iter().find(|t| t.name == name)
        {
            return Some(t.clone());
        }
        self.shared
            .catalog()
            .iter()
            .find(|l| l.tool.name == name)
            .map(|l| l.tool.clone())
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let key = authed(&context)?;
        let started = tokio::time::Instant::now();
        let name = request.name.to_string();
        let Some(session) = self.session.get() else {
            let e = ToolError::new(
                ToolErrorKind::NoSession,
                "start a session (initialize) first: budgets are per session",
            );
            let known = self.shared.tools.spec(&name).is_some()
                || self.shared.catalog().iter().any(|l| l.tool.name == name);
            tool_input(&name, &request.arguments.unwrap_or_default());
            let tool = known.then_some(name.as_str());
            tool_event(
                tool,
                Some(&key.key.name),
                e.kind.as_str(),
                started,
                0.0,
                None,
            );
            return Ok(tool_error(&e).into());
        };
        let arguments = request.arguments.unwrap_or_default();
        tool_input(&name, &arguments);
        let engine = self.shared.engine_tools && self.shared.tools.spec(&name).is_some();
        let mut call = None;
        let (result, activity) = if engine {
            let ((result, activity), id) =
                with_call_id(self.engine_call(session, &key, &name, arguments)).await;
            call = id;
            (result, Some(activity))
        } else {
            // The host's tools as they are now, kept for the call.
            let catalog = self.shared.catalog();
            match catalog.iter().find(|l| l.tool.name == name) {
                Some(listed) => {
                    let (result, activity) = self
                        .host_call(
                            &key,
                            &context,
                            &name,
                            &listed.scope,
                            &listed.required,
                            arguments,
                        )
                        .await;
                    (Ok(result), Some(activity))
                }
                None => (
                    Err(ErrorData::invalid_params(
                        format!("no tool named {name:?}"),
                        None,
                    )),
                    None,
                ),
            }
        };
        let (outcome, cost) = match &activity {
            Some(a) => (a.error.map_or("ok", ToolErrorKind::as_str), a.cost_usd),
            None => (ToolErrorKind::UnknownTool.as_str(), 0.0),
        };
        let tool = activity.is_some().then_some(name.as_str());
        tool_event(tool, Some(&key.key.name), outcome, started, cost, call);
        if let Some(activity) = activity {
            self.shared.report(activity);
        }
        Ok(result?.into())
    }
}

/// The request's key, as the HTTP layer authenticated it.
fn authed(context: &RequestContext<RoleServer>) -> Result<Arc<Credential>, ErrorData> {
    context
        .extensions
        .get::<http::request::Parts>()
        .and_then(|parts| parts.extensions.get::<Authed>())
        .map(|a| a.0.clone())
        .ok_or_else(|| ErrorData::internal_error("unauthenticated request", None))
}

fn caller(key: &super::Key, context: &RequestContext<RoleServer>) -> Caller {
    Caller {
        client: context
            .peer
            .peer_info()
            .map(|info| info.client_info.name.clone()),
        key_id: Some(key.id.clone()),
        key_name: Some(key.name.clone()),
        scopes: key.scopes.clone(),
    }
}

/// Whether a key's scopes hold `scope`, exactly: no scope implies another.
fn holds(scopes: &[String], scope: &str) -> bool {
    scopes.iter().any(|s| s == scope)
}

fn lacks(tool: &str, scope: &str) -> ToolError {
    ToolError::new(
        ToolErrorKind::Forbidden,
        format!("this key can't use {tool}: it lacks the {scope} scope"),
    )
}

fn tool_error(e: &ToolError) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(error_text(e))])
}

/// A host error's kind, as the agent reads it.
fn host_kind(e: &HostError) -> ToolErrorKind {
    match e {
        HostError::InvalidArguments(_) => ToolErrorKind::InvalidArguments,
        HostError::NotFound(_) => ToolErrorKind::NotFound,
        HostError::Refused(_) => ToolErrorKind::Refused,
        HostError::Failed(_) => ToolErrorKind::Failed,
    }
}

/// A host error's message, or its kind when the app gave none.
fn host_message(e: &HostError) -> String {
    match e.message().trim() {
        "" => e.kind().replace('_', " "),
        m => m.to_owned(),
    }
}

/// An activity, its summary and its fields from the same outcome (the
/// summary and count, or the error), so they can't disagree.
fn activity(
    key: &super::Key,
    tool: &str,
    query: Option<String>,
    outcome: Result<(String, Option<u64>), &ToolError>,
    cost_usd: f64,
) -> Activity {
    let (summary, count, error) = match outcome {
        Ok((summary, count)) => (summary, count, None),
        Err(e) => (error_summary(&e.message), None, Some(e)),
    };
    Activity {
        key_name: key.name.clone(),
        tool: tool.into(),
        summary,
        is_error: error.is_some(),
        cost_usd,
        error: error.map(|e| e.kind),
        error_message: error.map(|e| e.message.trim().to_owned()),
        query,
        count,
    }
}

/// "error: " and the message's first line, cut to a few words' length.
fn error_summary(message: &str) -> String {
    format!("error: {}", clip(message.lines().next().unwrap_or(""), 80))
}

fn clip(s: &str, max: usize) -> String {
    let s = s.trim();
    match s.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", s[..at].trim_end()),
        None => s.to_owned(),
    }
}

/// A few words on an engine tool's call, from its arguments: "searched
/// ‘lactate clearance’", "looked up 3 works", "verified 2 citations".
pub(crate) fn engine_summary(name: &str, arguments: &Map<String, Value>) -> String {
    let count = |key: &str| arguments.get(key).and_then(Value::as_array).map(Vec::len);
    let quoted = |s: &str| format!("‘{}’", clip(s, 60));
    let plural = |n: usize, one: &str, many: &str| match n {
        1 => format!("a {one}"),
        n => format!("{n} {many}"),
    };
    let id = || {
        arguments
            .get("id")
            .and_then(Value::as_str)
            .map_or_else(|| "a work".into(), quoted)
    };
    match name {
        "search" => {
            let words = arguments
                .get("text")
                .and_then(Value::as_str)
                .map(String::from)
                .or_else(|| arguments.get("query").and_then(first_term));
            match words {
                Some(w) => format!("searched {}", quoted(&w)),
                None => "searched".into(),
            }
        }
        "search_supports" => "checked whether a search is supported".into(),
        "lookup" => format!(
            "looked up {}",
            plural(count("ids").unwrap_or(0), "work", "works")
        ),
        "verify" => match count("citations") {
            Some(n) => format!("verified {}", plural(n, "citation", "citations")),
            None => "verified a citation".into(),
        },
        "identify" => "identified a document".into(),
        "citing_works" => format!("found works citing {}", id()),
        "references" => format!("listed the references of {}", id()),
        _ => format!("called {name}"),
    }
}

/// Most characters of an activity's query.
const QUERY_CHARS: usize = 500;

/// What an engine tool was asked, as given, from its arguments: `search`'s
/// keywords or tree, identifiers and citations joined with "; ".
fn engine_query(name: &str, arguments: &Map<String, Value>) -> Option<String> {
    let text = |key: &str| arguments.get(key).and_then(Value::as_str).map(String::from);
    let tree = || arguments.get("query").map(|q| tree_line(q, false));
    let joined = |items: Option<&Value>, line: fn(&Value) -> String| {
        items
            .and_then(Value::as_array)
            .map(|items| items.iter().map(line).collect::<Vec<_>>().join("; "))
    };
    let query = match name {
        "search" => text("text").or_else(tree),
        "search_supports" => tree(),
        "lookup" => joined(arguments.get("ids"), as_given),
        "verify" => arguments
            .get("citation")
            .map(citation_line)
            .or_else(|| joined(arguments.get("citations"), citation_line)),
        "identify" => ["title_hint", "title"]
            .into_iter()
            .filter_map(text)
            .find(|t| !t.trim().is_empty())
            .or_else(|| {
                text("text")?
                    .lines()
                    .map(str::trim)
                    .find(|l| !l.is_empty())
                    .map(String::from)
            }),
        "citing_works" | "references" => text("id"),
        _ => None,
    };
    query
        .map(|q| clip(&q, QUERY_CHARS))
        .filter(|q| !q.is_empty())
}

/// How many came back, from an engine tool's result: works, lookups,
/// verdicts.
fn engine_count(name: &str, structured: &Value) -> Option<u64> {
    let len = |key: &str| structured[key].as_array().map(|a| a.len() as u64);
    match name {
        "search" | "citing_works" | "references" => len("hits"),
        "lookup" | "verify" => len("results"),
        "identify" => Some(1),
        _ => None,
    }
}

/// A string as it is; anything else as JSON.
fn as_given(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// A citation as given: its text, or its fields in a line.
fn citation_line(citation: &Value) -> String {
    match serde_json::from_value::<aspero_json::ClaimInput>(citation.clone()) {
        Ok(fields) => match &fields.text {
            Some(text) => text.clone(),
            None => fields.describe(),
        },
        _ => as_given(citation),
    }
}

/// A query tree in a line, as written: `title:sepsis AND (journal:"Lancet"
/// OR journal:"BMJ") AND NOT title:neonatal AND years:2018–`. A node it
/// can't read is shown as JSON.
fn tree_line(node: &Value, nested: bool) -> String {
    let Value::Object(map) = node else {
        return node.to_string();
    };
    let mut keys = map.keys().filter(|k| *k != "phrase");
    let (Some(key), None) = (keys.next(), keys.next()) else {
        return node.to_string();
    };
    let value = &map[key.as_str()];
    let phrase = map.get("phrase") == Some(&Value::Bool(true));
    let bracket = |line: String, many: bool| {
        if nested && many {
            format!("({line})")
        } else {
            line
        }
    };
    let group = |items: &[Value], op: &str| {
        items
            .iter()
            .map(|n| tree_line(n, true))
            .collect::<Vec<_>>()
            .join(op)
    };
    match (key.as_str(), value) {
        ("all", Value::Array(items)) => bracket(group(items, " AND "), items.len() > 1),
        ("any", Value::Array(items)) => bracket(group(items, " OR "), items.len() > 1),
        ("none", Value::Array(items)) => match items.as_slice() {
            [one] => format!("NOT {}", tree_line(one, true)),
            _ => format!("NOT ({})", group(items, " OR ")),
        },
        (_, Value::String(s)) => {
            let many = s.split_whitespace().nth(1).is_some();
            let words = if phrase {
                format!("\"{s}\"")
            } else if many && (nested || key != "keywords") {
                format!("({s})")
            } else {
                s.clone()
            };
            match key.as_str() {
                "keywords" => words,
                _ => format!("{key}:{words}"),
            }
        }
        ("years", Value::Object(range)) => {
            let year = |end: &str| range.get(end).map(as_given).unwrap_or_default();
            format!("years:{}–{}", year("from"), year("to"))
        }
        (_, value) => format!("{key}:{}", as_given(value)),
    }
}

/// A query tree's first term's words: keywords, a title, an author, ….
fn first_term(node: &Value) -> Option<String> {
    const TERMS: [&str; 6] = [
        "keywords", "title", "author", "journal", "abstract", "phrase",
    ];
    match node {
        Value::Object(map) => {
            for key in TERMS {
                if let Some(Value::String(s)) = map.get(key) {
                    return Some(s.clone());
                }
            }
            map.values().find_map(first_term)
        }
        Value::Array(items) => items.iter().find_map(first_term),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn args(v: Value) -> Map<String, Value> {
        let Value::Object(map) = v else {
            panic!("arguments are an object")
        };
        map
    }

    #[test]
    fn engine_calls_are_summed_up_in_a_few_words() {
        let say = |name: &str, v: Value| engine_summary(name, &args(v));
        assert_eq!(
            say("search", json!({"text": "lactate clearance"})),
            "searched ‘lactate clearance’"
        );
        assert_eq!(
            say(
                "search",
                json!({"query": {"all": [{"years": {"from": 2018}}, {"title": "sepsis"}]}})
            ),
            "searched ‘sepsis’"
        );
        assert_eq!(
            say("lookup", json!({"ids": ["a", "b", "c"]})),
            "looked up 3 works"
        );
        assert_eq!(say("lookup", json!({"ids": ["a"]})), "looked up a work");
        assert_eq!(
            say("verify", json!({"citations": ["a", "b"]})),
            "verified 2 citations"
        );
        assert_eq!(
            say("verify", json!({"citation": "a"})),
            "verified a citation"
        );
        assert_eq!(
            say("identify", json!({"text": "…"})),
            "identified a document"
        );
        assert_eq!(
            say("citing_works", json!({"id": "10.1/x"})),
            "found works citing ‘10.1/x’"
        );
        assert_eq!(
            error_summary(&format!("{}\nmore", "x".repeat(100))),
            format!("error: {}…", "x".repeat(80))
        );
    }

    #[test]
    fn an_engine_calls_query_is_what_was_asked_in_a_line() {
        let asked = |name: &str, v: Value| engine_query(name, &args(v));
        assert_eq!(
            asked(
                "search",
                json!({"query": {"all": [
                    {"title": "sepsis"},
                    {"any": [{"journal": "Lancet", "phrase": true}, {"journal": "BMJ", "phrase": true}]},
                    {"none": [{"title": "neonatal"}]},
                    {"keywords": "lactate clearance"},
                    {"years": {"from": 2018}},
                    {"not_retracted": true},
                ]}})
            )
            .as_deref(),
            Some(
                "title:sepsis AND (journal:\"Lancet\" OR journal:\"BMJ\") AND NOT title:neonatal \
                 AND (lactate clearance) AND years:2018– AND not_retracted:true"
            )
        );
        assert_eq!(
            asked(
                "search_supports",
                json!({"query": {"none": [{"author": "Smith J"}, {"years": 2020}, {"x": [1]}]}})
            )
            .as_deref(),
            Some("NOT (author:(Smith J) OR years:2020 OR x:[1])")
        );
        assert_eq!(
            asked("search", json!({"query": {"keywords": "deep learning"}})).as_deref(),
            Some("deep learning")
        );
        assert_eq!(
            asked(
                "verify",
                json!({"citation": {"doi": "10.1/x", "year": 2015}})
            )
            .as_deref(),
            Some("2015. 10.1/x")
        );
        assert_eq!(
            asked(
                "identify",
                json!({"text": "\n  Journal of Things\nA title", "title_hint": " "})
            )
            .as_deref(),
            Some("Journal of Things")
        );
        assert_eq!(
            asked(
                "identify",
                json!({"text": "Journal of Things", "title_hint": "", "title": "A title"})
            )
            .as_deref(),
            Some("A title")
        );
        assert_eq!(
            asked("references", json!({"id": "10.1/x"})).as_deref(),
            Some("10.1/x")
        );
        assert_eq!(
            asked("lookup", json!({"ids": ["x".repeat(400), "y".repeat(400)]})),
            Some(format!("{}; {}…", "x".repeat(400), "y".repeat(98)))
        );
        assert_eq!(asked("search", json!({})), None);
        assert_eq!(asked("lookup", json!({"ids": []})), None);
    }

    #[test]
    fn an_engine_calls_count_is_how_many_came_back() {
        let hits = json!({"hits": [{}, {}], "results": [{}]});
        assert_eq!(engine_count("search", &hits), Some(2));
        assert_eq!(engine_count("references", &hits), Some(2));
        assert_eq!(engine_count("verify", &hits), Some(1));
        assert_eq!(engine_count("identify", &json!({})), Some(1));
        assert_eq!(engine_count("search_supports", &hits), None);
    }
}
