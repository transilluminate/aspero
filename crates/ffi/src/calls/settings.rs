//! What a settings or about pane shows: budgets, the OpenAlex key's check,
//! the cache, providers refusing calls, and the library's dependencies.

use std::sync::Arc;

use aspero_core::Error;

use crate::records::{Budget, CacheStats, Dependency, KeyCheck, Unavailable};
use crate::{Aspero, AsperoError, CallToken, Patience, notices};

#[uniffi::export]
impl Aspero {
    /// What the library is built from, by name (ignoring case): every
    /// crate (its normal dependencies, for macOS) with its licence, and
    /// the bundled citation style data. Offline; generated at release
    /// (`just notices`).
    pub fn dependencies(&self) -> Vec<Dependency> {
        notices::dependencies().to_vec()
    }

    /// The full third-party notices: each licence text once, after the
    /// crates under it and their copyright lines, then the citation style
    /// data's licence and authors. For an app's acknowledgements. Offline.
    pub fn third_party_notices(&self) -> String {
        notices::NOTICES.to_owned()
    }

    /// Each provider's budget as its last response reported it, without
    /// spending any: OpenAlex's, once it has been called (or a key
    /// checked) since launch.
    pub fn budgets(&self) -> Vec<Budget> {
        self.engine
            .budgets()
            .iter()
            .map(|(p, b)| Budget::new(p, b).capped(&self.engine, p, b))
            .collect()
    }

    /// Asks OpenAlex what's used and left of today's budget, for free
    /// (`/rate-limit`, with a key), then answers as `budgets` does. Without
    /// a key, or `offline`, nothing is asked: the budgets are as the last
    /// responses left them.
    #[uniffi::method(default(cancel = None))]
    pub async fn refresh_budgets(
        &self,
        patience: Patience,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<Vec<Budget>, AsperoError> {
        let engine = self.engine(patience);
        self.run(
            async move {
                engine
                    .refresh_budgets()
                    .await
                    .iter()
                    .map(|(p, b)| Budget::new(p, b).capped(&engine, p, b))
                    .collect()
            },
            cancel,
        )
        .await
    }

    /// Whether OpenAlex accepts `key`, with one free lookup (never
    /// cached), and what it has left. Doesn't change the handle's key.
    #[uniffi::method(default(cancel = None))]
    pub async fn check_openalex_key(
        &self,
        key: String,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<KeyCheck, AsperoError> {
        let mailto = self.mailto.clone();
        let pacing = self.cache.clone().map(|c| c as _);
        let checked = self
            .run(
                async move { aspero_engine::check_openalex_key(&key, mailto, pacing).await },
                cancel,
            )
            .await?;
        let check = checked.map_err(|e| AsperoError::Setup(e.to_string()))?;
        Ok(KeyCheck {
            status: check.status.into(),
            message: check.message,
            budget: check
                .budget
                .map(|(p, b)| Budget::new(&p, &b).capped(&self.engine, &p, &b)),
        })
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
            bytes: s.bytes,
            edges: s.edges,
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
