//! One HTTP session's handler: the engine tools and the host's, filtered
//! by the key's scopes, under the session's and the key's budgets.

use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, InitializeRequestParams,
    InitializeResult, ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler};
use serde_json::{Map, Value};

use super::auth::Credential;
use super::host::{Activity, HostError};
use super::{Shared, today};
use crate::server::{INSTRUCTIONS, engine_result, server_info};
use crate::session::usd;
use crate::{Caller, Scope, Session, ToolError, ToolErrorKind};

const HOST_INSTRUCTIONS: &str = "Tools provided by the app that embeds aspero.";

/// The key a request came with, put in its HTTP extensions once
/// authenticated.
#[derive(Debug, Clone)]
pub(crate) struct Authed(pub Arc<Credential>);

pub(crate) struct Handler {
    shared: Arc<Shared>,
    session: Session,
    /// Whether the client has initialized: budgets attach to sessions, so
    /// sessionless requests (the 2026-07-28 protocol's) call no tools.
    initialized: AtomicBool,
}

impl Handler {
    pub fn new(shared: Arc<Shared>) -> Self {
        let session = Session::new(Caller::default(), shared.session_budget_usd)
            .with_raise_hint("ask the user to raise the session budget in the app's settings");
        Handler {
            shared,
            session,
            initialized: AtomicBool::new(false),
        }
    }

    /// The tools `key` may call, engine tools first.
    fn listed(&self, key: &Credential) -> Vec<Tool> {
        let scopes = &key.key.scopes;
        let mut tools = Vec::new();
        if self.shared.engine_tools && scopes.contains(&Scope::Sources) {
            tools.extend(self.shared.engine_listed.iter().cloned());
        }
        tools.extend(
            self.shared
                .catalog()
                .iter()
                .filter(|l| scopes.contains(&l.scope))
                .map(|l| l.tool.clone()),
        );
        tools
    }

    async fn engine_call(
        &self,
        key: &Credential,
        name: &str,
        arguments: Map<String, Value>,
    ) -> (Result<CallToolResult, ErrorData>, Activity) {
        let k = &key.key;
        let spends = self.shared.tools.spec(name).is_some_and(|s| s.spends);
        let refused = if !k.scopes.contains(&Scope::Sources) {
            Some(lacks(name, Scope::Sources))
        } else if spends {
            self.over_daily_budget(key).err()
        } else {
            None
        };
        let result = match refused {
            Some(e) => Err(e),
            None => {
                self.shared
                    .tools
                    .call(&self.session, name, Value::Object(arguments.clone()))
                    .await
            }
        };
        let cost = result.as_ref().map_or(0.0, |o| o.cost_usd);
        if cost > 0.0 {
            self.shared.ledger_add(&k.id, cost);
        }
        let activity = Activity {
            key_name: k.name.clone(),
            tool: name.into(),
            summary: match &result {
                Ok(_) => engine_summary(name, &arguments),
                Err(e) => error_summary(&e.message),
            },
            is_error: result.is_err(),
            cost_usd: cost,
        };
        (engine_result(&self.session, name, result), activity)
    }

    /// Refuses a paid call once the key has spent its daily budget (or its
    /// spend can't be read).
    fn over_daily_budget(&self, key: &Credential) -> Result<(), ToolError> {
        let k = &key.key;
        match self.shared.ledger.spent(&k.id, &today()) {
            Ok(spent) if spent < k.daily_budget_usd => Ok(()),
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
        scope: Scope,
        required: &[String],
        arguments: Map<String, Value>,
    ) -> (CallToolResult, Activity) {
        let k = &key.key;
        let activity = |summary: String, is_error: bool| Activity {
            key_name: k.name.clone(),
            tool: name.into(),
            summary,
            is_error,
            cost_usd: 0.0,
        };
        let fail = |kind: &str, message: String| {
            let summary = error_summary(&message);
            (tool_error(kind, &message), activity(summary, true))
        };
        if !k.scopes.contains(&scope) {
            let e = lacks(name, scope);
            return fail("forbidden", e.message);
        }
        let missing: Vec<&str> = required
            .iter()
            .filter(|f| !arguments.contains_key(f.as_str()))
            .map(String::as_str)
            .collect();
        if !missing.is_empty() {
            return fail(
                "invalid_arguments",
                format!(
                    "invalid arguments for {name}: missing {}",
                    missing
                        .iter()
                        .map(|f| format!("`{f}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
        }
        let Some(host) = &self.shared.host else {
            return fail("failed", format!("{name} has no host to call"));
        };
        let caller = caller(k, context);
        let json = Value::Object(arguments).to_string();
        let timeout = self.shared.host_timeout;
        let answer = tokio::time::timeout(timeout, host.call(name.into(), json, caller)).await;
        match answer {
            Err(_) => fail(
                "timeout",
                format!(
                    "the app didn't answer {name} within {} s",
                    timeout.as_secs_f64()
                ),
            ),
            Ok(Err(e)) => fail(e.kind(), host_message(&e)),
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
                    (result, activity(first, false))
                }
                _ => fail(
                    "failed",
                    format!("the app's {name} answered with something other than a JSON object"),
                ),
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

    async fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, ErrorData> {
        self.initialized.store(true, Ordering::Relaxed);
        context.peer.set_peer_info(request.clone());
        self.negotiate_initialize(&request)
    }

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
        if !self.initialized.load(Ordering::Relaxed) {
            return Ok(tool_error(
                "no_session",
                "start a session (initialize) first: budgets are per session",
            )
            .into());
        }
        let name = request.name.to_string();
        let arguments = request.arguments.unwrap_or_default();
        let engine = self.shared.engine_tools && self.shared.tools.spec(&name).is_some();
        let (result, activity) = if engine {
            let (result, activity) = self.engine_call(&key, &name, arguments).await;
            (result, Some(activity))
        } else {
            let host = self
                .shared
                .catalog()
                .iter()
                .find(|l| l.tool.name == name)
                .map(|l| (l.scope, l.required.clone()));
            match host {
                Some((scope, required)) => {
                    let (result, activity) = self
                        .host_call(&key, &context, &name, scope, &required, arguments)
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

fn lacks(tool: &str, scope: Scope) -> ToolError {
    ToolError::new(
        ToolErrorKind::Forbidden,
        format!(
            "this key can't use {tool}: it lacks the {} scope",
            scope.as_str()
        ),
    )
}

fn tool_error(kind: &str, message: &str) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(format!(
        "Error ({kind}): {message}"
    ))])
}

/// A host error's message, or its kind when the app gave none.
fn host_message(e: &HostError) -> String {
    match e.message().trim() {
        "" => e.kind().replace('_', " "),
        m => m.to_owned(),
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
            .map(quoted)
            .unwrap_or_else(|| "a work".into())
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
        v.as_object().unwrap().clone()
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
}
