//! Citation links: a work's references, and analyses of a set of works.

use std::sync::Arc;

use aspero_core::Cursor;

use crate::records::{Analysis, AnalysisKind, ReferencesPage};
use crate::{Aspero, AsperoError, CallToken, Patience};

#[uniffi::export]
impl Aspero {
    /// A page of the works `id` (any identifier `lookup` takes) cites: up
    /// to `limit` from `cursor` (a previous page's `nextCursor`), each
    /// looked up, in OpenAlex's order. `coverage` is `unknown` when
    /// OpenAlex has no reference data for the work. A work that can't be
    /// resolved gives a `failure`.
    #[uniffi::method(default(cursor = None, cancel = None))]
    pub async fn references(
        &self,
        id: String,
        cursor: Option<String>,
        limit: u32,
        patience: Patience,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<ReferencesPage, AsperoError> {
        let engine = self.engine(patience);
        self.run(
            async move {
                let cursor = cursor.map(Cursor);
                let page = engine.references(&id, cursor.as_ref(), limit).await;
                ReferencesPage::new(&id, page)
            },
            cancel,
        )
        .await
    }

    /// Analyses of the set of works `seeds` names (identifiers as
    /// written): retracted references, odd ones out, probably missing
    /// works, and related works (the best `related_limit` around the set,
    /// by co-citation and coupling). Each seed resolves cheaply (asked
    /// once) or says why not; paid requests stop before `budget_usd` would
    /// be passed, and what stopped says how far it got. With
    /// `estimate_only`, nothing paid is asked: `estimate` says what a run
    /// would cost.
    #[uniffi::method(default(budget_usd = None, related_limit = 20, cancel = None))]
    // Swift takes options as defaulted parameters, not an options record.
    #[allow(clippy::too_many_arguments)]
    pub async fn analyse(
        &self,
        seeds: Vec<String>,
        kinds: Vec<AnalysisKind>,
        budget_usd: Option<f64>,
        estimate_only: bool,
        patience: Patience,
        related_limit: u32,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<Analysis, AsperoError> {
        let engine = self.engine(patience);
        self.run(
            async move {
                let kinds: Vec<aspero_engine::AnalysisKind> = kinds
                    .into_iter()
                    .map(|k| k.to_engine(related_limit))
                    .collect();
                let analysis = engine
                    .analyse(&seeds, &kinds, budget_usd, estimate_only)
                    .await;
                Analysis::new(analysis)
            },
            cancel,
        )
        .await
    }
}
