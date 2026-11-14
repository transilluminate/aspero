//! Streamable HTTP on loopback, for apps that embed aspero: an
//! [`HttpServer`] started in-process on a port the app chooses, serving
//! the engine tools and the app's own ([`Host`]) to agents holding the
//! app's bearer keys.
//!
//! - **Transport:** `rmcp`'s Streamable HTTP service at `/mcp`, on
//!   `127.0.0.1` only, under a small hyper server of our own: HTTP/1.1,
//!   sessions (`initialize`, `Mcp-Session-Id`), each with its own budget.
//! - **Every request** passes, in order: the path; `Host` must be
//!   `127.0.0.1:<port>` or `localhost:<port>` (403: DNS rebinding); no
//!   `Origin` (403: browsers send one, agents don't); a bearer token whose
//!   SHA-256 is a key's, compared in constant time (401, no detail); a
//!   session id, if any, that this key opened.
//! - **Scopes:** the engine tools need [`Scope::Sources`], a host tool
//!   its declared scope. `tools/list` shows a key what it may call, from
//!   its scopes and the host's tools at the time; a call without the scope
//!   is a tool error, and the host isn't asked.
//! - **Changes:** when [`HttpServer::set_keys`] changes a key's scopes
//!   (same id, same hash), that key's open sessions are sent
//!   `notifications/tools/list_changed`; after
//!   [`HttpServer::host_tools_changed`], every open session is. They go on
//!   the session's GET stream, or wait for the client to open one.
//! - **Budgets:** per session ([`HttpConfig::session_budget_usd`]), and per
//!   key per UTC day ([`Key::daily_budget_usd`]), engine spend only, kept
//!   in a [`Ledger`] (the cache file) so restarts don't reset it.
//! - **Activity:** after every tool call the host hears who did what and
//!   what it cost, from a thread of its own, in order.

mod auth;
mod handler;
mod host;

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use bytes::Bytes;
use http::header::{ORIGIN, WWW_AUTHENTICATE};
use http::{Method, Request, Response, StatusCode};
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::server::graceful::GracefulShutdown;
use jiff::Timestamp;
use rmcp::RoleServer;
use rmcp::model::Tool;
use rmcp::service::Peer;
use rmcp::transport::streamable_http_server::SessionManager;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub use auth::sha256_hex;
pub use host::{Activity, Host, HostError, HostOutput, HostTool};

use auth::Credential;
use handler::{Authed, Handler};
use host::Listed;

use crate::server::engine_tool;
use crate::{DEFAULT_BUDGET_USD, Scope, Tools};

/// The port unless the app picks another.
pub const DEFAULT_PORT: u16 = 23130;
/// A key's daily budget unless the app sets one, in USD.
pub const DEFAULT_DAILY_BUDGET_USD: f64 = 1.0;
/// How long a host tool may take.
pub const HOST_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a session may take to take a notification before it's given
/// up on.
const NOTIFY_TIMEOUT: Duration = Duration::from_secs(10);
/// The path the protocol is served at.
pub const PATH: &str = "/mcp";

const SESSION_ID: &str = "mcp-session-id";

/// A bearer key the app minted. aspero sees only its token's hash.
#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    /// The app's id for it: per-key spend and use are kept by id.
    pub id: String,
    /// Shown to the user in activity ("Claude Code").
    pub name: String,
    pub scopes: Vec<Scope>,
    /// The SHA-256 of the token, as hex ([`sha256_hex`]).
    pub sha256: String,
    /// Engine spend allowed per UTC day, in USD.
    pub daily_budget_usd: f64,
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct HttpConfig {
    /// On `127.0.0.1`; 0 for any free port (tests). Never moved: a port in
    /// use fails [`HttpServer::start`].
    pub port: u16,
    pub keys: Vec<Key>,
    /// Each session's spend cap, in USD, as [`Session`](crate::Session)'s.
    pub session_budget_usd: f64,
    /// Whether the engine tools are served, or only the host's.
    pub engine_tools: bool,
    /// How long a host tool may take before the call fails.
    pub host_timeout: Duration,
}

impl Default for HttpConfig {
    fn default() -> Self {
        HttpConfig {
            port: DEFAULT_PORT,
            keys: Vec::new(),
            session_budget_usd: DEFAULT_BUDGET_USD,
            engine_tools: true,
            host_timeout: HOST_TIMEOUT,
        }
    }
}

/// Why the server couldn't start, or take the host's tools.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StartError {
    /// Something else listens there; the server never moves.
    PortInUse {
        port: u16,
    },
    /// A host tool is named like an engine tool, or another host tool.
    ToolNameClash {
        name: String,
    },
    Setup(String),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::PortInUse { port } => write!(f, "port {port} is in use"),
            StartError::ToolNameClash { name } => {
                write!(f, "two tools are named {name:?}")
            }
            StartError::Setup(e) => write!(f, "could not start the MCP server: {e}"),
        }
    }
}

impl std::error::Error for StartError {}

/// Where each key's engine spend per UTC day (`YYYY-MM-DD`) is kept.
pub trait Ledger: Send + Sync {
    /// USD spent; an error refuses the key's paid calls.
    fn spent(&self, key_id: &str, day: &str) -> Result<f64, String>;
    fn add(&self, key_id: &str, day: &str, usd: f64) -> Result<(), String>;
}

/// The cache file: spend survives restarts.
impl Ledger for aspero_engine::SqliteCache {
    fn spent(&self, key_id: &str, day: &str) -> Result<f64, String> {
        self.key_spend(key_id, day).map_err(|e| e.to_string())
    }

    fn add(&self, key_id: &str, day: &str, usd: f64) -> Result<(), String> {
        self.add_key_spend(key_id, day, usd)
            .map_err(|e| e.to_string())
    }
}

/// In memory, for an app without a cache: spend lasts as long as the
/// process.
#[derive(Debug, Default)]
pub struct MemoryLedger(Mutex<HashMap<(String, String), f64>>);

impl Ledger for MemoryLedger {
    fn spent(&self, key_id: &str, day: &str) -> Result<f64, String> {
        let map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        Ok(map
            .get(&(key_id.to_owned(), day.to_owned()))
            .copied()
            .unwrap_or(0.0))
    }

    fn add(&self, key_id: &str, day: &str, usd: f64) -> Result<(), String> {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        *map.entry((key_id.to_owned(), day.to_owned())).or_default() += usd;
        Ok(())
    }
}

/// Today, UTC, as `YYYY-MM-DD`.
pub(crate) fn today() -> String {
    Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .date()
        .to_string()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub running: bool,
    pub port: u16,
    /// Sessions open now.
    pub sessions: u32,
    pub last_request_at: Option<Timestamp>,
    pub last_key_name: Option<String>,
    /// Each key's, in the order set.
    pub keys: Vec<KeyStatus>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KeyStatus {
    pub key_id: String,
    /// Its last authenticated request since the server started.
    pub last_used_at: Option<Timestamp>,
}

/// What sessions share: the tools, the host's, budgets, activity.
pub(crate) struct Shared {
    tools: Arc<Tools>,
    engine_tools: bool,
    engine_listed: Vec<Tool>,
    engine_names: Vec<&'static str>,
    host: Option<Arc<dyn Host>>,
    catalog: RwLock<Arc<Vec<Listed>>>,
    ledger: Arc<dyn Ledger>,
    session_budget_usd: f64,
    host_timeout: Duration,
    activity: Mutex<Option<std::sync::mpsc::Sender<Activity>>>,
    /// Each initialized session's line back to its client, for telling it
    /// the tools changed.
    listeners: Mutex<Vec<Listener>>,
}

/// An open session's client, and the key that opened it.
struct Listener {
    key_id: String,
    digest: [u8; 32],
    peer: Peer<RoleServer>,
}

impl Shared {
    fn catalog(&self) -> Arc<Vec<Listed>> {
        self.catalog
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn ledger_add(&self, key_id: &str, usd: f64) {
        if let Err(e) = self.ledger.add(key_id, &today(), usd) {
            tracing::warn!(target: "aspero::mcp", key = key_id, error = %e, "could not record a key's spend");
        }
    }

    /// Keeps a session's client, to tell it when its tools change.
    fn listen(&self, key: &Credential, peer: Peer<RoleServer>) {
        let mut listeners = self.listeners.lock().unwrap_or_else(|e| e.into_inner());
        listeners.retain(|l| !l.peer.is_transport_closed());
        listeners.push(Listener {
            key_id: key.key.id.clone(),
            digest: key.digest,
            peer,
        });
    }

    /// Sends `notifications/tools/list_changed`, on `runtime`, to the open
    /// sessions `which` picks (by key id and hash), without waiting: a slow
    /// or gone client holds up no one, and its errors are ignored.
    fn tools_changed(
        &self,
        runtime: &tokio::runtime::Handle,
        which: impl Fn(&str, &[u8; 32]) -> bool,
    ) {
        let mut listeners = self.listeners.lock().unwrap_or_else(|e| e.into_inner());
        listeners.retain(|l| !l.peer.is_transport_closed());
        for listener in listeners.iter().filter(|l| which(&l.key_id, &l.digest)) {
            let peer = listener.peer.clone();
            runtime.spawn(async move {
                let sent = tokio::time::timeout(NOTIFY_TIMEOUT, peer.notify_tool_list_changed());
                if let Ok(Err(e)) = sent.await {
                    tracing::debug!(target: "aspero::mcp", error = %e, "could not tell a session its tools changed");
                }
            });
        }
    }

    /// Hands `activity` to the host's thread.
    fn report(&self, activity: Activity) {
        if let Some(tx) = &*self.activity.lock().unwrap_or_else(|e| e.into_inner()) {
            let _ = tx.send(activity);
        }
    }
}

/// Who opened a session, so only that key uses it.
struct Owner {
    key_id: String,
    digest: [u8; 32],
}

#[derive(Default)]
struct Usage {
    last_at: Option<Timestamp>,
    last_key_name: Option<String>,
    keys: HashMap<String, Timestamp>,
}

struct State {
    port: u16,
    credentials: RwLock<Arc<Vec<Arc<Credential>>>>,
    shared: Arc<Shared>,
    service: StreamableHttpService<Handler, LocalSessionManager>,
    manager: Arc<LocalSessionManager>,
    owners: Mutex<HashMap<String, Owner>>,
    usage: Mutex<Usage>,
}

type Body = BoxBody<Bytes, Infallible>;

fn plain(status: StatusCode) -> Response<Body> {
    let mut response = Response::new(Full::new(Bytes::new()).boxed());
    *response.status_mut() = status;
    response
}

impl State {
    fn credentials(&self) -> Arc<Vec<Arc<Credential>>> {
        self.credentials
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    async fn handle(&self, mut request: Request<Incoming>) -> Response<Body> {
        if request.uri().path() != PATH {
            return plain(StatusCode::NOT_FOUND);
        }
        if !auth::host_allowed(request.headers(), self.port)
            || request.headers().contains_key(ORIGIN)
        {
            return plain(StatusCode::FORBIDDEN);
        }
        let credentials = self.credentials();
        let Some(credential) = auth::authenticate(&credentials, request.headers()).cloned() else {
            let mut response = plain(StatusCode::UNAUTHORIZED);
            response
                .headers_mut()
                .insert(WWW_AUTHENTICATE, http::HeaderValue::from_static("Bearer"));
            return response;
        };
        let session = request
            .headers()
            .get(SESSION_ID)
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        if let Some(id) = &session {
            let owners = self.owners.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(owner) = owners.get(id)
                && (owner.key_id != credential.key.id || owner.digest != credential.digest)
            {
                return plain(StatusCode::NOT_FOUND);
            }
        }
        self.used(&credential);
        let delete = request.method() == Method::DELETE;
        request.extensions_mut().insert(Authed(credential.clone()));
        let response = self.service.handle(request).await;
        let opened = response
            .headers()
            .get(SESSION_ID)
            .and_then(|v| v.to_str().ok());
        let mut owners = self.owners.lock().unwrap_or_else(|e| e.into_inner());
        match (&session, opened) {
            (None, Some(id)) => {
                owners.insert(
                    id.to_owned(),
                    Owner {
                        key_id: credential.key.id.clone(),
                        digest: credential.digest,
                    },
                );
                // Forget sessions that ended on their own (idle).
                if let Ok(live) = self.manager.sessions.try_read() {
                    owners.retain(|id, _| live.contains_key(id.as_str()));
                }
            }
            (Some(id), _) if delete && response.status().is_success() => {
                owners.remove(id);
            }
            _ => {}
        }
        response.map(|b| b.boxed())
    }

    fn used(&self, credential: &Credential) {
        let now = Timestamp::now();
        let mut usage = self.usage.lock().unwrap_or_else(|e| e.into_inner());
        usage.last_at = Some(now);
        usage.last_key_name = Some(credential.key.name.clone());
        usage.keys.insert(credential.key.id.clone(), now);
    }
}

/// A running server. Stop it with [`stop`](Self::stop); dropping it stops
/// it too, without waiting.
pub struct HttpServer {
    state: Arc<State>,
    cancel: CancellationToken,
    running: AtomicBool,
    task: Mutex<Option<JoinHandle<()>>>,
    /// The runtime serving it, for work begun outside it (`set_keys`).
    runtime: tokio::runtime::Handle,
}

impl HttpServer {
    /// Binds `127.0.0.1:<port>` and serves until stopped: `tools` (unless
    /// `config.engine_tools` is off) and `host`'s tools, its activity
    /// callback after every call, per-key spend in `ledger`. Run within a
    /// Tokio runtime, which serves it.
    pub async fn start(
        tools: Arc<Tools>,
        config: HttpConfig,
        host: Option<Arc<dyn Host>>,
        ledger: Arc<dyn Ledger>,
    ) -> Result<HttpServer, StartError> {
        let budget = config.session_budget_usd;
        if !(budget.is_finite() && budget >= 0.0) {
            return Err(StartError::Setup(format!(
                "the session budget must be zero or more US dollars, not {budget}"
            )));
        }
        let credentials = config
            .keys
            .into_iter()
            .map(|k| Credential::new(k).map(Arc::new))
            .collect::<Result<Vec<_>, _>>()
            .map_err(StartError::Setup)?;
        let engine_names: Vec<&'static str> = tools.specs().iter().map(|s| s.name).collect();
        let catalog = match &host {
            Some(h) => host::catalog(h.as_ref(), &engine_names)?,
            None => Vec::new(),
        };
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, config.port)))
            .await
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::AddrInUse => StartError::PortInUse { port: config.port },
                _ => StartError::Setup(format!("port {}: {e}", config.port)),
            })?;
        let port = listener
            .local_addr()
            .map_err(|e| StartError::Setup(e.to_string()))?
            .port();

        let activity = host.clone().map(|host| {
            let (tx, rx) = std::sync::mpsc::channel::<Activity>();
            let spawned = std::thread::Builder::new()
                .name("aspero-mcp-activity".into())
                .spawn(move || {
                    for a in rx {
                        host.activity(a);
                    }
                });
            if let Err(e) = spawned {
                tracing::warn!(target: "aspero::mcp", error = %e, "no activity thread");
            }
            tx
        });
        let shared = Arc::new(Shared {
            engine_listed: tools.specs().iter().map(engine_tool).collect(),
            engine_names,
            tools,
            engine_tools: config.engine_tools,
            host,
            catalog: RwLock::new(Arc::new(catalog)),
            ledger,
            session_budget_usd: budget,
            host_timeout: config.host_timeout,
            activity: Mutex::new(activity),
            listeners: Mutex::default(),
        });
        let cancel = CancellationToken::new();
        let manager = Arc::new(LocalSessionManager::default());
        let rmcp_config = StreamableHttpServerConfig::default()
            .with_allowed_hosts([format!("127.0.0.1:{port}"), format!("localhost:{port}")])
            // An empty list: every Origin refused (ours refuses first).
            .enforce_origin_validation()
            .with_cancellation_token(cancel.child_token());
        let for_sessions = shared.clone();
        let service = StreamableHttpService::new(
            move || Ok(Handler::new(for_sessions.clone())),
            manager.clone(),
            rmcp_config,
        );
        let state = Arc::new(State {
            port,
            credentials: RwLock::new(Arc::new(credentials)),
            shared,
            service,
            manager,
            owners: Mutex::default(),
            usage: Mutex::default(),
        });
        let task = tokio::spawn(accept(listener, state.clone(), cancel.clone()));
        tracing::info!(target: "aspero::mcp", port, "MCP server listening on 127.0.0.1");
        Ok(HttpServer {
            state,
            cancel,
            running: AtomicBool::new(true),
            task: Mutex::new(Some(task)),
            runtime: tokio::runtime::Handle::current(),
        })
    }

    /// Replaces the keys at once. Sessions opened with a key that's gone,
    /// or whose hash changed, end; their next request is refused (401).
    /// Sessions of a key whose scopes changed (same id, same hash) are sent
    /// `notifications/tools/list_changed`, and list and call by the new
    /// scopes from now on. A key whose hash isn't 64 hex digits is left out
    /// (and logged): it could never match. Doesn't wait on any client.
    pub fn set_keys(&self, keys: Vec<Key>) {
        let credentials: Vec<Arc<Credential>> = keys
            .into_iter()
            .filter_map(|k| match Credential::new(k) {
                Ok(c) => Some(Arc::new(c)),
                Err(e) => {
                    tracing::warn!(target: "aspero::mcp", "{e}; key ignored");
                    None
                }
            })
            .collect();
        // Their owners stay on record (until the sessions are gone), so no
        // other key, nor the same id with a new hash, takes them over
        // before they close.
        let ended: Vec<String> = self
            .state
            .owners
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|(_, o)| {
                !credentials
                    .iter()
                    .any(|c| c.key.id == o.key_id && c.digest == o.digest)
            })
            .map(|(id, _)| id.clone())
            .collect();
        // Kept keys whose scopes changed: their sessions' lists did.
        let rescoped: Vec<(String, [u8; 32])> = {
            let mut current = self
                .state
                .credentials
                .write()
                .unwrap_or_else(|e| e.into_inner());
            let rescoped = credentials
                .iter()
                .filter(|c| {
                    current.iter().any(|o| {
                        o.key.id == c.key.id
                            && o.digest == c.digest
                            && !same_scopes(&o.key.scopes, &c.key.scopes)
                    })
                })
                .map(|c| (c.key.id.clone(), c.digest))
                .collect();
            *current = Arc::new(credentials);
            rescoped
        };
        if !rescoped.is_empty() {
            self.state
                .shared
                .tools_changed(&self.runtime, |id, digest| {
                    rescoped.iter().any(|(i, d)| i == id && d == digest)
                });
        }
        if !ended.is_empty() {
            let manager = self.state.manager.clone();
            self.runtime.spawn(async move {
                for id in ended {
                    let _ = manager.close_session(&id.into()).await;
                }
            });
        }
    }

    /// Asks the host for its tools again, lists them from now on, and
    /// sends every open session `notifications/tools/list_changed`
    /// (without waiting on any client). On a clash or a bad schema, keeps
    /// the tools it had, and tells no one.
    pub fn host_tools_changed(&self) -> Result<(), StartError> {
        let shared = &self.state.shared;
        let Some(host) = &shared.host else {
            return Ok(());
        };
        let catalog = host::catalog(host.as_ref(), &shared.engine_names)?;
        *shared.catalog.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(catalog);
        shared.tools_changed(&self.runtime, |_, _| true);
        Ok(())
    }

    pub fn port(&self) -> u16 {
        self.state.port
    }

    pub fn status(&self) -> Status {
        let state = &self.state;
        let usage = state.usage.lock().unwrap_or_else(|e| e.into_inner());
        let sessions = match state.manager.sessions.try_read() {
            Ok(live) => live.len(),
            Err(_) => state.owners.lock().unwrap_or_else(|e| e.into_inner()).len(),
        };
        Status {
            running: self.running.load(Ordering::Relaxed),
            port: state.port,
            sessions: u32::try_from(sessions).unwrap_or(u32::MAX),
            last_request_at: usage.last_at,
            last_key_name: usage.last_key_name.clone(),
            keys: state
                .credentials()
                .iter()
                .map(|c| KeyStatus {
                    key_id: c.key.id.clone(),
                    last_used_at: usage.keys.get(&c.key.id).copied(),
                })
                .collect(),
        }
    }

    /// Ends every session, stops listening and waits (a couple of seconds
    /// at most) for requests in flight; the port is free when it returns.
    /// Stopping again does nothing.
    pub async fn stop(&self) {
        if !self.running.swap(false, Ordering::Relaxed) {
            return;
        }
        let manager = &self.state.manager;
        let ids: Vec<_> = manager.sessions.read().await.keys().cloned().collect();
        for id in ids {
            let _ = manager.close_session(&id).await;
        }
        self.cancel.cancel();
        let task = self.task.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(task) = task {
            let _ = task.await;
        }
        // Ends the activity thread once it has said what's queued.
        self.state
            .shared
            .activity
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        tracing::info!(target: "aspero::mcp", port = self.state.port, "MCP server stopped");
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// Whether two keys may do the same things, in whatever order.
fn same_scopes(a: &[Scope], b: &[Scope]) -> bool {
    a.iter().all(|s| b.contains(s)) && b.iter().all(|s| a.contains(s))
}

/// Accepts connections until cancelled, then lets requests in flight
/// finish (for up to two seconds) after closing the listener.
async fn accept(listener: TcpListener, state: Arc<State>, cancel: CancellationToken) {
    let graceful = GracefulShutdown::new();
    loop {
        tokio::select! {
            () = cancel.cancelled() => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    let state = state.clone();
                    let service = service_fn(move |request| {
                        let state = state.clone();
                        async move { Ok::<_, Infallible>(state.handle(request).await) }
                    });
                    let connection = http1::Builder::new()
                        .timer(TokioTimer::new())
                        .serve_connection(TokioIo::new(stream), service);
                    let connection = graceful.watch(connection);
                    tokio::spawn(async move {
                        let _ = connection.await;
                    });
                }
                Err(e) => {
                    // Out of file descriptors, say: wait, then go on.
                    tracing::warn!(target: "aspero::mcp", error = %e, "accept failed");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        }
    }
    drop(listener);
    let _ = tokio::time::timeout(Duration::from_secs(2), graceful.shutdown()).await;
}
