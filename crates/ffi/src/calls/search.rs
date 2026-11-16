//! Searching, and what each source can search by.

use std::sync::Arc;

use aspero_core::{Cursor, SearchQuery, Sort, WorkType, YearRange};
use aspero_engine::EngineError;

use crate::records::{SearchNode, SearchRequest, SearchResults, SourceSearch};
use crate::{Aspero, AsperoError, CallToken, Patience};

#[uniffi::export]
impl Aspero {
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
                query.author = request.author;
                query.title = request.title;
                query.container = request.container;
                query.sort = request.sort.map_or(Sort::Relevance, Into::into);
                query.extra = request.extra.into_iter().collect();
                query.tree = request.query.map(Into::into);
                match request.since.as_deref().map(SearchQuery::parse_since) {
                    Some(Err(reason)) => {
                        return SearchResults::new(Err(EngineError::InvalidQuery(reason)));
                    }
                    Some(Ok(since)) => query.since = Some(since),
                    None => {}
                }
                SearchResults::new(engine.search(request.source.as_deref(), &query).await)
            },
            cancel,
        )
        .await
    }

    /// Whether `source` (or, without one, any source: `search` then picks
    /// the first that can) can search by `query`, and with `since` (as
    /// `SearchRequest.since`), only works added since that day: `nil` if
    /// so, otherwise why, in English ("OpenAlex can't combine title and
    /// author with any", "OpenAlex can't search by date added (it needs a
    /// paid plan)"; without a source, the first's reason, ", and no other
    /// source can").
    /// Offline and instant. Searching anyway fails as `unsupported` with
    /// the same reason, or as `badRequest` for a value that can't be valid
    /// (an author id that's neither an ORCID nor an OpenAlex id, an unknown
    /// language code, a date that isn't one).
    #[uniffi::method(default(since = None))]
    // Exported: UniFFI hands the arguments over by value.
    #[allow(clippy::needless_pass_by_value)]
    pub fn search_supports(
        &self,
        source: Option<String>,
        query: SearchNode,
        since: Option<String>,
    ) -> Option<String> {
        let since = match since.as_deref().map(SearchQuery::parse_since) {
            Some(Err(reason)) => return Some(reason),
            Some(Ok(since)) => Some(since),
            None => None,
        };
        self.engine
            .search_supports_since(source.as_deref(), &query.into(), since)
    }

    /// What each source can search by, in the order `search` picks from:
    /// without a `source`, the first. Offline.
    pub fn search_capabilities(&self) -> Vec<SourceSearch> {
        self.engine
            .search_providers()
            .map(|p| SourceSearch::new(&p.id(), p.capabilities()))
            .collect()
    }
}
