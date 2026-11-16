//! The HTTP tape: provider responses recorded once and replayed after, so
//! benchmark and red-team runs pay for each distinct request once, ever,
//! and can be run again offline with the same answers.
//!
//! A developer tool, not a cache: it sits below the engine (and its answer
//! cache), keeps whole HTTP responses keyed by the request, and never
//! expires them. Unlike the disposable cache it is durable: the table is
//! created if missing and never dropped, and a file that isn't a database
//! is an error, not a reason to start afresh.
//!
//! Keys and stored URLs are normalised ([`TapeKey`]): query parameters
//! sorted, and credentials and contact addresses (`api_key`, `mailto`,
//! `email`, `Authorization`, …) taken out, so one tape answers with or
//! without a key and never holds a key or an email address; what a
//! response echoes of them is redacted before it's stored. What is worth
//! recording, and how replays are served, is the engine's decision
//! (`aspero_engine::transport::TapeClient`).

use std::path::Path;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use aspero_core::http::Method;
use aspero_core::{HttpRequest, HttpResponse};
use jiff::Timestamp;
use rusqlite::{Connection, OptionalExtension, params};

use crate::{Result, timestamp};

const TAPE: &str = "
CREATE TABLE IF NOT EXISTS tape (
    key          TEXT PRIMARY KEY,  -- TapeKey::key: method, URL, headers, body
    method       TEXT NOT NULL,
    url          TEXT NOT NULL,     -- normalised: no credentials or contacts
    body         BLOB,              -- the request's body, if any
    status       INTEGER NOT NULL,
    headers      TEXT NOT NULL,     -- response headers, JSON [[name, value], …]
    response     BLOB NOT NULL,     -- response body
    recorded_at  INTEGER NOT NULL   -- unix seconds
);
";

/// Query parameters (compared ignoring case) carrying credentials or
/// contact addresses: never in a key, never stored.
const SECRET_PARAMS: &[&str] = &[
    "api_key",
    "apikey",
    "key",
    "mailto",
    "email",
    "token",
    "access_token",
];

/// Request headers carrying credentials or contacts (the user agent names
/// one): never in a key, never stored. So is any header whose name says
/// key, token, secret or auth.
const SECRET_HEADERS: &[&str] = &["cookie", "user-agent", "from"];

/// Response headers never stored.
const DROPPED_HEADERS: &[&str] = &["set-cookie"];

/// Secrets shorter than this aren't redacted from responses: they'd match
/// by chance.
const MIN_SECRET: usize = 4;

const REDACTED: &str = "<redacted>";

/// A request as the tape knows it: method, normalised URL, the headers
/// that can change the answer (`accept`), and the body. Built from the
/// request with its secrets set aside, to be redacted from what's stored.
#[derive(Clone, PartialEq, Eq)]
pub struct TapeKey {
    method: &'static str,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
    secrets: Vec<String>,
}

impl std::fmt::Debug for TapeKey {
    /// Leaves the secrets out.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TapeKey")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &self.headers)
            .field("body_len", &self.body.as_ref().map(Vec::len))
            .finish_non_exhaustive()
    }
}

impl TapeKey {
    /// The request's key; `None` if its URL doesn't parse (such a request
    /// can't be sent either).
    pub fn new(request: &HttpRequest) -> Option<Self> {
        let mut url = url::Url::parse(&request.url).ok()?;
        let is_secret = |name: &str| SECRET_PARAMS.iter().any(|s| name.eq_ignore_ascii_case(s));
        let mut secrets = Vec::new();
        // Secret values as sent (percent-encoded however the caller did)…
        for pair in url.query().unwrap_or_default().split('&') {
            if let Some((name, value)) = pair.split_once('=')
                && is_secret(name)
            {
                secrets.push(value.to_owned());
            }
        }
        let mut params = Vec::new();
        for (name, value) in url.query_pairs() {
            if is_secret(&name) {
                // … as read, and as form-encoded.
                secrets.push(url::form_urlencoded::byte_serialize(value.as_bytes()).collect());
                secrets.push(value.into_owned());
            } else {
                params.push((name.into_owned(), value.into_owned()));
            }
        }
        params.sort();
        if params.is_empty() {
            url.set_query(None);
        } else {
            url.query_pairs_mut().clear().extend_pairs(&params);
        }
        url.set_fragment(None);
        if !url.username().is_empty() || url.password().is_some() {
            secrets.push(url.username().to_owned());
            secrets.extend(url.password().map(str::to_owned));
            // Only fails for URLs that can't have credentials.
            let _ = url.set_username("");
            let _ = url.set_password(None);
        }

        let mut headers = Vec::new();
        for (name, value) in &request.headers {
            let name = name.to_ascii_lowercase();
            if is_secret_header(&name) {
                // "Bearer <token>": the token alone, too.
                secrets.extend(value.split_once(' ').map(|(_, t)| t.trim().to_owned()));
                secrets.push(value.clone());
            } else {
                headers.push((name, value.clone()));
            }
        }
        headers.sort();
        let method = match request.method {
            Method::Post => "POST",
            _ => "GET",
        };
        let mut key = TapeKey {
            method,
            url: url.into(),
            headers,
            body: request.body.clone(),
            secrets: Vec::new(),
        };
        key.add_secrets(secrets);
        Some(key)
    }

    /// Also redacts `secrets` from what's stored: a contact address sent
    /// only in the user agent, which the request doesn't carry.
    pub fn with_secrets(mut self, secrets: impl IntoIterator<Item = String>) -> Self {
        self.add_secrets(secrets);
        self
    }

    fn add_secrets(&mut self, secrets: impl IntoIterator<Item = String>) {
        for secret in secrets {
            if secret.len() >= MIN_SECRET && !self.secrets.contains(&secret) {
                self.secrets.push(secret);
            }
        }
        // Longest first, so a secret containing another is redacted whole.
        self.secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
    }

    /// The key as stored: method and URL, then the headers kept, then the
    /// body (as text, lossily).
    pub fn key(&self) -> String {
        let mut key = format!("{} {}", self.method, self.url);
        for (name, value) in &self.headers {
            key.extend(["\n", name, ": ", value]);
        }
        if let Some(body) = &self.body {
            key.push_str("\n\n");
            key.push_str(&String::from_utf8_lossy(body));
        }
        key
    }

    /// The normalised URL: no credentials or contacts, parameters sorted.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// `text` with every secret replaced.
    fn redact(&self, text: &str) -> String {
        self.secrets
            .iter()
            .fold(text.to_owned(), |t, s| t.replace(s.as_str(), REDACTED))
    }

    fn redact_bytes(&self, bytes: &[u8]) -> Vec<u8> {
        let mut out = bytes.to_vec();
        for secret in &self.secrets {
            out = replace_bytes(&out, secret.as_bytes(), REDACTED.as_bytes());
        }
        out
    }
}

fn is_secret_header(name: &str) -> bool {
    SECRET_HEADERS.contains(&name)
        || ["key", "token", "secret", "auth"]
            .iter()
            .any(|w| name.contains(w))
}

/// `haystack` with every `needle` replaced by `with`.
fn replace_bytes(haystack: &[u8], needle: &[u8], with: &[u8]) -> Vec<u8> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return haystack.to_vec();
    }
    let mut out = Vec::with_capacity(haystack.len());
    let mut i = 0;
    while i < haystack.len() {
        if haystack[i..].starts_with(needle) {
            out.extend_from_slice(with);
            i += needle.len();
        } else {
            out.push(haystack[i]);
            i += 1;
        }
    }
    out
}

/// Recorded responses in a SQLite file (WAL, busy timeout), which several
/// processes may share.
pub struct SqliteTape {
    conn: Mutex<Connection>,
}

impl SqliteTape {
    /// Opens (creating if needed) the tape at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(TAPE)?;
        Ok(SqliteTape {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The response recorded for `key`, as it was stored (secrets
    /// redacted), received when it was recorded.
    pub fn get(&self, key: &TapeKey) -> Result<Option<HttpResponse>> {
        let row: Option<(u16, String, Vec<u8>, i64)> = self
            .conn()
            .query_row(
                "SELECT status, headers, response, recorded_at FROM tape WHERE key = ?1",
                params![key.key()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let Some((status, headers, body, at)) = row else {
            return Ok(None);
        };
        let headers: Vec<(String, String)> = serde_json::from_str(&headers)?;
        Ok(Some(HttpResponse::new(
            status,
            headers,
            body,
            timestamp(at),
        )))
    }

    /// Records `response` as the answer to `key`, replacing any before,
    /// with the key's secrets redacted from its headers and body.
    pub fn put(&self, key: &TapeKey, response: &HttpResponse) -> Result<()> {
        let headers: Vec<(String, String)> = response
            .headers
            .iter()
            .filter(|(name, _)| !DROPPED_HEADERS.contains(&name.as_str()))
            .map(|(name, value)| (name.clone(), key.redact(value)))
            .collect();
        let at: Timestamp = response.received_at;
        self.conn().execute(
            "INSERT OR REPLACE INTO tape
                 (key, method, url, body, status, headers, response, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                key.key(),
                key.method,
                key.url,
                key.body.as_deref().map(|b| key.redact_bytes(b)),
                response.status,
                serde_json::to_string(&headers)?,
                key.redact_bytes(&response.body),
                at.as_second(),
            ],
        )?;
        Ok(())
    }

    /// How many responses are recorded.
    pub fn entries(&self) -> Result<u64> {
        let n: i64 = self
            .conn()
            .query_row("SELECT COUNT(*) FROM tape", [], |r| r.get(0))?;
        Ok(u64::try_from(n).unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "sk-live-0123456789abcdef";
    const EMAIL: &str = "someone@example.org";

    fn key(url: &str) -> TapeKey {
        TapeKey::new(&HttpRequest::get(url)).unwrap()
    }

    fn ok(body: &str) -> HttpResponse {
        HttpResponse::new(
            200,
            vec![("Content-Type".into(), "application/json".into())],
            body.as_bytes().to_vec(),
            Timestamp::from_second(1_794_312_000).unwrap(),
        )
    }

    #[test]
    fn keys_leave_out_secrets_and_sort_parameters() {
        let a = key(&format!(
            "https://api.crossref.org/works?rows=3&mailto={}&query=deep+learning&api_key={KEY}",
            EMAIL.replace('@', "%40")
        ));
        let b = key("https://api.crossref.org/works?query=deep%20learning&rows=3");
        assert_eq!(a.key(), b.key());
        assert_eq!(
            a.url(),
            "https://api.crossref.org/works?query=deep+learning&rows=3"
        );
        for k in [&a.key(), a.url()] {
            assert!(!k.contains(KEY) && !k.contains("example.org"), "{k}");
        }
        assert_eq!(
            key("https://x.org/a?b=2&a=1&KEY=s3cr3t#frag").url(),
            "https://x.org/a?a=1&b=2"
        );
        assert_eq!(
            key("https://x.org/a?email=me%40x.org").url(),
            "https://x.org/a"
        );
    }

    #[test]
    fn keys_leave_out_credential_headers_but_keep_accept() {
        let plain = HttpRequest::get("https://api.openalex.org/works/doi:10.1/x")
            .header("Accept", "application/json");
        let keyed = plain
            .clone()
            .header("Authorization", format!("Bearer {KEY}"))
            .header("User-Agent", format!("aspero/1 (mailto:{EMAIL})"))
            .header("X-Api-Key", KEY);
        let (plain, keyed) = (TapeKey::new(&plain).unwrap(), TapeKey::new(&keyed).unwrap());
        assert_eq!(plain.key(), keyed.key());
        assert!(plain.key().contains("accept: application/json"));
        let csl = TapeKey::new(
            &HttpRequest::get("https://api.openalex.org/works/doi:10.1/x")
                .header("accept", "application/vnd.citationstyles.csl+json"),
        )
        .unwrap();
        assert_ne!(
            csl.key(),
            plain.key(),
            "content negotiation is a different answer"
        );
        assert!(!format!("{keyed:?}").contains(KEY));
    }

    #[test]
    fn a_recorded_tape_holds_no_key_or_email() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tape.sqlite");
        let tape = SqliteTape::open(&path).unwrap();
        let request = HttpRequest::get(format!(
            "https://api.crossref.org/works?query=x&mailto={EMAIL}&api_key={KEY}"
        ))
        .header("authorization", format!("Bearer {KEY}"));
        let k = TapeKey::new(&request)
            .unwrap()
            .with_secrets([EMAIL.to_owned()]);
        // A response echoing both, in its headers and body.
        let mut echoed = ok(&format!(
            r#"{{"query": {{"mailto": "{EMAIL}", "key": "{KEY}"}}}}"#
        ));
        echoed
            .headers
            .push(("x-echo".into(), format!("{EMAIL} {KEY}")));
        echoed
            .headers
            .push(("set-cookie".into(), format!("k={KEY}")));
        tape.put(&k, &echoed).unwrap();

        let replayed = tape.get(&k).unwrap().unwrap();
        assert_eq!(replayed.status, 200);
        assert!(replayed.text().contains(REDACTED));
        assert_eq!(replayed.header("set-cookie"), None);
        drop(tape);

        let mut bytes = Vec::new();
        for suffix in ["", "-wal"] {
            let mut file = path.as_os_str().to_owned();
            file.push(suffix);
            bytes.extend(std::fs::read(&file).unwrap_or_default());
        }
        let has = |needle: &str| bytes.windows(needle.len()).any(|w| w == needle.as_bytes());
        assert!(has("api.crossref.org"), "the tape was read");
        for secret in [KEY, EMAIL, "example.org", "someone%40example"] {
            assert!(!has(secret), "{secret} is on the tape");
        }
    }

    #[test]
    fn replays_carry_what_was_recorded_and_when() {
        let tape = SqliteTape::open_in_memory().unwrap();
        let k = key("https://doi.org/10.1/x");
        assert_eq!(tape.get(&k).unwrap(), None);
        let recorded = ok("{}");
        tape.put(&k, &recorded).unwrap();
        assert_eq!(tape.get(&k).unwrap(), Some(recorded));
        assert_eq!(tape.entries().unwrap(), 1);
    }

    #[test]
    fn byte_replacement_replaces_every_occurrence() {
        assert_eq!(replace_bytes(b"abcabc", b"bc", b"-"), b"a-a-");
        assert_eq!(replace_bytes(b"ab", b"abc", b"-"), b"ab");
    }
}
