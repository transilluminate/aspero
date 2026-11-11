//! The engine's side of the [`HttpClient`] seam: a real client, and pacing.

use std::sync::Arc;
use std::time::Duration;

use aspero_core::http::Method;
use aspero_core::{Error, HttpClient, HttpRequest, HttpResponse};
use aspero_store::Pacing;
use async_trait::async_trait;
use jiff::Timestamp;
use tokio::sync::Mutex;
use tokio::time::Instant;

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
/// Logs each request (at `info`, target `aspero::http`) with the provider
/// label, status, duration and time spent waiting for a slot.
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
                    target: "aspero::http",
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
        let started = Instant::now();
        let url = loggable_url(&request.url);
        let result = self.inner.send(request).await;
        let ms = |since: Instant, until: Instant| until.duration_since(since).as_millis() as u64;
        let (waited_ms, took_ms) = (ms(queued, started), ms(started, Instant::now()));
        match &result {
            Ok(resp) => tracing::info!(
                target: "aspero::http",
                provider = %self.label, status = resp.status, took_ms, waited_ms, "{url}"
            ),
            Err(e) => tracing::warn!(
                target: "aspero::http",
                provider = %self.label, error = %e, took_ms, waited_ms, "{url}"
            ),
        }
        result
    }
}

/// A URL for logs: path and query only, contact and key parameters
/// redacted (logs get pasted into issues), long queries shortened.
fn loggable_url(url: &str) -> String {
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
    const MAX: usize = 140;
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
}
