//! Agents in an app (the `mcp` feature): an [`McpServer`] the app starts
//! on a loopback port of its choosing, serving the engine's tools through
//! the app's own [`Aspero`] handle (its key, cap, disabled sources and
//! cache) and the app's own tools, through [`McpHost`], to agents holding
//! the app's bearer keys. See `aspero_mcp::http`.

use std::sync::Arc;
use std::time::SystemTime;

use aspero_mcp::http::{self, HttpConfig, HttpServer, Ledger, MemoryLedger, StartError};
use aspero_mcp::{Caller, Scope, Tools};

use crate::Aspero;

/// What a key may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum McpScope {
    /// The engine tools: search, look up and verify works in open sources,
    /// spending from the key's daily budget.
    Sources,
    /// The app's tools that read its history.
    ReadHistory,
    /// The app's tools that add to its history.
    WriteHistory,
}

impl From<McpScope> for Scope {
    fn from(s: McpScope) -> Self {
        match s {
            McpScope::Sources => Scope::Sources,
            McpScope::ReadHistory => Scope::ReadHistory,
            McpScope::WriteHistory => Scope::WriteHistory,
        }
    }
}

impl From<Scope> for McpScope {
    fn from(s: Scope) -> Self {
        match s {
            Scope::Sources => McpScope::Sources,
            Scope::ReadHistory => McpScope::ReadHistory,
            Scope::WriteHistory => McpScope::WriteHistory,
        }
    }
}

/// A tool the app serves.
#[derive(Debug, Clone, uniffi::Record)]
pub struct HostTool {
    /// Unique, and unlike the engine tools' (`search`, `search_supports`,
    /// `lookup`, `verify`, `identify`, `citing_works`, `references`).
    pub name: String,
    pub title: String,
    pub description: String,
    /// JSON Schema of the arguments: an object. Its `required` fields are
    /// checked before the app is called.
    pub input_schema: String,
    /// JSON Schema of the result's `structured`: an object.
    #[uniffi(default = None)]
    pub output_schema: Option<String>,
    /// What a key needs to see and call it.
    pub scope: McpScope,
}

/// A tool's answer.
#[derive(Debug, Clone, uniffi::Record)]
pub struct HostToolResult {
    /// A JSON object: what the agent gets as structured content.
    pub structured: String,
    /// A few lines for the agent; the first is the activity's summary.
    pub summary: String,
}

/// Why a tool failed: the agent reads the message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum HostToolError {
    #[error("{message}")]
    InvalidArguments { message: String },
    #[error("{message}")]
    NotFound { message: String },
    #[error("{message}")]
    Refused { message: String },
    #[error("{message}")]
    Failed { message: String },
}

/// A callback that threw something else, or failed: the tool failed.
impl From<uniffi::UnexpectedUniFFICallbackError> for HostToolError {
    fn from(e: uniffi::UnexpectedUniFFICallbackError) -> Self {
        HostToolError::Failed { message: e.reason }
    }
}

/// Who is calling: the key the agent presented.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct McpCaller {
    pub key_id: String,
    pub key_name: String,
    pub scopes: Vec<McpScope>,
}

/// A tool call, done.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct McpActivity {
    pub key_name: String,
    pub tool: String,
    /// A few words: "searched ‘lactate clearance’", "looked up 3 works", a
    /// host tool's summary's first line, or "error: …".
    pub summary: String,
    pub is_error: bool,
    /// What it spent, in US dollars; the app's tools cost nothing.
    pub cost_usd: f64,
}

/// The app, to the MCP server: its tools, and what agents did.
#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait McpHost: Send + Sync {
    /// The app's tools: asked at start and on `hostToolsChanged()`.
    fn tools(&self) -> Vec<HostTool>;

    /// Calls the tool `name` with `arguments` (a JSON object, with its
    /// schema's required fields) for `caller`, whose key has the tool's
    /// scope. Abandoned after 30 seconds, when the agent gets an error.
    async fn call(
        &self,
        name: String,
        arguments: String,
        caller: McpCaller,
    ) -> Result<HostToolResult, HostToolError>;

    /// After every tool call, the engine's or the app's, success or
    /// error: on a background thread, in order. Return quickly.
    fn activity(&self, activity: McpActivity);
}

/// A bearer key the app minted: aspero sees only its token's hash.
#[derive(Debug, Clone, uniffi::Record)]
pub struct McpKey {
    pub id: String,
    /// Shown in activity: "Claude Code".
    pub name: String,
    pub scopes: Vec<McpScope>,
    /// The SHA-256 of the token, as 64 hex digits (lowercase).
    pub sha256: String,
    /// Engine spend allowed per UTC day, in US dollars, kept in the cache
    /// file (in memory without one).
    #[uniffi(default = 1.0)]
    pub daily_budget_usd: f64,
}

impl From<McpKey> for http::Key {
    fn from(k: McpKey) -> Self {
        http::Key {
            id: k.id,
            name: k.name,
            scopes: k.scopes.into_iter().map(Into::into).collect(),
            sha256: k.sha256,
            daily_budget_usd: k.daily_budget_usd,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct McpServerConfig {
    /// On 127.0.0.1 only. Never moved: if it's taken, `start` throws
    /// `portInUse`.
    #[uniffi(default = 23130)]
    pub port: u16,
    pub keys: Vec<McpKey>,
    /// Each session's spend cap, in US dollars.
    #[uniffi(default = 0.25)]
    pub session_budget_usd: f64,
    /// Whether the engine's tools are served, or only the app's.
    #[uniffi(default = true)]
    pub engine_tools: bool,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct McpStatus {
    pub running: bool,
    pub port: u16,
    /// Sessions open now.
    pub sessions: u32,
    pub last_request_at: Option<SystemTime>,
    pub last_key_name: Option<String>,
    pub keys: Vec<McpKeyStatus>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct McpKeyStatus {
    pub key_id: String,
    /// Its last request since the server started.
    pub last_used_at: Option<SystemTime>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum McpError {
    /// Something else listens on the port.
    #[error("port {port} is in use")]
    PortInUse { port: u16 },
    /// An app tool is named like an engine tool, or like another app tool.
    #[error("two tools are named {name:?}")]
    ToolNameClash { name: String },
    /// A key's hash or a budget is malformed, or the server couldn't start.
    #[error("could not set up the MCP server: {message}")]
    Setup { message: String },
}

impl From<StartError> for McpError {
    fn from(e: StartError) -> Self {
        match e {
            StartError::PortInUse { port } => McpError::PortInUse { port },
            StartError::ToolNameClash { name } => McpError::ToolNameClash { name },
            StartError::Setup(message) => McpError::Setup { message },
            other => McpError::Setup {
                message: other.to_string(),
            },
        }
    }
}

/// The app's [`McpHost`] as the server's host.
struct ForeignHost(Arc<dyn McpHost>);

#[async_trait::async_trait]
impl http::Host for ForeignHost {
    fn tools(&self) -> Vec<http::HostTool> {
        self.0
            .tools()
            .into_iter()
            .map(|t| http::HostTool {
                name: t.name,
                title: t.title,
                description: t.description,
                input_schema: t.input_schema,
                output_schema: t.output_schema,
                scope: t.scope.into(),
            })
            .collect()
    }

    async fn call(
        &self,
        name: String,
        arguments: String,
        caller: Caller,
    ) -> Result<http::HostOutput, http::HostError> {
        let caller = McpCaller {
            key_id: caller.key_id.unwrap_or_default(),
            key_name: caller.key_name.unwrap_or_default(),
            scopes: caller.scopes.into_iter().map(Into::into).collect(),
        };
        match self.0.call(name, arguments, caller).await {
            Ok(r) => Ok(http::HostOutput {
                structured: r.structured,
                summary: r.summary,
            }),
            Err(e) => Err(match e {
                HostToolError::InvalidArguments { message } => {
                    http::HostError::InvalidArguments(message)
                }
                HostToolError::NotFound { message } => http::HostError::NotFound(message),
                HostToolError::Refused { message } => http::HostError::Refused(message),
                HostToolError::Failed { message } => http::HostError::Failed(message),
            }),
        }
    }

    fn activity(&self, a: http::Activity) {
        self.0.activity(McpActivity {
            key_name: a.key_name,
            tool: a.tool,
            summary: a.summary,
            is_error: a.is_error,
            cost_usd: a.cost_usd,
        });
    }
}

/// An MCP server over Streamable HTTP at `http://127.0.0.1:<port>/mcp`.
/// Requests need `Authorization: Bearer <token>` for one of the keys;
/// browsers (an `Origin`) and other host names are refused. Stop it with
/// `stop()`; dropping it stops it too.
#[derive(uniffi::Object)]
pub struct McpServer {
    aspero: Arc<Aspero>,
    inner: Arc<HttpServer>,
}

#[uniffi::export]
impl McpServer {
    /// Starts serving `aspero`'s engine tools (unless `engine_tools` is
    /// off) and `host`'s, through `aspero`'s engine: its key, cap,
    /// disabled sources and cache, where per-key spend is kept.
    #[uniffi::constructor(default(host = None))]
    pub async fn start(
        aspero: Arc<Aspero>,
        config: McpServerConfig,
        host: Option<Arc<dyn McpHost>>,
    ) -> Result<Arc<Self>, McpError> {
        let tools = Arc::new(Tools::new(aspero.engine.clone()));
        let ledger: Arc<dyn Ledger> = match &aspero.cache {
            Some(cache) => cache.clone(),
            None => Arc::new(MemoryLedger::default()),
        };
        let host = host.map(|h| Arc::new(ForeignHost(h)) as Arc<dyn http::Host>);
        let mut http_config = HttpConfig::default();
        http_config.port = config.port;
        http_config.keys = config.keys.into_iter().map(Into::into).collect();
        http_config.session_budget_usd = config.session_budget_usd;
        http_config.engine_tools = config.engine_tools;
        let started = aspero
            .run(
                async move { HttpServer::start(tools, http_config, host, ledger).await },
                None,
            )
            .await
            .map_err(|e| McpError::Setup {
                message: e.to_string(),
            })??;
        Ok(Arc::new(McpServer {
            aspero,
            inner: Arc::new(started),
        }))
    }

    /// Replaces the keys at once. Sessions of a key that's gone, or whose
    /// hash changed, end: their next request is refused (401). Sessions of
    /// a key whose scopes changed (same id, same hash) are told their tools
    /// changed (`notifications/tools/list_changed`). A key whose hash isn't
    /// 64 hex digits is left out. Returns without waiting on any agent.
    pub fn set_keys(&self, keys: Vec<McpKey>) {
        self.inner
            .set_keys(keys.into_iter().map(Into::into).collect());
    }

    /// Asks the host for its tools again, and tells every open session its
    /// tools changed (`notifications/tools/list_changed`), without waiting
    /// on any agent. On a clash (or a schema that isn't a JSON object)
    /// throws, keeping the tools it had.
    pub fn host_tools_changed(&self) -> Result<(), McpError> {
        Ok(self.inner.host_tools_changed()?)
    }

    pub fn port(&self) -> u16 {
        self.inner.port()
    }

    pub fn status(&self) -> McpStatus {
        let s = self.inner.status();
        McpStatus {
            running: s.running,
            port: s.port,
            sessions: s.sessions,
            last_request_at: s.last_request_at.map(SystemTime::from),
            last_key_name: s.last_key_name,
            keys: s
                .keys
                .into_iter()
                .map(|k| McpKeyStatus {
                    key_id: k.key_id,
                    last_used_at: k.last_used_at.map(SystemTime::from),
                })
                .collect(),
        }
    }

    /// Ends the sessions and stops listening; the port is free when it
    /// returns. Stopping again does nothing.
    pub async fn stop(&self) {
        let inner = self.inner.clone();
        let _ = self
            .aspero
            .run(async move { inner.stop().await }, None)
            .await;
    }
}
