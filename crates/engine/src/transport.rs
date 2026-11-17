//! The engine's side of the [`HttpClient`] seam: a real client, pacing,
//! and the HTTP tape that benchmarks replay.

use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use aspero_core::http::Method;
use aspero_core::{Error, HttpClient, HttpRequest, HttpResponse};
use aspero_store::{Pacing, SqliteTape, StoreError, TapeKey};
use async_trait::async_trait;
use jiff::Timestamp;
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::detach;
use crate::observe::{self, Usd};

/// `HttpClient` over `reqwest` (rustls). One per provider, so each gets its
/// own attempt timeout and connection pool.
pub struct ReqwestClient {
    client: reqwest::Client,
}

impl ReqwestClient {
    pub fn new(user_agent: &str, attempt_timeout: Duration) -> Result<Self, Error> {
        let client = reqwest::Client::builder()
            .user_agent(user_agent)
            .timeout(attempt_timeout)
            .build()
            .map_err(Error::network)?;
        Ok(ReqwestClient { client })
    }
}

#[async_trait]
impl HttpClient for ReqwestClient {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, Error> {
        let method = match request.method {
            Method::Post => reqwest::Method::POST,
            _ => reqwest::Method::GET,
        };
        let mut builder = self.client.request(method, &request.url);
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        if let Some(body) = request.body {
            builder = builder.body(body);
        }
        let resp = builder.send().await.map_err(transport_error)?;
        let status = resp.status().as_u16();
        let headers = resp
            .headers()
            .iter()
            .filter_map(|(k, v)| Some((k.as_str().to_owned(), v.to_str().ok()?.to_owned())))
            .collect();
        let body = resp.bytes().await.map_err(transport_error)?.to_vec();
        Ok(HttpResponse::new(status, headers, body, Timestamp::now()))
    }
}

fn transport_error(e: reqwest::Error) -> Error {
    if e.is_timeout() {
        Error::Timeout
    } else {
        Error::network(e)
    }
}

/// Spaces request *starts* at least `min_interval` apart, across all tasks
/// sharing it, and optionally limits requests in flight. Slots are handed
/// out in arrival order, so waiting is fair.
///
/// With [`shared`](Paced::shared) pacing, slots come from a store that
/// every process using it shares (the cache file), so parallel processes
/// keep to one rate between them; if the store fails, pacing falls back to
/// this process alone.
///
/// A request of a paid call is marked sent once its slot has come, just
/// before it's sent; one whose caller has gone by then isn't sent (see
/// `detach`).
///
/// Emits a `request` event (target `aspero::net`, DESIGN "Diagnostics
/// events") as each request completes, or is dropped once sent: the
/// provider label, what it was for and which attempt, its status, its
/// time and its wait for a slot, and its cost; its URL only at `debug`
/// (`request_url`).
pub struct Paced {
    label: String,
    inner: Arc<dyn HttpClient>,
    min_interval: Duration,
    next_slot: Mutex<Option<Instant>>,
    shared: Option<Shared>,
    in_flight: Option<Arc<tokio::sync::Semaphore>>,
}

struct Shared {
    pacing: Arc<dyn Pacing>,
    /// Wall-clock time, which processes share; swappable for tests.
    clock: fn() -> Timestamp,
}

impl Paced {
    pub fn new(
        label: impl Into<String>,
        inner: Arc<dyn HttpClient>,
        min_interval: Duration,
    ) -> Self {
        Paced {
            label: label.into(),
            inner,
            min_interval,
            next_slot: Mutex::new(None),
            shared: None,
            in_flight: None,
        }
    }

    /// Keeps at most `n` requests in flight at once (0: no limit), for
    /// providers that refuse concurrent requests rather than fast ones.
    pub fn max_in_flight(mut self, n: usize) -> Self {
        self.in_flight = (n > 0).then(|| Arc::new(tokio::sync::Semaphore::new(n)));
        self
    }

    /// Paces with every other process using `pacing`.
    pub fn shared(self, pacing: Arc<dyn Pacing>) -> Self {
        self.shared_by_clock(pacing, Timestamp::now)
    }

    fn shared_by_clock(mut self, pacing: Arc<dyn Pacing>, clock: fn() -> Timestamp) -> Self {
        self.shared = Some(Shared { pacing, clock });
        self
    }

    /// Claims the next slot and returns when it starts.
    async fn claim(&self) -> Instant {
        let mut next = self.next_slot.lock().await;
        let now = Instant::now();
        let mut slot = next.map_or(now, |n| n.max(now));
        if let Some(shared) = &self.shared {
            let wall = (shared.clock)();
            match shared
                .pacing
                .claim_slot(&self.label, self.min_interval, wall)
            {
                Ok(at) => {
                    let wait = Duration::try_from(at.duration_since(wall)).unwrap_or_default();
                    slot = slot.max(now + wait);
                }
                Err(e) => tracing::warn!(
                    target: "aspero::net",
                    provider = %self.label, error = %e, "shared pacing failed; pacing this process alone"
                ),
            }
        }
        *next = Some(slot + self.min_interval);
        slot
    }
}

#[async_trait]
impl HttpClient for Paced {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, Error> {
        let queued = Instant::now();
        let _permit = match &self.in_flight {
            Some(limit) => Some(limit.acquire().await.expect("never closed")),
            None => None,
        };
        let slot = self.claim().await;
        tokio::time::sleep_until(slot).await;
        if !crate::detach::may_send() {
            // A paid call whose caller has gone: it's being cancelled, and
            // this is never sent.
            std::future::pending::<()>().await;
        }
        let mut event = RequestEvent {
            provider: &self.label,
            started: Instant::now(),
            waited_ms: observe::ms(queued, Instant::now()),
            url: tracing::enabled!(target: "aspero::net", tracing::Level::DEBUG)
                .then(|| loggable_url(&request.url)),
            done: false,
        };
        let result = self.inner.send(request).await;
        match &result {
            Ok(resp) => event.emit(&resp.status, cost(resp)),
            Err(e) => event.emit(&Failed(e), None),
        }
        result
    }
}

/// A request sent, until its event is emitted: as it completes, or, if
/// it's dropped first (a deadline passed, or its caller stopped waiting),
/// as `cancelled`.
struct RequestEvent<'a> {
    provider: &'a str,
    started: Instant,
    waited_ms: u64,
    /// For the `debug` companion, only when that's on.
    url: Option<String>,
    done: bool,
}

impl RequestEvent<'_> {
    fn emit(&mut self, status: &dyn std::fmt::Display, cost: Option<f64>) {
        self.done = true;
        let (op, attempt) = observe::request();
        let ms = observe::ms(self.started, Instant::now());
        tracing::info!(
            target: "aspero::net",
            provider = %self.provider, op = %op, status = %status, ms, waited_ms = self.waited_ms,
            attempt, cache = %"miss", cost_usd = cost.map(|c| tracing::field::display(Usd(c))),
            detached = detach::detached().then_some(true), "request"
        );
        if let Some(url) = &self.url {
            tracing::debug!(
                target: "aspero::net",
                provider = %self.provider, op = %op, attempt, url = %url, "request_url"
            );
        }
    }
}

impl Drop for RequestEvent<'_> {
    fn drop(&mut self) {
        if !self.done {
            self.emit(&"cancelled", None);
        }
    }
}

/// A response's cost, where it says one (OpenAlex's
/// `x-ratelimit-cost-usd`).
fn cost(resp: &HttpResponse) -> Option<f64> {
    resp.header("x-ratelimit-cost-usd")?.trim().parse().ok()
}

/// A failed request's `status`, worked out only when it's logged.
struct Failed<'a>(&'a Error);

impl std::fmt::Display for Failed<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(failure(self.0))
    }
}

/// A failed request's `status`: what went wrong, by kind (`timeout`,
/// `dns`, `refused`, `reset`, `connect`, `body`, `network`), or the
/// error's kind when it isn't the network's.
fn failure(e: &Error) -> &'static str {
    let Error::Network(source) = e else {
        return e.kind().as_str();
    };
    let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(source.as_ref());
    let mut connect = false;
    while let Some(err) = cause {
        if let Some(r) = err.downcast_ref::<reqwest::Error>() {
            if r.is_timeout() {
                return "timeout";
            }
            connect |= r.is_connect();
            if r.is_body() || r.is_decode() {
                return "body";
            }
        }
        if let Some(io) = err.downcast_ref::<std::io::Error>() {
            match io.kind() {
                std::io::ErrorKind::ConnectionRefused => return "refused",
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted => {
                    return "reset";
                }
                std::io::ErrorKind::TimedOut => return "timeout",
                _ => {}
            }
        }
        // hyper-util's resolver failure says so only in words.
        if err.to_string().starts_with("dns error") {
            return "dns";
        }
        cause = err.source();
    }
    if connect { "connect" } else { "network" }
}

/// The tape's file: recorded and replayed (a developer tool, for benchmark
/// and red-team runs; not in `--help`). Empty or `off`: no tape.
pub const TAPE_ENV: &str = "ASPERO_HTTP_TAPE";

/// `replay-only` (a request not on the tape fails, unsent: fully offline,
/// reproducible runs) or `record` (the default: replay what's there, send
/// and record the rest).
pub const TAPE_MODE_ENV: &str = "ASPERO_HTTP_TAPE_MODE";

/// Whether a tape may send what it hasn't recorded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TapeMode {
    /// Replays what's recorded; sends the rest, recording deterministic
    /// answers.
    #[default]
    Record,
    /// Replays only: a request not on the tape fails with
    /// [`Error::Offline`], unsent.
    ReplayOnly,
}

/// Why a tape couldn't be set up.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TapeError {
    #[error("{TAPE_MODE_ENV}: {0:?} is neither `record` nor `replay-only`")]
    Mode(String),
    #[error("{TAPE_MODE_ENV}=replay-only needs a tape: set {TAPE_ENV}")]
    NoTape,
    #[error("{TAPE_ENV}: {source} ({path})")]
    Open { path: String, source: StoreError },
}

/// An HTTP tape for [`StandardConfig::http_tape`](crate::StandardConfig::http_tape):
/// the file, and whether misses may be sent.
#[derive(Clone)]
#[non_exhaustive]
pub struct HttpTape {
    pub tape: Arc<SqliteTape>,
    pub mode: TapeMode,
}

impl HttpTape {
    pub fn new(tape: Arc<SqliteTape>, mode: TapeMode) -> Self {
        HttpTape { tape, mode }
    }

    /// The tape [`TAPE_ENV`] and [`TAPE_MODE_ENV`] say, if any.
    pub fn from_env() -> Result<Option<Self>, TapeError> {
        let var = |name: &str| std::env::var(name).ok();
        Self::from_vars(var(TAPE_ENV).as_deref(), var(TAPE_MODE_ENV).as_deref())
    }

    /// The tape at `path` (empty or `off`: none) in `mode` (as
    /// [`TAPE_MODE_ENV`] gives it).
    pub fn from_vars(path: Option<&str>, mode: Option<&str>) -> Result<Option<Self>, TapeError> {
        let mode = match mode.map(str::trim).filter(|m| !m.is_empty()) {
            None | Some("record") => TapeMode::Record,
            Some("replay-only") => TapeMode::ReplayOnly,
            Some(other) => return Err(TapeError::Mode(other.to_owned())),
        };
        let Some(path) = path.map(str::trim).filter(|p| !p.is_empty() && *p != "off") else {
            return match mode {
                TapeMode::ReplayOnly => Err(TapeError::NoTape),
                TapeMode::Record => Ok(None),
            };
        };
        let tape = SqliteTape::open(Path::new(path)).map_err(|source| TapeError::Open {
            path: path.to_owned(),
            source,
        })?;
        Ok(Some(HttpTape::new(Arc::new(tape), mode)))
    }
}

/// Replays responses recorded on an [`HttpTape`], and records the rest as
/// they're sent, so each distinct request is paid for once, ever.
///
/// It wraps the paced client, so replays are instant: no pacing slot, no
/// in-flight limit. What's recorded is what's deterministic data: 2xx
/// answers, and 404s (a meaningful "not found"), never a 404 that says a
/// budget is spent, a 429, a 5xx, another error, or a transport failure.
/// Live state is never taped: OpenAlex's `/rate-limit`, whose answer is
/// today's budget.
///
/// A replay costs nothing and says nothing of a budget: its usage headers
/// (`x-ratelimit-*`, as recorded) are dropped and a recorded cost reads 0,
/// so it neither counts towards a spend nor moves a daily cap. It's
/// received when it was recorded. A miss sent counts as usual.
///
/// An engine behind one ([`EngineBuilder::taped`](crate::EngineBuilder::taped))
/// looks identifiers up one per request, so each one's answer replays
/// however batches are made up; it asks a batch `replaying_only` first,
/// so batches recorded whole still replay.
pub struct TapeClient {
    label: String,
    inner: Arc<dyn HttpClient>,
    tape: Arc<SqliteTape>,
    mode: TapeMode,
    secrets: Vec<String>,
}

impl TapeClient {
    pub fn new(label: impl Into<String>, inner: Arc<dyn HttpClient>, tape: &HttpTape) -> Self {
        TapeClient {
            label: label.into(),
            inner,
            tape: tape.tape.clone(),
            mode: tape.mode,
            secrets: Vec::new(),
        }
    }

    /// Also keeps `secrets` off the tape (redacted where a response echoes
    /// them): a contact address sent only in the user agent.
    pub fn scrub(mut self, secrets: impl IntoIterator<Item = String>) -> Self {
        self.secrets.extend(secrets);
        self
    }

    async fn untaped(&self, request: HttpRequest) -> Result<HttpResponse, Error> {
        match self.mode {
            TapeMode::Record => self.inner.send(request).await,
            TapeMode::ReplayOnly => Err(Error::Offline),
        }
    }
}

#[async_trait]
impl HttpClient for TapeClient {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, Error> {
        // Asked whether a whole batch is on the tape: nothing is sent.
        let peeking = peeking();
        let Some(key) = TapeKey::new(&request).filter(|_| !is_live(&request.url)) else {
            if peeking {
                return Err(Error::Offline);
            }
            return self.untaped(request).await;
        };
        let key = key.with_secrets(self.secrets.iter().cloned());
        let url = loggable_url(key.url());
        match self.tape.get(&key) {
            Ok(Some(recorded)) => {
                let replayed = replayed(recorded);
                let (op, attempt) = observe::request();
                tracing::info!(
                    target: "aspero::net",
                    provider = %self.label, op = %op, status = replayed.status, ms = 0u64,
                    waited_ms = 0u64, attempt, cache = %"replayed",
                    cost_usd = cost(&replayed).map(|c| tracing::field::display(Usd(c))),
                    "request"
                );
                tracing::debug!(
                    target: "aspero::net",
                    provider = %self.label, op = %op, attempt, url = %url, "request_url"
                );
                return Ok(replayed);
            }
            Ok(None) => {}
            Err(e) => tracing::warn!(
                target: "aspero::tape",
                provider = %self.label, error = %e, "tape unreadable; sending {url}"
            ),
        }
        if peeking {
            tracing::debug!(
                target: "aspero::tape",
                provider = %self.label, "batch not on tape: {url}"
            );
            return Err(Error::Offline);
        }
        if self.mode == TapeMode::ReplayOnly {
            tracing::warn!(
                target: "aspero::tape",
                provider = %self.label, "not on tape: {url}"
            );
            return Err(Error::Offline);
        }
        let response = self.inner.send(request).await?;
        if recordable(&response)
            && let Err(e) = self.tape.put(&key, &response)
        {
            tracing::warn!(
                target: "aspero::tape",
                provider = %self.label, error = %e, "couldn't record {url}"
            );
        }
        Ok(response)
    }
}

tokio::task_local! {
    /// Set while the engine asks whether a whole batch is on the tape.
    static REPLAYING_ONLY: ();
}

/// Runs `f` with every [`TapeClient`] replay-only, whatever its mode: what
/// it asks that isn't on the tape fails as [`Error::Offline`], unsent. The
/// engine asks a batch this way under a tape, so a batch recorded whole
/// still replays while one that isn't is asked one ID at a time. Requests
/// that don't go through a tape are sent as usual.
pub(crate) async fn replaying_only<F: Future>(f: F) -> F::Output {
    REPLAYING_ONLY.scope((), f).await
}

/// Whether this task is asking only what's on the tape
/// ([`replaying_only`]), so sends nothing.
pub(crate) fn peeking() -> bool {
    REPLAYING_ONLY.try_with(|()| ()).is_ok()
}

/// Whether `url` asks for live state rather than data: OpenAlex's
/// `/rate-limit`, today's budget.
fn is_live(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    path.trim_end_matches('/').ends_with("/rate-limit")
}

/// Whether a response is deterministic data, worth replaying: a 2xx, or a
/// 404 that isn't a spent budget (OpenAlex answers the request crossing
/// its budget with an error status, and says so in its body or headers).
/// Past the daily allowance, prepaid credit pays: the budget is spent only
/// when that is too.
fn recordable(resp: &HttpResponse) -> bool {
    match resp.status {
        200..=299 => true,
        404 => {
            let spent = |name: &str| {
                resp.header(name)
                    .and_then(|v| v.trim().parse::<f64>().ok())
                    .map(|r| r <= 0.0)
            };
            let spent = spent("x-ratelimit-remaining-usd") == Some(true)
                && spent("x-ratelimit-prepaid-remaining-usd") != Some(false);
            !spent && !resp.text().contains("not_enough_credits")
        }
        _ => false,
    }
}

/// A recorded response as replayed: without the budget it reported then,
/// and costing nothing now.
fn replayed(recorded: HttpResponse) -> HttpResponse {
    let usage = |name: &str| {
        ["x-ratelimit-", "x-rate-limit-", "ratelimit-"]
            .iter()
            .any(|p| name.starts_with(p))
    };
    let charged = recorded.header("x-ratelimit-cost-usd").is_some();
    let mut headers: Vec<(String, String)> = recorded
        .headers
        .into_iter()
        .filter(|(name, _)| !usage(name))
        .collect();
    if charged {
        headers.push(("x-ratelimit-cost-usd".into(), "0".into()));
    }
    HttpResponse::new(
        recorded.status,
        headers,
        recorded.body,
        recorded.received_at,
    )
}

/// A URL for logs: path and query only, contact and key parameters
/// redacted (logs get pasted into issues), long queries shortened.
fn loggable_url(url: &str) -> String {
    const MAX: usize = 140;
    let path = url
        .split_once("://")
        .map_or(url, |(_, rest)| rest.find('/').map_or("/", |i| &rest[i..]));
    let redacted: String = match path.split_once('?') {
        None => path.to_owned(),
        Some((p, query)) => {
            let params: Vec<String> = query
                .split('&')
                .map(|kv| match kv.split_once('=') {
                    Some((k, _)) if matches!(k, "mailto" | "api_key" | "key") => format!("{k}=…"),
                    _ => kv.to_owned(),
                })
                .collect();
            format!("{p}?{}", params.join("&"))
        }
    };
    match redacted.char_indices().nth(MAX) {
        Some((cut, _)) => format!("{}…", &redacted[..cut]),
        None => redacted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo;

    #[async_trait]
    impl HttpClient for Echo {
        async fn send(&self, _: HttpRequest) -> Result<HttpResponse, Error> {
            Ok(HttpResponse::new(
                200,
                vec![],
                vec![],
                Timestamp::UNIX_EPOCH,
            ))
        }
    }

    #[tokio::test(start_paused = true)]
    async fn paced_requests_start_min_interval_apart() {
        let paced = Arc::new(Paced::new(
            "test",
            Arc::new(Echo),
            Duration::from_millis(250),
        ));
        let start = Instant::now();
        let mut tasks = Vec::new();
        for _ in 0..4 {
            let paced = paced.clone();
            tasks.push(tokio::spawn(async move {
                paced.send(HttpRequest::get("http://x")).await.unwrap();
                start.elapsed().as_millis()
            }));
        }
        let mut starts = Vec::new();
        for t in tasks {
            starts.push(t.await.unwrap());
        }
        starts.sort_unstable();
        assert_eq!(starts, [0, 250, 500, 750]);
    }

    #[test]
    fn logged_urls_hide_contacts_and_keys() {
        assert_eq!(
            loggable_url("https://api.crossref.org/works?query=x&mailto=me%40example.org&rows=3"),
            "/works?query=x&mailto=…&rows=3"
        );
        assert_eq!(
            loggable_url("https://doi.org/10.1038/nature14539"),
            "/10.1038/nature14539"
        );
    }

    thread_local! {
        static START: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
    }

    /// Wall-clock time that follows tokio's paused clock.
    fn paused_clock() -> Timestamp {
        let start = START.with(|s| {
            let start = s.get().unwrap_or_else(Instant::now);
            s.set(Some(start));
            start
        });
        let elapsed = jiff::SignedDuration::try_from(start.elapsed()).unwrap();
        Timestamp::UNIX_EPOCH + elapsed
    }

    #[tokio::test(start_paused = true)]
    async fn processes_sharing_pacing_keep_to_one_rate() {
        paused_clock();
        let store = Arc::new(aspero_store::SqliteCache::open_in_memory().unwrap());
        let process = || {
            Arc::new(
                Paced::new("test", Arc::new(Echo), Duration::from_millis(250))
                    .shared_by_clock(store.clone(), paused_clock),
            )
        };
        let (a, b) = (process(), process());
        let start = Instant::now();
        let mut tasks = Vec::new();
        for paced in [&a, &b, &a, &b] {
            let paced = paced.clone();
            tasks.push(tokio::spawn(async move {
                paced.send(HttpRequest::get("http://x")).await.unwrap();
                start.elapsed().as_millis()
            }));
        }
        let mut starts = Vec::new();
        for t in tasks {
            starts.push(t.await.unwrap());
        }
        starts.sort_unstable();
        assert_eq!(starts, [0, 250, 500, 750]);
    }

    #[tokio::test(start_paused = true)]
    async fn idle_time_is_not_banked() {
        let paced = Paced::new("test", Arc::new(Echo), Duration::from_millis(100));
        paced.send(HttpRequest::get("http://x")).await.unwrap();
        tokio::time::sleep(Duration::from_secs(5)).await;
        let start = Instant::now();
        paced.send(HttpRequest::get("http://x")).await.unwrap();
        paced.send(HttpRequest::get("http://x")).await.unwrap();
        assert_eq!(start.elapsed().as_millis(), 100);
    }

    /// Answers every request with `status` (OpenAlex-style usage headers),
    /// counting them.
    struct Counting {
        status: u16,
        sent: std::sync::atomic::AtomicUsize,
    }

    impl Counting {
        fn new(status: u16) -> Arc<Self> {
            Arc::new(Counting {
                status,
                sent: std::sync::atomic::AtomicUsize::new(0),
            })
        }

        fn sent(&self) -> usize {
            self.sent.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl HttpClient for Counting {
        async fn send(&self, _: HttpRequest) -> Result<HttpResponse, Error> {
            self.sent.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(HttpResponse::new(
                self.status,
                vec![
                    ("x-ratelimit-cost-usd".into(), "0.001".into()),
                    ("x-ratelimit-remaining-usd".into(), "0.5".into()),
                    ("x-ratelimit-limit-usd".into(), "1".into()),
                    ("content-type".into(), "application/json".into()),
                ],
                b"{}".to_vec(),
                Timestamp::UNIX_EPOCH,
            ))
        }
    }

    fn taped(inner: &Arc<Counting>, mode: TapeMode) -> (TapeClient, HttpTape) {
        let tape = HttpTape::new(Arc::new(SqliteTape::open_in_memory().unwrap()), mode);
        (TapeClient::new("test", inner.clone(), &tape), tape)
    }

    fn get(url: &str) -> HttpRequest {
        HttpRequest::get(url).header("accept", "application/json")
    }

    #[tokio::test]
    async fn a_replay_costs_nothing_and_reports_no_budget() {
        let inner = Counting::new(200);
        let (client, _) = taped(&inner, TapeMode::Record);
        let sent = client
            .send(get("https://api.openalex.org/works?search=x"))
            .await
            .unwrap();
        assert_eq!(
            sent.header("x-ratelimit-cost-usd"),
            Some("0.001"),
            "a miss counts"
        );
        let again = client
            .send(get("https://api.openalex.org/works?search=x"))
            .await
            .unwrap();
        assert_eq!(inner.sent(), 1, "replayed");
        assert_eq!(again.body, sent.body);
        assert_eq!(
            again.received_at, sent.received_at,
            "received when recorded"
        );
        assert_eq!(again.header("x-ratelimit-cost-usd"), Some("0"));
        assert_eq!(again.header("x-ratelimit-remaining-usd"), None);
        assert_eq!(again.header("x-ratelimit-limit-usd"), None);
        assert_eq!(again.header("content-type"), Some("application/json"));
    }

    #[tokio::test]
    async fn only_deterministic_answers_are_recorded() {
        for (status, kept) in [
            (200, true),
            (404, true),
            (400, false),
            (429, false),
            (500, false),
            (503, false),
        ] {
            let inner = Counting::new(status);
            let (client, tape) = taped(&inner, TapeMode::Record);
            for _ in 0..2 {
                let resp = client
                    .send(get("https://api.crossref.org/works/10.1/x"))
                    .await;
                assert_eq!(resp.unwrap().status, status);
            }
            assert_eq!(tape.tape.entries().unwrap(), u64::from(kept), "{status}");
            assert_eq!(inner.sent(), if kept { 1 } else { 2 }, "{status}");
        }

        // A 404 crossing OpenAlex's budget is a spent budget, not "not found".
        let spent = HttpResponse::new(
            404,
            vec![("x-ratelimit-remaining-usd".into(), "0".into())],
            Vec::new(),
            Timestamp::UNIX_EPOCH,
        );
        assert!(!recordable(&spent));
        let credits = HttpResponse::new(
            404,
            vec![],
            b"not_enough_credits".to_vec(),
            Timestamp::UNIX_EPOCH,
        );
        assert!(!recordable(&credits));
        // Past the daily allowance, on prepaid credit, a 404 is "not
        // found": the round-3 red team's misses went unrecorded so.
        let prepaid = HttpResponse::new(
            404,
            vec![
                ("x-ratelimit-remaining-usd".into(), "0".into()),
                ("x-ratelimit-prepaid-remaining-usd".into(), "8.8513".into()),
            ],
            Vec::new(),
            Timestamp::UNIX_EPOCH,
        );
        assert!(recordable(&prepaid));
        let both_spent = HttpResponse::new(
            404,
            vec![
                ("x-ratelimit-remaining-usd".into(), "0".into()),
                ("x-ratelimit-prepaid-remaining-usd".into(), "0".into()),
            ],
            Vec::new(),
            Timestamp::UNIX_EPOCH,
        );
        assert!(!recordable(&both_spent));
    }

    #[tokio::test]
    async fn transport_failures_are_not_recorded() {
        struct Down;
        #[async_trait]
        impl HttpClient for Down {
            async fn send(&self, _: HttpRequest) -> Result<HttpResponse, Error> {
                Err(Error::Timeout)
            }
        }
        let tape = HttpTape::new(
            Arc::new(SqliteTape::open_in_memory().unwrap()),
            TapeMode::Record,
        );
        let client = TapeClient::new("test", Arc::new(Down), &tape);
        assert!(matches!(
            client.send(get("https://doi.org/10.1/x")).await,
            Err(Error::Timeout)
        ));
        assert_eq!(tape.tape.entries().unwrap(), 0);
    }

    #[tokio::test]
    async fn replay_only_fails_a_miss_unsent() {
        let inner = Counting::new(200);
        let (client, tape) = taped(&inner, TapeMode::Record);
        client.send(get("https://doi.org/10.1/x")).await.unwrap();
        let replaying = TapeClient::new(
            "test",
            inner.clone(),
            &HttpTape::new(tape.tape, TapeMode::ReplayOnly),
        );
        assert_eq!(
            replaying
                .send(get("https://doi.org/10.1/x"))
                .await
                .unwrap()
                .status,
            200
        );
        let miss = replaying.send(get("https://doi.org/10.1/y")).await;
        assert!(matches!(miss, Err(Error::Offline)), "{miss:?}");
        assert_eq!(inner.sent(), 1);
    }

    #[tokio::test]
    async fn asked_replay_only_a_recording_tape_sends_nothing() {
        let inner = Counting::new(200);
        let (client, _) = taped(&inner, TapeMode::Record);
        client.send(get("https://doi.org/10.1/x")).await.unwrap();
        let (hit, miss, live) = replaying_only(async {
            (
                client.send(get("https://doi.org/10.1/x")).await,
                client.send(get("https://doi.org/10.1/y")).await,
                client
                    .send(get("https://api.openalex.org/rate-limit"))
                    .await,
            )
        })
        .await;
        assert_eq!(hit.unwrap().status, 200);
        assert!(matches!(miss, Err(Error::Offline)), "{miss:?}");
        assert!(matches!(live, Err(Error::Offline)), "{live:?}");
        assert_eq!(inner.sent(), 1);
        // Outside it, a miss is sent and recorded as usual.
        client.send(get("https://doi.org/10.1/y")).await.unwrap();
        assert_eq!(inner.sent(), 2);
    }

    #[tokio::test]
    async fn live_budget_asks_are_never_taped() {
        let inner = Counting::new(200);
        let (client, tape) = taped(&inner, TapeMode::Record);
        for _ in 0..2 {
            client
                .send(get("https://api.openalex.org/rate-limit"))
                .await
                .unwrap();
        }
        assert_eq!(inner.sent(), 2);
        assert_eq!(tape.tape.entries().unwrap(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn replays_skip_pacing() {
        let inner = Counting::new(200);
        let tape = HttpTape::new(
            Arc::new(SqliteTape::open_in_memory().unwrap()),
            TapeMode::Record,
        );
        let paced = Arc::new(Paced::new("test", inner.clone(), Duration::from_secs(1)));
        let client = TapeClient::new("test", paced, &tape);
        client.send(get("https://doi.org/10.1/x")).await.unwrap();
        let start = Instant::now();
        for _ in 0..5 {
            client.send(get("https://doi.org/10.1/x")).await.unwrap();
        }
        assert_eq!(start.elapsed().as_millis(), 0);
        assert_eq!(inner.sent(), 1);
    }

    #[test]
    fn tape_settings_read_from_the_environment() {
        assert!(HttpTape::from_vars(None, None).unwrap().is_none());
        assert!(
            HttpTape::from_vars(Some(" off "), Some("record"))
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            HttpTape::from_vars(None, Some("replay-only")),
            Err(TapeError::NoTape)
        ));
        assert!(matches!(
            HttpTape::from_vars(None, Some("replay")),
            Err(TapeError::Mode(_))
        ));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t/tape.sqlite");
        let tape = HttpTape::from_vars(path.to_str(), Some("replay-only"))
            .unwrap()
            .unwrap();
        assert_eq!(tape.mode, TapeMode::ReplayOnly);
        assert!(path.exists());
    }
}
