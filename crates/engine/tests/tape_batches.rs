//! Lookups under the HTTP tape go one ID per request, so a replay survives
//! batches made up differently: by a code change moving which claims carry
//! identifiers, or a run over fewer claims. Batches recorded whole (before)
//! still replay; verdicts are the same with and without the tape.
#![cfg(feature = "openalex")]

use std::sync::{Arc, Mutex};

use aspero_core::testing::FIXTURE_TIME;
use aspero_core::{
    Claim, Error, FieldSet, HttpClient, HttpRequest, HttpResponse, ResiliencePolicy, WorkId,
};
use aspero_engine::transport::{HttpTape, TapeClient, TapeMode};
use aspero_engine::{Engine, OpenAlexConfig, Outcome, SqliteCache, SqliteTape};
use aspero_openalex::OpenAlex;
use async_trait::async_trait;
use futures::StreamExt;

/// OpenAlex as far as identifier lookups go: a work by DOI alone (404 when
/// there's none) or by a `doi:a|b|…` filter; any other list is empty.
/// Counts what it's sent.
struct FakeOpenAlex {
    /// DOI, title.
    works: Vec<(&'static str, &'static str)>,
    sent: Mutex<Vec<String>>,
}

impl FakeOpenAlex {
    fn new() -> Arc<Self> {
        Arc::new(FakeOpenAlex {
            works: vec![
                ("10.1000/alpha", "Sparse attention in protein folding"),
                ("10.1000/beta", "Glacier retreat in the southern Andes"),
                ("10.1000/gamma", "A census of urban foxes"),
                ("10.1000/delta", "Tidal energy and estuary sediment"),
            ],
            sent: Mutex::default(),
        })
    }

    fn sent(&self) -> Vec<String> {
        self.sent.lock().unwrap().clone()
    }

    fn work(&self, doi: &str) -> Option<serde_json::Value> {
        let n = self
            .works
            .iter()
            .position(|(d, _)| d.eq_ignore_ascii_case(doi))?;
        let (doi, title) = self.works[n];
        Some(serde_json::json!({
            "id": format!("https://openalex.org/W{}", 100 + n),
            "doi": format!("https://doi.org/{doi}"),
            "title": title,
            "display_name": title,
            "publication_year": 2019,
            "type": "article",
            "authorships": [{
                "author": {"display_name": "Ada Smith"},
                "raw_author_name": "Ada Smith"
            }]
        }))
    }
}

fn respond(status: u16, body: &serde_json::Value) -> HttpResponse {
    HttpResponse::new(
        status,
        vec![
            ("content-type".into(), "application/json".into()),
            ("x-ratelimit-cost-usd".into(), "0".into()),
        ],
        body.to_string().into_bytes(),
        FIXTURE_TIME,
    )
}

/// `%XX` escapes decoded (and `+` as a space), as a query or path has them.
fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 3 <= bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap();
                out.push(u8::from_str_radix(hex, 16).unwrap());
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap()
}

#[async_trait]
impl HttpClient for FakeOpenAlex {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, Error> {
        self.sent.lock().unwrap().push(request.url.clone());
        let (path, query) = request.url.split_once('?').unwrap_or((&request.url, ""));
        let path = decode(path);
        if let Some((_, doi)) = path.split_once("/works/doi:") {
            return Ok(match self.work(doi) {
                Some(work) => respond(200, &work),
                None => respond(404, &serde_json::json!({"error": "not found"})),
            });
        }
        let filter = query
            .split('&')
            .filter_map(|kv| kv.split_once('='))
            .find(|(k, _)| *k == "filter")
            .map(|(_, v)| decode(v));
        let results: Vec<serde_json::Value> = match filter.as_deref() {
            Some(f) if f.starts_with("doi:") => f["doi:".len()..]
                .split('|')
                .filter_map(|doi| self.work(doi))
                .collect(),
            _ => Vec::new(),
        };
        let list = serde_json::json!({"meta": {"count": results.len()}, "results": results});
        Ok(respond(200, &list))
    }
}

fn tape(mode: TapeMode) -> HttpTape {
    HttpTape::new(Arc::new(SqliteTape::open_in_memory().unwrap()), mode)
}

/// OpenAlex over `http`, behind `tape` if any, the engine told so when
/// `taped`; an empty answer cache, as the harnesses run with one.
fn engine(http: Arc<dyn HttpClient>, tape: Option<&HttpTape>, taped: bool) -> Engine {
    let http: Arc<dyn HttpClient> = match tape {
        Some(tape) => Arc::new(TapeClient::new("openalex", http, tape)),
        None => http,
    };
    let mut policy = ResiliencePolicy::default();
    policy.max_retries = 0;
    Engine::builder()
        .provider(
            Arc::new(OpenAlex::new(OpenAlexConfig::default(), http)),
            policy,
            10,
        )
        .cache(Arc::new(SqliteCache::open_in_memory().unwrap()))
        .clock(|| FIXTURE_TIME)
        .taped(taped)
        .build()
}

fn ids(dois: &[&str]) -> Vec<WorkId> {
    dois.iter().map(|d| d.parse().unwrap()).collect()
}

async fn outcomes(engine: &Engine, dois: &[&str]) -> Vec<String> {
    let report = engine.lookup(&ids(dois), FieldSet::DEFAULT).await;
    report
        .results
        .iter()
        .map(|r| match &r.attempts[0].outcome {
            Outcome::Found => "found".to_owned(),
            Outcome::NotFound => "not found".to_owned(),
            other => format!("{other:?}"),
        })
        .collect()
}

#[tokio::test]
async fn recomposed_batches_replay_from_each_ids_entry() {
    let tape = tape(TapeMode::Record);
    let http = FakeOpenAlex::new();
    let first = engine(http.clone(), Some(&tape), true);
    let asked = [
        "10.1000/alpha",
        "10.1000/beta",
        "10.1000/gamma",
        "10.9999/absent",
    ];
    assert_eq!(
        outcomes(&first, &asked).await,
        ["found", "found", "found", "not found"]
    );
    let sent = http.sent();
    assert_eq!(sent.len(), 4, "one request per ID: {sent:?}");
    assert!(sent.iter().all(|u| u.contains("/works/doi:")), "{sent:?}");
    assert_eq!(tape.tape.entries().unwrap(), 4);

    // Made up differently, in another order: all on the tape, even the
    // absence, and nothing sent (replay-only would fail anything that
    // isn't).
    let offline = HttpTape::new(tape.tape.clone(), TapeMode::ReplayOnly);
    let http = FakeOpenAlex::new();
    let second = engine(http.clone(), Some(&offline), true);
    assert_eq!(
        outcomes(
            &second,
            &["10.9999/absent", "10.1000/gamma", "10.1000/alpha"]
        )
        .await,
        ["not found", "found", "found"]
    );
    assert_eq!(
        outcomes(&second, &["10.1000/beta", "10.9999/absent"]).await,
        ["found", "not found"]
    );
    assert!(
        http.sent().is_empty(),
        "zero inner calls: {:?}",
        http.sent()
    );
}

#[tokio::test]
async fn batches_recorded_whole_still_replay() {
    // A tape recorded by an engine that batched.
    let tape = tape(TapeMode::Record);
    let http = FakeOpenAlex::new();
    let batched = engine(http.clone(), Some(&tape), false);
    let asked = ["10.1000/alpha", "10.1000/beta", "10.9999/absent"];
    let answers = outcomes(&batched, &asked).await;
    assert_eq!(answers, ["found", "found", "not found"]);
    assert_eq!(http.sent().len(), 1, "one filter call");

    // The same batch replays whole, nothing sent.
    let http = FakeOpenAlex::new();
    let taped = engine(http.clone(), Some(&tape), true);
    assert_eq!(outcomes(&taped, &asked).await, answers);
    assert!(http.sent().is_empty(), "{:?}", http.sent());

    // One made up differently is asked an ID at a time, and so recorded.
    let http = FakeOpenAlex::new();
    let taped = engine(http.clone(), Some(&tape), true);
    assert_eq!(
        outcomes(&taped, &["10.1000/beta", "10.1000/delta"]).await,
        ["found", "found"]
    );
    let sent = http.sent();
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert!(sent.iter().all(|u| u.contains("/works/doi:")), "{sent:?}");
}

/// Each claim's verdict and reason, in claim order.
async fn verdicts(engine: &Engine, claims: &[Claim]) -> Vec<String> {
    let mut found: Vec<(usize, String)> = engine
        .verify_batch(claims, 4)
        .map(|(i, v)| (i, format!("{:?}: {}", v.verdict, v.reason_text)))
        .collect()
        .await;
    found.sort_by_key(|(i, _)| *i);
    found.into_iter().map(|(_, v)| v).collect()
}

#[tokio::test]
async fn verdicts_are_the_same_with_and_without_the_tape() {
    let claims: Vec<Claim> = [
        "Smith A. Sparse attention in protein folding. 2019. doi:10.1000/alpha",
        "Smith A. Glacier retreat in the southern Andes. 2019. doi:10.1000/beta",
        "Smith A. A census of urban foxes. 2019. doi:10.1000/gamma",
        "Smith A. Tidal energy and estuary sediment. 2019. doi:10.1000/gamma",
        "Jones B. A paper never written. 2018. doi:10.9999/absent",
    ]
    .into_iter()
    .map(Claim::from_text)
    .collect();

    let untaped = verdicts(&engine(FakeOpenAlex::new(), None, false), &claims).await;
    assert!(untaped[0].starts_with("Verified"), "{untaped:?}");
    assert!(!untaped[3].starts_with("Verified"), "{untaped:?}");
    assert!(!untaped[4].starts_with("Verified"), "{untaped:?}");

    let tape = tape(TapeMode::Record);
    let taped = verdicts(&engine(FakeOpenAlex::new(), Some(&tape), true), &claims).await;
    assert_eq!(taped, untaped);

    // Fewer claims, in another order: replayed whole, the same verdicts.
    let offline = HttpTape::new(tape.tape.clone(), TapeMode::ReplayOnly);
    let http = FakeOpenAlex::new();
    let fewer = [claims[4].clone(), claims[1].clone(), claims[3].clone()];
    let replayed = verdicts(&engine(http.clone(), Some(&offline), true), &fewer).await;
    assert_eq!(
        replayed,
        [untaped[4].clone(), untaped[1].clone(), untaped[3].clone()]
    );
    assert!(
        http.sent().is_empty(),
        "zero inner calls: {:?}",
        http.sent()
    );
}
