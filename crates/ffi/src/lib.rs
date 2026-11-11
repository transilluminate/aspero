//! aspero for apps, through UniFFI: an [`Aspero`] handle that answers in
//! the JSON contract (`aspero-json`), the same shapes `aspero … --format
//! json` prints.
//!
//! The app passes everything in ([`Config`]): nothing is read from the
//! environment, so the app keeps keys where it likes (the Keychain). Calls
//! run on the handle's own runtime, so they can be awaited from any
//! executor; one dropped by its caller (a cancelled Swift `Task`) is
//! cancelled. Hold one handle for the app's life: calls through it share
//! the cache, pacing and requests in flight.

use std::sync::Arc;

use aspero_core::{Claim, ProviderId, Usage};
use aspero_engine::{
    CallOptions, CrossrefConfig, DataciteConfig, DoiConfig, Engine, EuropePmcConfig, Extras,
    OpenAlexConfig, OpenLibraryConfig, SecretString, SqliteCache, StandardConfig, standard,
};
use aspero_json::InputClaim;
use futures::StreamExt;
use serde_json::{Value, json};
use tokio::task::JoinHandle;

uniffi::setup_scaffolding!();

/// Results per call at most from each provider search when verifying.
const SEARCH_LIMIT: u32 = 5;

#[derive(Debug, Clone, uniffi::Record)]
pub struct Config {
    /// Contact address for providers' polite pools: the app's support
    /// address.
    pub mailto: Option<String>,
    pub openalex_key: Option<String>,
    /// Where to keep the answer cache (`cache.sqlite` inside); `None` for
    /// no cache.
    pub cache_dir: Option<String>,
}

/// How patient a call is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Patience {
    /// Providers' own retries and deadlines: for batches.
    Batch,
    /// Someone is waiting: give up on a provider within seconds and fall
    /// back.
    Interactive,
    /// The cache only, however old.
    Offline,
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum AsperoError {
    #[error("could not set up: {0}")]
    Setup(String),
    /// A bug: the call panicked or its task was lost.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Receives each verification as it finishes.
#[uniffi::export(with_foreign)]
pub trait VerifyListener: Send + Sync {
    /// One result: the same object as an item of `verify`'s `results`.
    fn on_result(&self, json: String);
}

#[derive(uniffi::Object)]
pub struct Aspero {
    engine: Engine,
    runtime: tokio::runtime::Runtime,
}

#[uniffi::export]
impl Aspero {
    #[uniffi::constructor]
    pub fn new(config: Config) -> Result<Arc<Self>, AsperoError> {
        let setup = |e: String| AsperoError::Setup(e);
        let cache = match &config.cache_dir {
            Some(dir) => {
                let path = std::path::Path::new(dir).join("cache.sqlite");
                Some(Arc::new(
                    SqliteCache::open(&path).map_err(|e| setup(e.to_string()))?,
                ))
            }
            None => None,
        };
        let mut standard_config = StandardConfig::default();
        standard_config.mailto = config.mailto.filter(|m| !m.trim().is_empty());
        standard_config.pacing = cache.clone().map(|c| c as _);
        standard_config.cache = cache.map(|c| c as _);
        let mut openalex = OpenAlexConfig::default();
        openalex.api_key = config
            .openalex_key
            .filter(|k| !k.trim().is_empty())
            .map(SecretString::from);
        standard_config.openalex = Some(openalex);
        standard_config.crossref = Some(CrossrefConfig::default());
        standard_config.datacite = Some(DataciteConfig::default());
        standard_config.doi = Some(DoiConfig::default());
        standard_config.europepmc = Some(EuropePmcConfig::default());
        standard_config.openlibrary = Some(OpenLibraryConfig::default());
        let engine = standard(standard_config).map_err(|e| setup(e.to_string()))?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("aspero")
            .enable_all()
            .build()
            .map_err(|e| setup(e.to_string()))?;
        Ok(Arc::new(Aspero { engine, runtime }))
    }

    /// Looks up identifiers as written (DOIs, URLs, PMIDs, arXiv IDs, …):
    /// `{"results": [...], "usage": {...}}`, as `aspero lookup --format
    /// json [--full]`.
    pub async fn lookup(
        &self,
        ids: Vec<String>,
        full: bool,
        patience: Patience,
    ) -> Result<String, AsperoError> {
        let engine = self.engine(patience);
        self.run(async move {
            let mut extras = Extras::default();
            extras.full = full;
            let report = engine.lookup_inputs(&ids, extras).await;
            let results: Vec<Value> = ids
                .iter()
                .zip(&report.items)
                .map(|(input, item)| aspero_json::input_lookup(input, item, full))
                .collect();
            json!({"results": results, "usage": aspero_json::usage(&report.usage)}).to_string()
        })
        .await
    }

    /// Verifies citations, each free text or a JSON object of fields (as
    /// `aspero verify -` reads lines): `{"results": [...], "usage":
    /// {...}}` in claim order.
    pub async fn verify(
        &self,
        claims: Vec<String>,
        full: bool,
        patience: Patience,
    ) -> Result<String, AsperoError> {
        let engine = self.engine(patience);
        self.run(async move {
            let mut results: Vec<Option<Value>> = vec![None; claims.len()];
            let usage = verify_all(&engine, &claims, full, |i, item| results[i] = Some(item)).await;
            let results: Vec<Value> = results.into_iter().flatten().collect();
            json!({"results": results, "usage": aspero_json::usage(&usage)}).to_string()
        })
        .await
    }

    /// As [`verify`](Self::verify), handing each result to `listener` as
    /// soon as it finishes, in any order (each echoes its `input` and
    /// `key`). Returns the usage.
    pub async fn verify_each(
        &self,
        claims: Vec<String>,
        full: bool,
        patience: Patience,
        listener: Arc<dyn VerifyListener>,
    ) -> Result<String, AsperoError> {
        let engine = self.engine(patience);
        self.run(async move {
            let usage = verify_all(&engine, &claims, full, |_, item| {
                listener.on_result(item.to_string())
            })
            .await;
            aspero_json::usage(&usage).to_string()
        })
        .await
    }

    /// The identifiers mentioned in `text`, where and as written:
    /// `{"mentions": [...]}`, as `aspero scan --format json`. Offline.
    pub fn scan(&self, text: String) -> String {
        let mentions: Vec<Value> = aspero_core::id::scan(&text)
            .iter()
            .map(|m| aspero_json::mention("-", &text, m))
            .collect();
        json!({"mentions": mentions}).to_string()
    }

    /// Providers refusing calls now and why (a spent quota, a rejected
    /// key, repeated failures): `[{"provider", "error", "category"}]`.
    pub fn unavailable(&self) -> String {
        let list: Vec<Value> = self
            .engine
            .unavailable()
            .into_iter()
            .map(|(provider, e)| {
                json!({
                    "provider": provider,
                    "error": e.to_string(),
                    "category": format!("{:?}", e.category()).to_lowercase(),
                })
            })
            .collect();
        Value::Array(list).to_string()
    }
}

impl Aspero {
    fn engine(&self, patience: Patience) -> Engine {
        self.engine.with_options(match patience {
            Patience::Batch => CallOptions::default(),
            Patience::Interactive => CallOptions::interactive(),
            Patience::Offline => CallOptions::offline(),
        })
    }

    /// Runs `work` on the handle's runtime; dropping the returned future
    /// cancels it.
    async fn run<T: Send + 'static>(
        &self,
        work: impl Future<Output = T> + Send + 'static,
    ) -> Result<T, AsperoError> {
        let mut task = Abortable(self.runtime.spawn(work));
        (&mut task.0)
            .await
            .map_err(|e| AsperoError::Internal(e.to_string()))
    }
}

/// Aborts its task when dropped.
struct Abortable<T>(JoinHandle<T>);

impl<T> Drop for Abortable<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Verifies every claim, handing each item (by claim index) to `each` as
/// it finishes: unreadable claims first, then the rest as they complete.
async fn verify_all(
    engine: &Engine,
    lines: &[String],
    full: bool,
    mut each: impl FnMut(usize, Value),
) -> Vec<(ProviderId, Usage)> {
    let claims: Vec<InputClaim> = lines.iter().map(|l| InputClaim::from_line(l)).collect();
    let (positions, valid): (Vec<usize>, Vec<Claim>) = claims
        .iter()
        .enumerate()
        .filter_map(|(i, c)| Some((i, c.claim.as_ref().ok()?.clone())))
        .unzip();
    for (i, claim) in claims.iter().enumerate() {
        if let Err(e) = &claim.claim {
            each(i, aspero_json::invalid_claim(claim.echo(), e));
        }
    }
    let mut usage = Vec::new();
    if valid.len() > 1 {
        for (p, u) in engine.prefetch(&valid).await.usage {
            add_usage(&mut usage, &p, u);
        }
    }
    let mut verified = std::pin::pin!(engine.verify_many(&valid, SEARCH_LIMIT));
    while let Some((k, v)) = verified.next().await {
        for (p, u) in &v.report.usage {
            add_usage(&mut usage, p, *u);
        }
        let i = positions[k];
        each(i, aspero_json::verification(claims[i].echo(), &v, full));
    }
    usage
}

fn add_usage(totals: &mut Vec<(ProviderId, Usage)>, provider: &ProviderId, usage: Usage) {
    match totals.iter_mut().find(|(p, _)| p == provider) {
        Some((_, total)) => *total = total.then(usage),
        None => totals.push((provider.clone(), usage)),
    }
}
