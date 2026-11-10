//! The transport seam between providers and the network.
//!
//! Providers build [`HttpRequest`]s and map [`HttpResponse`]s; they never own
//! a client. The engine supplies the [`HttpClient`], which is where per-request
//! pacing, timeouts and the user agent live, so every provider gets the same
//! politeness for free. Tests supply a client that replays recorded fixtures.
//!
//! A client returns `Ok` for any HTTP response, whatever the status; only
//! transport failures (connection, TLS, timeout) are `Err`. Mapping statuses
//! to [`Error`]s is provider knowledge.

use std::fmt;

use async_trait::async_trait;
use jiff::Timestamp;

use crate::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Method {
    Get,
    Post,
}

/// Header names whose values are never shown in `Debug` output.
const SENSITIVE_HEADERS: &[&str] = &["authorization", "x-api-key"];

#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    /// Header names are lowercase.
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        HttpRequest {
            method: Method::Get,
            url: url.into(),
            headers: Vec::new(),
            body: None,
        }
    }

    pub fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_ascii_lowercase(), value.into()));
        self
    }
}

impl fmt::Debug for HttpRequest {
    /// Redacts credentials, so requests can be logged safely.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let headers: Vec<(&str, &str)> = self
            .headers
            .iter()
            .map(|(k, v)| {
                let v = if SENSITIVE_HEADERS.contains(&k.as_str()) {
                    "<redacted>"
                } else {
                    v.as_str()
                };
                (k.as_str(), v)
            })
            .collect();
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &headers)
            .field("body_len", &self.body.as_ref().map(Vec::len))
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct HttpResponse {
    pub status: u16,
    /// Header names are lowercase.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// When the response arrived, stamped by the client. Providers use it for
    /// `fetched_at` and to turn relative reset times into timestamps.
    pub received_at: Timestamp,
}

impl HttpResponse {
    pub fn new(
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
        received_at: Timestamp,
    ) -> Self {
        let headers = headers
            .into_iter()
            .map(|(k, v)| (k.to_ascii_lowercase(), v))
            .collect();
        HttpResponse {
            status,
            headers,
            body,
            received_at,
        }
    }

    /// First value of a header, by case-insensitive name.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// The body as text, lossily, for error messages.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// Characters escaped by [`path_segment`].
const PATH_ESCAPES: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

/// Escapes an identifier for use in a URL path, leaving `/` and `:` literal:
/// APIs expect `/works/doi:10.1038/nature14539` as written, but DOIs may
/// contain `#`, `?` or `<>`, which must not be taken as URL syntax.
pub fn path_segment(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, PATH_ESCAPES).to_string()
}

#[async_trait]
pub trait HttpClient: Send + Sync {
    /// Sends one request. `Ok` for every HTTP status; `Err` only for transport
    /// failures (`Network`, `Timeout`).
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, Error>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_credentials() {
        let req = HttpRequest::get("https://example.org")
            .header("Authorization", "Bearer s3cret")
            .header("Accept", "application/json");
        let shown = format!("{req:?}");
        assert!(!shown.contains("s3cret"), "{shown}");
        assert!(shown.contains("application/json"));
    }

    #[test]
    fn path_segment_keeps_doi_slashes_but_escapes_url_syntax() {
        assert_eq!(
            path_segment("doi:10.1038/nature14539"),
            "doi:10.1038/nature14539"
        );
        assert_eq!(
            path_segment("10.1002/(sici)1097<34::aid>3.0.co;2-#"),
            "10.1002/(sici)1097%3C34::aid%3E3.0.co;2-%23"
        );
    }

    #[test]
    fn response_headers_are_case_insensitive() {
        let resp = HttpResponse::new(
            200,
            vec![("X-RateLimit-Cost-USD".into(), "0.001".into())],
            Vec::new(),
            Timestamp::UNIX_EPOCH,
        );
        assert_eq!(resp.header("x-ratelimit-cost-usd"), Some("0.001"));
    }
}
