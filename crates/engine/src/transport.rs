//! The engine's side of the [`HttpClient`] seam: a real client, and pacing.

use std::sync::Arc;
use std::time::Duration;

use aspero_core::http::Method;
use aspero_core::{Error, HttpClient, HttpRequest, HttpResponse};
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
/// sharing it. Slots are handed out in arrival order, so waiting is fair.
///
/// In-process only; the cross-process limiter (DESIGN §6) is deferred.
pub struct Paced {
    inner: Arc<dyn HttpClient>,
    min_interval: Duration,
    next_slot: Mutex<Option<Instant>>,
}

impl Paced {
    pub fn new(inner: Arc<dyn HttpClient>, min_interval: Duration) -> Self {
        Paced {
            inner,
            min_interval,
            next_slot: Mutex::new(None),
        }
    }

    /// Claims the next slot and returns when it starts.
    async fn claim(&self) -> Instant {
        let mut next = self.next_slot.lock().await;
        let now = Instant::now();
        let slot = next.map_or(now, |n| n.max(now));
        *next = Some(slot + self.min_interval);
        slot
    }
}

#[async_trait]
impl HttpClient for Paced {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, Error> {
        let slot = self.claim().await;
        tokio::time::sleep_until(slot).await;
        self.inner.send(request).await
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
        let paced = Arc::new(Paced::new(Arc::new(Echo), Duration::from_millis(250)));
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

    #[tokio::test(start_paused = true)]
    async fn idle_time_is_not_banked() {
        let paced = Paced::new(Arc::new(Echo), Duration::from_millis(100));
        paced.send(HttpRequest::get("http://x")).await.unwrap();
        tokio::time::sleep(Duration::from_secs(5)).await;
        let start = Instant::now();
        paced.send(HttpRequest::get("http://x")).await.unwrap();
        paced.send(HttpRequest::get("http://x")).await.unwrap();
        assert_eq!(start.elapsed().as_millis(), 100);
    }
}
