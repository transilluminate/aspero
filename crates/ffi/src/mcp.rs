//! Agents in an app (the `mcp` feature): an [`McpServer`] the app starts
//! on a loopback port of its choosing, serving the engine's tools through
//! the app's own [`Aspero`] handle (its key, cap, disabled sources and
//! cache) and the app's own tools, through [`McpHost`], to agents holding
//! the app's bearer keys. See `aspero_mcp::http`.

use std::sync::Arc;
use std::time::SystemTime;

use aspero_mcp::http::{self, HttpConfig, HttpServer, Ledger, MemoryLedger, StartError};
use aspero_mcp::{Caller, ToolErrorKind, Tools};

use crate::Aspero;

/// A tool the app serves.
#[derive(Debug, Clone, uniffi::Record)]
pub struct HostTool {
    /// Unique, and unlike the engine tools' (`search`, `search_supports`,
    /// `lookup`, `verify`, `identify`, `citing_works`, `references`,
    /// `analyse`).
    pub name: String,
    pub title: String,
    pub description: String,
    /// JSON Schema of the arguments: an object. Its `required` fields are
    /// checked before the app is called.
    pub input_schema: String,
    /// JSON Schema of the result's `structured`: an object.
    #[uniffi(default = None)]
    pub output_schema: Option<String>,
    /// What a key needs to see and call it: a name the app chooses
    /// ("history.read", "notes.write"): at most 64 ASCII letters, digits,
    /// `.`, `_` and `-`, and not `sources` (aspero's own). A key needs
    /// exactly this scope in its `scopes`: no scope implies another. A tool
    /// whose scope breaks these rules fails `start` or `hostToolsChanged()`
    /// (`setup`).
    pub scope: String,
    /// It only reads: changes nothing in the app's data. Off unless set,
    /// as MCP assumes.
    #[uniffi(default = false)]
    pub read_only: bool,
    /// Unless read-only, it may take something away (remove, untag,
    /// delete, overwrite) rather than only add. On unless cleared, as MCP
    /// assumes: clear it for a tool that only adds. A read-only tool is
    /// never listed as destructive.
    #[uniffi(default = true)]
    pub destructive: bool,
    /// Calling it again with the same arguments does nothing more. Off
    /// unless set, as MCP assumes.
    #[uniffi(default = false)]
    pub idempotent: bool,
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
    /// The key's scopes, as the app gave them.
    pub scopes: Vec<String>,
}

/// Why a tool call failed: the agent's own doing (`invalidArguments`,
/// `notFound`, `forbidden`, `unsupported`, `budgetSpent`) or trouble
/// elsewhere (`unavailable`, `throttled`, `auth`, `protocol`, the app's
/// `timeout`, `refused`, `failed`). New kinds may be added (`other`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum McpErrorKind {
    /// No such tool (a protocol error: no activity is reported for it).
    UnknownTool,
    /// The arguments are wrong, or an app tool's required field is missing;
    /// or the app threw `invalidArguments`.
    InvalidArguments,
    /// The source can't do this (a query tree it can't say).
    Unsupported,
    /// Nothing found (`identify`), or the app threw `notFound`.
    NotFound,
    /// The session's budget, or the key's daily one, is spent.
    BudgetSpent,
    /// The key lacks the tool's scope.
    Forbidden,
    /// A provider's rate limit or own budget (OpenAlex's daily one).
    Throttled,
    /// A provider rejected aspero's credentials.
    Auth,
    /// Providers failed (network, timeouts, server errors).
    Unavailable,
    /// A provider answered unexpectedly.
    Protocol,
    /// A call outside a session.
    NoSession,
    /// The app didn't answer one of its tools in time: whether it did
    /// anything is unknown.
    Timeout,
    /// The app threw `refused`.
    Refused,
    /// The app threw `failed` (or something else), or answered with
    /// something other than a JSON object.
    Failed,
    /// A kind added since: read `errorMessage`. Last, so the cases before
    /// keep their places.
    Other,
}

impl From<ToolErrorKind> for McpErrorKind {
    fn from(kind: ToolErrorKind) -> Self {
        match kind {
            ToolErrorKind::UnknownTool => McpErrorKind::UnknownTool,
            ToolErrorKind::InvalidArguments => McpErrorKind::InvalidArguments,
            ToolErrorKind::Unsupported => McpErrorKind::Unsupported,
            ToolErrorKind::NotFound => McpErrorKind::NotFound,
            ToolErrorKind::BudgetSpent => McpErrorKind::BudgetSpent,
            ToolErrorKind::Forbidden => McpErrorKind::Forbidden,
            ToolErrorKind::Throttled => McpErrorKind::Throttled,
            ToolErrorKind::Auth => McpErrorKind::Auth,
            ToolErrorKind::Unavailable => McpErrorKind::Unavailable,
            ToolErrorKind::Protocol => McpErrorKind::Protocol,
            ToolErrorKind::NoSession => McpErrorKind::NoSession,
            ToolErrorKind::Timeout => McpErrorKind::Timeout,
            ToolErrorKind::Refused => McpErrorKind::Refused,
            ToolErrorKind::Failed => McpErrorKind::Failed,
            _ => McpErrorKind::Other,
        }
    }
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
    /// The error's kind, when `is_error`.
    #[uniffi(default = None)]
    pub error: Option<McpErrorKind>,
    /// The error's message, whole: the summary's without "error: ".
    #[uniffi(default = None)]
    pub error_message: Option<String>,
    /// What an engine tool was asked, as given, at most 500 characters:
    /// `search`'s keywords or its query tree in a line (`title:sepsis AND
    /// years:2018–`), identifiers or citations joined with "; ", `identify`'s
    /// title hint, title or first line. `nil` for the app's tools.
    #[uniffi(default = None)]
    pub query: Option<String>,
    /// How many came back: works on a search's page, works looked up,
    /// citations verified, the one work identified. `nil` for errors, the
    /// app's tools and `search_supports`.
    #[uniffi(default = None)]
    pub count: Option<u64>,
    /// The call only estimated: `analyse` with `estimate_only`, which ran
    /// nothing and spent nothing (its `count` is the works it was given).
    /// False for every other call.
    #[uniffi(default = false)]
    pub estimate_only: bool,
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

/// What the app already holds (a reference manager's library), so the
/// engine tools' results can mark it and an agent doesn't propose, add or
/// fetch what the user has. Optional: pass it to `McpServer.start` as
/// `holdings` (it may be the same object as the `McpHost`).
#[uniffi::export(with_foreign)]
#[async_trait::async_trait]
pub trait McpHoldings: Send + Sync {
    /// For each work, given by every identifier its result carries
    /// (`doi:10.1097/…`, lowercase; `pmid:…`, `pmcid:…`, `arxiv:…`,
    /// `openalex:W…`; the canonical first), whether the app holds it under
    /// any of them: one answer per work, in order. Asked once per `search`,
    /// `citing_works` or `references` page and `analyse` result (its
    /// probably missing and related works), which mark each held work
    /// `held: true`. Local: answer from the app's own data. Abandoned
    /// after 5 seconds, when nothing is marked.
    async fn held(&self, works: Vec<Vec<String>>) -> Vec<bool>;
}

/// A bearer key the app minted: aspero sees only its token's hash.
#[derive(Debug, Clone, uniffi::Record)]
pub struct McpKey {
    pub id: String,
    /// Shown in activity: "Claude Code".
    pub name: String,
    /// What it may do, exactly as listed: `"sources"` for aspero's engine
    /// tools (searching, looking up and verifying works, spending from the
    /// daily budget), and the `scope` of each of the app's tools it may
    /// use ("history.read"). No scope implies another. A scope no tool
    /// declares grants nothing; one no tool could declare (see
    /// `HostTool.scope`) fails `start` (`setup`), and leaves the key out
    /// of `setKeys`.
    pub scopes: Vec<String>,
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
            scopes: k.scopes,
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
    /// The UTC day, `YYYY-MM-DD`, that each key's `spentTodayUsd` is of:
    /// keys' daily budgets are per UTC day.
    #[uniffi(default = "")]
    pub spend_day: String,
    /// When that day ends (midnight UTC) and keys' spend starts again from
    /// nothing.
    #[uniffi(default = None)]
    pub spend_resets_at: Option<SystemTime>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct McpKeyStatus {
    pub key_id: String,
    /// Its last request since the server started.
    pub last_used_at: Option<SystemTime>,
    /// Its engine spend today (UTC: `McpStatus.spendDay`), in US dollars,
    /// against its `dailyBudgetUsd`: what its budget is checked against,
    /// kept in the cache file (in memory without one), so it counts spend
    /// before a restart too.
    #[uniffi(default = 0.0)]
    pub spent_today_usd: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum McpError {
    /// Something else listens on the port.
    #[error("port {port} is in use")]
    PortInUse { port: u16 },
    /// An app tool is named like an engine tool, or like another app tool.
    #[error("two tools are named {name:?}")]
    ToolNameClash { name: String },
    /// A key's hash, a budget, a key's or tool's scope is malformed, or
    /// the server couldn't start.
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

/// The app's [`McpHost`] and [`McpHoldings`], either or both, as the
/// server's host.
struct ForeignHost {
    host: Option<Arc<dyn McpHost>>,
    holdings: Option<Arc<dyn McpHoldings>>,
}

#[async_trait::async_trait]
impl http::Host for ForeignHost {
    fn tools(&self) -> Vec<http::HostTool> {
        let Some(host) = &self.host else {
            return Vec::new();
        };
        host.tools()
            .into_iter()
            .map(|t| http::HostTool {
                name: t.name,
                title: t.title,
                description: t.description,
                input_schema: t.input_schema,
                output_schema: t.output_schema,
                scope: t.scope,
                read_only: t.read_only,
                destructive: t.destructive,
                idempotent: t.idempotent,
            })
            .collect()
    }

    async fn call(
        &self,
        name: String,
        arguments: String,
        caller: Caller,
    ) -> Result<http::HostOutput, http::HostError> {
        // Only listed tools are called, and only a host lists any.
        let Some(host) = &self.host else {
            return Err(http::HostError::Failed(format!("no tool named {name:?}")));
        };
        let caller = McpCaller {
            key_id: caller.key_id.unwrap_or_default(),
            key_name: caller.key_name.unwrap_or_default(),
            scopes: caller.scopes,
        };
        match host.call(name, arguments, caller).await {
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
        let Some(host) = &self.host else {
            return;
        };
        host.activity(McpActivity {
            key_name: a.key_name,
            tool: a.tool,
            summary: a.summary,
            is_error: a.is_error,
            cost_usd: a.cost_usd,
            error: a.error.map(Into::into),
            error_message: a.error_message,
            query: a.query,
            count: a.count,
            estimate_only: a.estimate_only,
        });
    }

    async fn held(&self, works: Vec<Vec<String>>) -> Option<Vec<bool>> {
        Some(self.holdings.as_ref()?.held(works).await)
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
    /// disabled sources and cache, where per-key spend is kept. With
    /// `holdings`, the engine tools' results mark the works it holds.
    #[uniffi::constructor(default(host = None, holdings = None))]
    pub async fn start(
        aspero: Arc<Aspero>,
        config: McpServerConfig,
        host: Option<Arc<dyn McpHost>>,
        holdings: Option<Arc<dyn McpHoldings>>,
    ) -> Result<Arc<Self>, McpError> {
        let tools = Arc::new(Tools::new(aspero.engine.clone()));
        let ledger: Arc<dyn Ledger> = match &aspero.cache {
            Some(cache) => cache.clone(),
            None => Arc::new(MemoryLedger::default()),
        };
        let host = (host.is_some() || holdings.is_some())
            .then(|| Arc::new(ForeignHost { host, holdings }) as Arc<dyn http::Host>);
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
    /// 64 hex digits, or that holds a malformed scope, is left out. Returns
    /// without waiting on any agent.
    pub fn set_keys(&self, keys: Vec<McpKey>) {
        self.inner
            .set_keys(keys.into_iter().map(Into::into).collect());
    }

    /// Asks the host for its tools again, and tells every open session its
    /// tools changed (`notifications/tools/list_changed`), without waiting
    /// on any agent. On a clash (or a malformed scope, or a schema that
    /// isn't a JSON object) throws, keeping the tools it had.
    pub fn host_tools_changed(&self) -> Result<(), McpError> {
        Ok(self.inner.host_tools_changed()?)
    }

    pub fn port(&self) -> u16 {
        self.inner.port()
    }

    /// How the server is: its sessions, and each key's last use and spend
    /// today (a read of the cache file per key).
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
                    spent_today_usd: k.spent_today_usd,
                })
                .collect(),
            spend_day: s.spend_day,
            spend_resets_at: Some(SystemTime::from(s.spend_resets_at)),
        }
    }

    /// Ends the sessions and stops listening; the port is free when it
    /// returns. Stopping again finishes a stop cut short (its task
    /// cancelled), and otherwise does nothing.
    pub async fn stop(&self) {
        let inner = self.inner.clone();
        let _ = self
            .aspero
            .run(async move { inner.stop().await }, None)
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UniFFI's defaults must be literals: they say what aspero-mcp's
    /// constants do.
    #[test]
    fn the_defaults_are_aspero_mcps() {
        let source = include_str!("mcp.rs");
        for (default, field) in [
            (
                format!("{:?}", http::DEFAULT_DAILY_BUDGET_USD),
                "daily_budget_usd: f64",
            ),
            (
                format!("{:?}", aspero_mcp::DEFAULT_BUDGET_USD),
                "session_budget_usd: f64",
            ),
            (http::DEFAULT_PORT.to_string(), "port: u16"),
        ] {
            let attribute = format!("#[uniffi(default = {default})]\n    pub {field},");
            assert!(source.contains(&attribute), "{attribute}");
        }
        let limit = format!("at most {} ASCII letters", aspero_mcp::scope::MAX_LEN);
        assert!(source.contains(&limit), "{limit}");
        let held = format!(
            "after {} seconds, when nothing is marked",
            http::HELD_TIMEOUT.as_secs()
        );
        assert!(source.contains(&held), "{held}");
    }

    /// An activity says it only estimated unless set: an app's existing
    /// `McpActivity(…)` calls are unchanged.
    #[test]
    fn estimate_only_is_last_and_off_unless_set() {
        let source = include_str!("mcp.rs");
        let attribute = "#[uniffi(default = false)]\n    pub estimate_only: bool,\n}";
        assert!(source.contains(attribute), "{attribute}");
    }

    struct Shelf;

    #[async_trait::async_trait]
    impl McpHoldings for Shelf {
        async fn held(&self, works: Vec<Vec<String>>) -> Vec<bool> {
            works
                .iter()
                .map(|ids| ids.iter().any(|id| id == "pmid:11"))
                .collect()
        }
    }

    /// The app's holdings answer for the host; without them, the host
    /// says nothing and nothing is marked.
    #[tokio::test]
    async fn holdings_answer_for_the_host_only_when_given() {
        use http::Host as _;
        let works = || {
            vec![
                vec!["doi:10.1/a".into(), "pmid:11".into()],
                vec!["doi:10.1/b".into()],
            ]
        };
        let given = ForeignHost {
            host: None,
            holdings: Some(Arc::new(Shelf)),
        };
        assert_eq!(given.held(works()).await, Some(vec![true, false]));
        assert!(given.tools().is_empty());
        let none = ForeignHost {
            host: None,
            holdings: None,
        };
        assert_eq!(none.held(works()).await, None);
    }

    /// Hints left unset are MCP's own defaults: a tool that says nothing
    /// may change and destroy, and isn't safe to repeat.
    #[test]
    fn unset_hints_are_mcps_defaults() {
        let source = include_str!("mcp.rs");
        for (default, field) in [
            ("false", "read_only"),
            ("true", "destructive"),
            ("false", "idempotent"),
        ] {
            let attribute = format!("#[uniffi(default = {default})]\n    pub {field}: bool,");
            assert!(source.contains(&attribute), "{attribute}");
        }
    }

    /// Every kind aspero-mcp has now is named, none left to `Other`: the
    /// kinds are read from its source, so a new one fails here until it's
    /// mirrored.
    #[test]
    fn every_tool_error_kind_has_its_own_case() {
        let source = include_str!("../../mcp/src/tools.rs");
        let body = source
            .split_once("pub enum ToolErrorKind {")
            .and_then(|(_, rest)| rest.split_once("\n}"))
            .expect("ToolErrorKind in aspero-mcp")
            .0;
        let declared: Vec<&str> = body
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with("//") && !l.is_empty())
            .map(|l| l.trim_end_matches(','))
            .collect();
        let kinds = [
            ToolErrorKind::UnknownTool,
            ToolErrorKind::InvalidArguments,
            ToolErrorKind::Unsupported,
            ToolErrorKind::NotFound,
            ToolErrorKind::BudgetSpent,
            ToolErrorKind::Forbidden,
            ToolErrorKind::Throttled,
            ToolErrorKind::Auth,
            ToolErrorKind::Unavailable,
            ToolErrorKind::Protocol,
            ToolErrorKind::NoSession,
            ToolErrorKind::Timeout,
            ToolErrorKind::Refused,
            ToolErrorKind::Failed,
        ];
        let named: Vec<String> = kinds.iter().map(|k| format!("{k:?}")).collect();
        assert_eq!(declared, named);
        for kind in kinds {
            let mirrored = McpErrorKind::from(kind);
            assert_ne!(mirrored, McpErrorKind::Other, "{kind:?}");
            assert_eq!(format!("{mirrored:?}"), format!("{kind:?}"));
        }
    }
}
