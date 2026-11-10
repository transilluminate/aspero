//! Test support for provider crates (feature `testing`): replay recorded HTTP
//! fixtures through the [`HttpClient`] seam, no network or runtime needed.
//!
//! A fixture is a pair of files recorded with `curl -D`: `<name>.headers`
//! (status line, then `name: value` lines) and `<name>.body`.

use std::collections::VecDeque;
use std::future::Future;
use std::path::Path;
use std::sync::Mutex;
use std::task::{Context, Poll, Waker};

use async_trait::async_trait;
use jiff::Timestamp;

use crate::error::Error;
use crate::http::{HttpClient, HttpRequest, HttpResponse};

/// The `received_at` of every fixture response: 2026-11-10T12:00:00Z.
pub const FIXTURE_TIME: Timestamp = Timestamp::constant(1_794_312_000, 0);

/// Loads `<dir>/<name>.headers` and `<dir>/<name>.body`.
///
/// # Panics
/// If the files are missing or the status line is malformed.
pub fn fixture(dir: impl AsRef<Path>, name: &str) -> HttpResponse {
    let dir = dir.as_ref();
    let read = |ext: &str| {
        let path = dir.join(format!("{name}.{ext}"));
        std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
    };
    let head = String::from_utf8(read("headers")).expect("headers are UTF-8");
    let mut lines = head.lines();
    let status = lines
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .expect("status line like `HTTP/2 200`");
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect();
    HttpResponse::new(status, headers, read("body"), FIXTURE_TIME)
}

/// Builds a response by hand, for cases that can't be recorded on demand
/// (429s, 5xx).
pub fn response(status: u16, headers: &[(&str, &str)], body: &str) -> HttpResponse {
    let headers = headers
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    HttpResponse::new(status, headers, body.as_bytes().to_vec(), FIXTURE_TIME)
}

/// An [`HttpClient`] that answers with queued responses in order and records
/// the requests it was sent.
#[derive(Debug, Default)]
pub struct ReplayClient {
    responses: Mutex<VecDeque<Result<HttpResponse, Error>>>,
    requests: Mutex<Vec<HttpRequest>>,
}

impl ReplayClient {
    pub fn new(responses: impl IntoIterator<Item = HttpResponse>) -> Self {
        ReplayClient {
            responses: Mutex::new(responses.into_iter().map(Ok).collect()),
            requests: Mutex::default(),
        }
    }

    /// Queues a transport failure as the next answer.
    pub fn then_fail(self, error: Error) -> Self {
        self.responses.lock().unwrap().push_back(Err(error));
        self
    }

    /// Requests sent so far, in order.
    pub fn requests(&self) -> Vec<HttpRequest> {
        self.requests.lock().unwrap().clone()
    }
}

#[async_trait]
impl HttpClient for ReplayClient {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, Error> {
        let url = request.url.clone();
        self.requests.lock().unwrap().push(request);
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| panic!("ReplayClient: no response queued for {url}"))
    }
}

/// Runs a future that completes without waiting, as everything driven by a
/// [`ReplayClient`] does. Avoids pulling an async runtime into provider tests.
///
/// # Panics
/// If the future is not immediately ready.
pub fn block_on<F: Future>(fut: F) -> F::Output {
    let mut fut = std::pin::pin!(fut);
    match fut.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(out) => out,
        Poll::Pending => panic!("future was not immediately ready"),
    }
}
