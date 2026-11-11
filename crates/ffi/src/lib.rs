//! aspero for apps, through UniFFI: an [`Aspero`] handle whose calls
//! return typed records ([`records`]), with the reasons and failures as
//! data so the app can word and localise them.
//!
//! The app passes everything in ([`Config`]): nothing is read from the
//! environment, so the app keeps keys where it likes (the Keychain). Calls
//! run on the handle's own runtime, so they can be awaited from any
//! executor. To cancel one, pass a [`CallToken`] and cancel it: UniFFI's
//! Swift bindings don't pass a `Task`'s cancellation to Rust, so the app
//! calls `cancel()` from `withTaskCancellationHandler`. Hold one handle for
//! the app's life: calls through it share the cache, pacing and requests
//! in flight.

use std::sync::Arc;

use aspero_core::{
    Claim, Cursor, Error, ErrorKind, FieldSet, ProviderId, SearchQuery, Usage, WorkId, WorkType,
    YearRange,
};
use aspero_engine::{
    CallOptions, CrossrefConfig, DataciteConfig, DoiConfig, Engine, EuropePmcConfig, Extras,
    OpenAlexConfig, OpenLibraryConfig, SecretString, SqliteCache, StandardConfig, standard,
};
use aspero_json::InputClaim;
use futures::StreamExt;
use tokio::sync::watch;
use tokio::task::JoinHandle;

pub mod records;

use records::{
    Budget, CacheStats, KeyCheck, KeyStatus, Lookup, Lookups, Mention, ProviderUsage, Reference,
    SearchRequest, SearchResults, Unavailable, Verification, Verifications,
};

uniffi::setup_scaffolding!();

/// Results per call at most from each provider search when verifying.
const SEARCH_LIMIT: u32 = 5;

/// A work every OpenAlex key can look up, free: for checking keys.
const KNOWN_WORK: &str = "doi:10.1038/nature14539";

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
    /// Its [`CallToken`] was cancelled.
    #[error("cancelled")]
    Cancelled,
    /// A bug: the call panicked or its task was lost.
    #[error("internal error: {0}")]
    Internal(String),
    /// The cache file couldn't be read or written.
    #[error("cache error: {0}")]
    Cache(String),
}

/// Receives each verification as it finishes.
#[uniffi::export(with_foreign)]
pub trait VerifyListener: Send + Sync {
    /// One result; its `index` says which claim.
    fn on_result(&self, result: Verification);
}

/// Cancels the calls it's passed. One token may cancel several calls; a
/// call given a token already cancelled ends at once. Cancelled calls stop
/// their requests and throw [`AsperoError::Cancelled`].
#[derive(uniffi::Object)]
pub struct CallToken {
    cancelled: watch::Sender<bool>,
}

#[uniffi::export]
impl CallToken {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(CallToken {
            cancelled: watch::Sender::new(false),
        })
    }

    pub fn cancel(&self) {
        self.cancelled.send_replace(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.cancelled.borrow()
    }
}

impl CallToken {
    async fn cancelled(&self) {
        let _ = self.cancelled.subscribe().wait_for(|c| *c).await;
    }
}

#[derive(uniffi::Object)]
pub struct Aspero {
    engine: Engine,
    runtime: tokio::runtime::Runtime,
    cache: Option<Arc<SqliteCache>>,
    mailto: Option<String>,
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
        let mailto = config.mailto.filter(|m| !m.trim().is_empty());
        let mut standard_config = StandardConfig::default();
        standard_config.mailto = mailto.clone();
        standard_config.pacing = cache.clone().map(|c| c as _);
        standard_config.cache = cache.clone().map(|c| c as _);
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
        Ok(Arc::new(Aspero {
            engine,
            runtime,
            cache,
            mailto,
        }))
    }

    /// Looks up identifiers as written (DOIs, URLs, PMIDs, arXiv IDs, …).
    /// With `full`: notices in both directions, open access, disagreements
    /// with the DOI's registry, and the latest version.
    #[uniffi::method(default(cancel = None))]
    pub async fn lookup(
        &self,
        ids: Vec<String>,
        full: bool,
        patience: Patience,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<Lookups, AsperoError> {
        let engine = self.engine(patience);
        self.run(
            async move {
                let mut extras = Extras::default();
                extras.full = full;
                let report = engine.lookup_inputs(&ids, extras).await;
                Lookups {
                    results: ids
                        .iter()
                        .zip(&report.items)
                        .map(|(input, item)| Lookup::new(input, item))
                        .collect(),
                    usage: records::usage(&report.usage),
                }
            },
            cancel,
        )
        .await
    }

    /// Verifies citations, each free text or a JSON object of fields (as
    /// `aspero verify -` reads lines), answering in claim order. With
    /// `full`, every candidate considered.
    #[uniffi::method(default(cancel = None))]
    pub async fn verify(
        &self,
        claims: Vec<String>,
        full: bool,
        patience: Patience,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<Verifications, AsperoError> {
        let engine = self.engine(patience);
        self.run(
            async move {
                let mut results: Vec<Option<Verification>> = vec![None; claims.len()];
                let usage = verify_all(&engine, &claims, full, |v| {
                    let i = v.index as usize;
                    results[i] = Some(v);
                })
                .await;
                Verifications {
                    results: results.into_iter().flatten().collect(),
                    usage,
                }
            },
            cancel,
        )
        .await
    }

    /// As [`verify`](Self::verify), handing each result to `listener` as
    /// soon as it finishes, in any order. Returns the usage.
    #[uniffi::method(default(cancel = None))]
    pub async fn verify_each(
        &self,
        claims: Vec<String>,
        full: bool,
        patience: Patience,
        listener: Arc<dyn VerifyListener>,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<Vec<ProviderUsage>, AsperoError> {
        let engine = self.engine(patience);
        self.run(
            async move { verify_all(&engine, &claims, full, |v| listener.on_result(v)).await },
            cancel,
        )
        .await
    }

    /// Searches one provider by keywords: `request.source`, or the first
    /// that searches. A failed search has no hits and says why.
    #[uniffi::method(default(cancel = None))]
    pub async fn search(
        &self,
        request: SearchRequest,
        patience: Patience,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<SearchResults, AsperoError> {
        let engine = self.engine(patience);
        self.run(
            async move {
                let mut query = SearchQuery::new(request.text);
                if request.year_from.is_some() || request.year_to.is_some() {
                    query.years = Some(YearRange {
                        from: request.year_from,
                        to: request.year_to,
                    });
                }
                query.work_type = request.work_type.as_deref().map(WorkType::from_csl);
                query.open_access = request.open_access_only.then_some(true);
                query.page.limit = request.limit;
                query.page.cursor = request.cursor.map(Cursor);
                SearchResults::new(engine.search(request.source.as_deref(), &query).await)
            },
            cancel,
        )
        .await
    }

    /// The identifiers mentioned in `text`, in order, where and as written.
    /// Offline.
    pub fn scan(&self, text: String) -> Vec<Mention> {
        aspero_core::id::scan(&text)
            .iter()
            .map(|m| Mention::new(&text, m))
            .collect()
    }

    /// The references in a pasted list (numbered, bulleted, one per
    /// paragraph or line; wrapped lines rejoined), each ready to verify.
    /// Offline.
    pub fn split_references(&self, text: String) -> Vec<Reference> {
        aspero_core::split_references(&text)
            .iter()
            .map(|r| Reference::new(&text, r))
            .collect()
    }

    /// Each provider's budget as its last response reported it, without
    /// spending any: OpenAlex's, once it has been called (or a key
    /// checked) since launch.
    pub fn budgets(&self) -> Vec<Budget> {
        self.engine
            .budgets()
            .iter()
            .map(|(p, b)| Budget::new(p, b))
            .collect()
    }

    /// Whether OpenAlex accepts `key`, with one free lookup (never
    /// cached), and what it has left. Doesn't change the handle's key.
    #[uniffi::method(default(cancel = None))]
    pub async fn check_openalex_key(
        &self,
        key: String,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<KeyCheck, AsperoError> {
        if key.trim().is_empty() {
            return Ok(KeyCheck {
                status: KeyStatus::Rejected,
                message: Some("no key given".into()),
                budget: None,
            });
        }
        let mut config = StandardConfig::default();
        config.mailto = self.mailto.clone();
        config.pacing = self.cache.clone().map(|c| c as _);
        let mut openalex = OpenAlexConfig::default();
        openalex.api_key = Some(SecretString::from(key.trim().to_owned()));
        config.openalex = Some(openalex);
        let engine = standard(config)
            .map_err(|e| AsperoError::Setup(e.to_string()))?
            .with_options(CallOptions::interactive());
        self.run(
            async move {
                let id: WorkId = KNOWN_WORK.parse().expect("a valid identifier");
                let report = engine.lookup(&[id], FieldSet::DEFAULT).await;
                let failure = report.results[0]
                    .attempts
                    .iter()
                    .find_map(|a| match &a.outcome {
                        aspero_engine::Outcome::Failed(e) => Some(e.clone()),
                        _ => None,
                    });
                let status = match failure.as_ref().map(Error::kind) {
                    None | Some(ErrorKind::BudgetSpent) => KeyStatus::Valid,
                    Some(ErrorKind::KeyRejected) => KeyStatus::Rejected,
                    Some(_) => KeyStatus::Unreachable,
                };
                KeyCheck {
                    status,
                    message: failure
                        .filter(|_| status != KeyStatus::Valid)
                        .map(|e| e.to_string()),
                    budget: engine.budgets().first().map(|(p, b)| Budget::new(p, b)),
                }
            },
            cancel,
        )
        .await
    }

    /// What the cache holds; `None` without one.
    pub fn cache_stats(&self) -> Result<Option<CacheStats>, AsperoError> {
        let Some(cache) = &self.cache else {
            return Ok(None);
        };
        let s = cache
            .stats()
            .map_err(|e| AsperoError::Cache(e.to_string()))?;
        Ok(Some(CacheStats {
            records: s.records,
            identifiers: s.identifiers,
            misses: s.misses,
            searches: s.searches,
        }))
    }

    /// Empties the cache.
    pub fn clear_cache(&self) -> Result<(), AsperoError> {
        match &self.cache {
            Some(cache) => cache.clear().map_err(|e| AsperoError::Cache(e.to_string())),
            None => Ok(()),
        }
    }

    /// Providers refusing calls now, and why: a spent budget, a rejected
    /// key, repeated failures.
    pub fn unavailable(&self) -> Vec<Unavailable> {
        self.engine
            .unavailable()
            .into_iter()
            .map(|(provider, e)| Unavailable {
                provider: provider.to_string(),
                kind: e.kind().into(),
                message: e.to_string(),
                until: match e {
                    Error::QuotaExhausted { resets_at: t } | Error::CircuitOpen { until: t } => {
                        t.map(Into::into)
                    }
                    _ => None,
                },
            })
            .collect()
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

    /// Runs `work` on the handle's runtime until it finishes or `cancel`
    /// is cancelled; dropping the returned future (where the caller's
    /// executor does) also cancels it.
    async fn run<T: Send + 'static>(
        &self,
        work: impl Future<Output = T> + Send + 'static,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<T, AsperoError> {
        if cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
            return Err(AsperoError::Cancelled);
        }
        let mut task = Abortable(self.runtime.spawn(work));
        let cancelled = async {
            match &cancel {
                Some(token) => token.cancelled().await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            biased;
            () = cancelled => Err(AsperoError::Cancelled),
            done = &mut task.0 => done.map_err(|e| AsperoError::Internal(e.to_string())),
        }
    }
}

/// Aborts its task when dropped.
struct Abortable<T>(JoinHandle<T>);

impl<T> Drop for Abortable<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Verifies every claim, handing each result to `each` as it finishes:
/// unreadable claims first, then the rest as they complete.
async fn verify_all(
    engine: &Engine,
    lines: &[String],
    full: bool,
    mut each: impl FnMut(Verification),
) -> Vec<ProviderUsage> {
    let claims: Vec<InputClaim> = lines.iter().map(|l| InputClaim::from_line(l)).collect();
    let (positions, valid): (Vec<usize>, Vec<Claim>) = claims
        .iter()
        .enumerate()
        .filter_map(|(i, c)| Some((i, c.claim.as_ref().ok()?.clone())))
        .unzip();
    for (i, claim) in claims.iter().enumerate() {
        if let Err(e) = &claim.claim {
            each(Verification::invalid(i, claim, e));
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
        each(Verification::new(i, &claims[i], &v, full));
    }
    records::usage(&usage)
}

fn add_usage(totals: &mut Vec<(ProviderId, Usage)>, provider: &ProviderId, usage: Usage) {
    match totals.iter_mut().find(|(p, _)| p == provider) {
        Some((_, total)) => *total = total.then(usage),
        None => totals.push((provider.clone(), usage)),
    }
}
