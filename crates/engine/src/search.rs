//! Searching one provider.

use aspero_core::{Error, Filter, Page, ProviderId, SearchMode, SearchQuery, Usage};

use crate::{Engine, EngineError, Served};

#[derive(Debug, Clone)]
pub struct SearchReport {
    pub provider: ProviderId,
    pub page: Page,
    /// Query filters the provider can't apply; they were not sent.
    pub ignored_filters: Vec<Filter>,
    pub usage: Option<Usage>,
    pub served: Served,
}

impl Engine {
    /// Searches one provider: `source` if given, otherwise the first that
    /// supports the query's mode.
    pub async fn search(
        &self,
        source: Option<&str>,
        query: &SearchQuery,
    ) -> Result<SearchReport, EngineError> {
        let citation = query.mode == SearchMode::Citation;
        let entry = match source {
            Some(name) => self
                .entries
                .iter()
                .find(|e| e.provider.id().as_str() == name)
                .ok_or_else(|| EngineError::UnknownProvider(name.to_owned()))?,
            None => self
                .entries
                .iter()
                .find(|e| {
                    let caps = e.provider.capabilities();
                    caps.search && (!citation || caps.citation_search)
                })
                .ok_or(EngineError::NoCapableProvider(if citation {
                    "citation search"
                } else {
                    "search"
                }))?,
        };
        let provider = entry.provider.id();
        let caps = entry.provider.capabilities();
        let refuse = |what| EngineError::Provider {
            provider: provider.clone(),
            error: Error::Unsupported(what),
        };
        if !caps.search {
            return Err(refuse("search"));
        }
        if citation && !caps.citation_search {
            return Err(refuse("citation search"));
        }

        let ignored_filters: Vec<Filter> = query
            .filters()
            .into_iter()
            .filter(|f| !caps.search_filters.contains(f))
            .collect();
        let mut sent = query.clone();
        for filter in &ignored_filters {
            match filter {
                Filter::Years => sent.years = None,
                Filter::WorkType => sent.work_type = None,
                Filter::OpenAccess => sent.open_access = None,
                Filter::Place => sent.place = None,
                _ => {}
            }
        }

        let report = |page, usage, served| SearchReport {
            provider: provider.clone(),
            page,
            ignored_filters: ignored_filters.clone(),
            usage,
            served,
        };
        let cached = self.cached_search(&provider, &sent);
        if self.options.offline {
            return match cached {
                Some((page, fresh)) => Ok(report(
                    page,
                    None,
                    if fresh { Served::Cache } else { Served::Stale },
                )),
                None => Err(EngineError::Provider {
                    provider: provider.clone(),
                    error: Error::Offline,
                }),
            };
        }
        if let Some((page, true)) = cached {
            tracing::info!(target: "aspero::cache", provider = %provider, text = %sent.text, "search served from cache");
            return Ok(report(page, None, Served::Cache));
        }
        match self.call(entry, || entry.provider.search(&sent)).await {
            Ok(resp) => {
                self.remember_search(&provider, &sent, &resp.value);
                Ok(report(resp.value, resp.usage, Served::Live))
            }
            Err(error) => match cached {
                Some((page, _)) => Ok(report(page, None, Served::Stale)),
                None => Err(EngineError::Provider {
                    provider: provider.clone(),
                    error,
                }),
            },
        }
    }

    /// A cached page and whether it is still fresh.
    fn cached_search(&self, provider: &ProviderId, query: &SearchQuery) -> Option<(Page, bool)> {
        let entry = self.cache.as_ref()?.get_search(provider, query).ok()??;
        let fresh = entry.is_fresh((self.clock)(), self.cache_policy.searches);
        Some((entry.value, fresh))
    }

    fn remember_search(&self, provider: &ProviderId, query: &SearchQuery, page: &Page) {
        if let Some(cache) = &self.cache {
            let _ = cache.put_search(provider, query, page, (self.clock)());
        }
    }
}
