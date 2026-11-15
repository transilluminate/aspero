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
use crate::server::{INSTRUCTIONS, engine_result, error_text, server_info};
use crate::session::usd;
use crate::{Caller, Scope, Session, ToolError, ToolErrorKind, tools};

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
        session: &Session,
        key: &Credential,
        name: &str,
        arguments: Map<String, Value>,
    ) -> (Result<CallToolResult, ErrorData>, Activity) {
        let k = &key.key;
        let said = engine_summary(name, &arguments);
        let arguments = Value::Object(arguments);
        let spends = self.shared.tools.spec(name).is_some_and(|s| s.spends);
        // The key's sessions' paid calls in flight hold their estimates, as
        // a session's do.
        let paid = self.shared.paid(&k.id);
        let held = if !k.scopes.contains(&Scope::Sources) {
            Err(lacks(name, Scope::Sources))
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
        let activity = Activity {
            key_name: k.name.clone(),
            tool: name.into(),
            summary: match &result {
                Ok(_) => said,
                Err(e) => error_summary(&e.message),
            },
            is_error: result.is_err(),
            cost_usd: cost,
        };
        (engine_result(session, name, result), activity)
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
        let fail = |e: ToolError| (tool_error(&e), activity(error_summary(&e.message), true));
        if !k.scopes.contains(&scope) {
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
                    (result, activity(first, false))
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
        let Some(session) = self.session.get() else {
            return Ok(tool_error(&ToolError::new(
                ToolErrorKind::NoSession,
                "start a session (initialize) first: budgets are per session",
            ))
            .into());
        };
        let name = request.name.to_string();
        let arguments = request.arguments.unwrap_or_default();
        let engine = self.shared.engine_tools && self.shared.tools.spec(&name).is_some();
        let (result, activity) = if engine {
            let (result, activity) = self.engine_call(session, &key, &name, arguments).await;
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
                            listed.scope,
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
