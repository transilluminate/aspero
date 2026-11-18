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

use aspero_engine::{CallId, Engine, SecretString, SqliteCache, StandardConfig, standard};
use tokio::sync::watch;
use tokio::task::JoinHandle;

mod calls;
mod config;
#[cfg(feature = "mcp")]
pub mod mcp;
mod notices;
pub mod records;
mod system_log;

pub use config::{Config, Patience};
pub use system_log::{LogLevel, enable_system_log};

use records::Verification;

uniffi::setup_scaffolding!();

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
///
/// Its [`id`](Self::id) is the id of the engine call it's passed, which
/// that call's diagnostics events carry as `call` (DESIGN "Diagnostics
/// events"). A call verifying more than one claim gives each claim's call
/// an id of its own, with the token's as its `parent`. Several calls given
/// one token share its id.
#[derive(uniffi::Object)]
pub struct CallToken {
    cancelled: watch::Sender<bool>,
    call: CallId,
}

#[uniffi::export]
impl CallToken {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(CallToken {
            cancelled: watch::Sender::new(false),
            call: CallId::new(),
        })
    }

    /// The id its call's diagnostics events carry as `call`: unique within
    /// the process, from the counter every engine call's id comes from.
    pub fn id(&self) -> u64 {
        self.call.get()
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
        let cache_policy = config.cache_policy();
        let max_bytes = match config.cache_max_mb {
            Some(0) => return Err(setup("the cache's size limit must be at least 1 MB".into())),
            mb => mb.map(aspero_engine::megabytes),
        };
        let cache = match &config.cache_dir {
            Some(dir) => {
                let path = std::path::Path::new(dir).join("cache.sqlite");
                let cache = SqliteCache::open(&path).map_err(|e| setup(e.to_string()))?;
                cache
                    .set_max_bytes(max_bytes)
                    .map_err(|e| setup(e.to_string()))?;
                Some(Arc::new(cache))
            }
            None => None,
        };
        let mailto = config.mailto.filter(|m| !m.trim().is_empty());
        let key = config.openalex_key.filter(|k| !k.trim().is_empty());
        let mut standard_config = StandardConfig::all(key.map(SecretString::from));
        if let Some(cap) = config.openalex_daily_cap_usd
            && !(cap.is_finite() && cap >= 0.0)
        {
            return Err(setup(format!(
                "the OpenAlex daily cap must be zero or more US dollars, not {cap}"
            )));
        }
        if let Some(cap) = config.openalex_daily_credit_cap_usd
            && !(cap.is_finite() && cap >= 0.0)
        {
            return Err(setup(format!(
                "the OpenAlex daily credit cap must be zero or more US dollars, not {cap}"
            )));
        }
        standard_config.openalex_daily_cap_usd = config.openalex_daily_cap_usd;
        standard_config.openalex_daily_credit_cap_usd = config.openalex_daily_credit_cap_usd;
        standard_config
            .disable(&config.disabled_sources)
            .map_err(|e| setup(e.to_string()))?;
        standard_config.cache_policy = cache_policy;
        standard_config.mailto.clone_from(&mailto);
        standard_config.pacing = cache.clone().map(|c| c as _);
        standard_config.prepaid_days = cache.clone().map(|c| c as _);
        standard_config.cache = cache.clone().map(|c| c as _);
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
}

impl Aspero {
    fn engine(&self, patience: Patience) -> Engine {
        self.engine.with_options(patience.options())
    }

    /// Runs `work` on the handle's runtime until it finishes or `cancel`
    /// is cancelled; dropping the returned future (where the caller's
    /// executor does) also cancels it. The engine calls `work` makes take
    /// `cancel`'s id.
    async fn run<T: Send + 'static>(
        &self,
        work: impl Future<Output = T> + Send + 'static,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<T, AsperoError> {
        if cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
            return Err(AsperoError::Cancelled);
        }
        let call = cancel.as_ref().map(|c| c.call.clone());
        let work = async move {
            match call {
                Some(call) => call.scope(work).await,
                None => work.await,
            }
        };
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
