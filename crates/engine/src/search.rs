//! Searching one provider.

use std::sync::Arc;

use aspero_core::{
    Error, Filter, Page, Provider, ProviderId, SearchMode, SearchQuery, Sort, Usage,
};

use crate::{Engine, EngineError, Served};

#[derive(Debug, Clone)]
pub struct SearchReport {
    pub provider: ProviderId,
    pub page: Page,
    /// Query filters the provider can't apply; they were not sent. Fields
    /// (author, title, container) are never ignored: a provider lacking
    /// one fails.
    pub ignored_filters: Vec<Filter>,
    pub usage: Option<Usage>,
    pub served: Served,
}

impl Engine {
    /// Providers that search by keywords, in the order [`search`](Self::search)
    /// picks from: without a `source`, the first.
    pub fn search_providers(&self) -> impl Iterator<Item = &Arc<dyn Provider>> {
        self.providers().filter(|p| p.capabilities().search)
    }

    /// Searches one provider: `source` if given, otherwise the first that
    /// supports the query's mode. Fields and sorts it lacks fail with
    /// [`Error::Unsupported`]; other filters it lacks are ignored.
    pub async fn search(
        &self,
        source: Option<&str>,
        query: &SearchQuery,
    ) -> Result<SearchReport, EngineError> {
        let citation = query.mode == SearchMode::Citation;
        // Keywords, a field, (verifying) a place, or an expert's own
        // parameters (an OpenAlex `filter` alone): something to find.
        let fields = [Filter::Author, Filter::Title, Filter::Container];
        if query.text.trim().is_empty()
            && query.place.is_none()
            && query.extra.is_empty()
            && !fields.iter().any(|f| query.field(*f).is_some())
        {
            return Err(EngineError::NothingToSearch);
        }
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

        let (missing_fields, ignored_filters): (Vec<Filter>, Vec<Filter>) = query
            .filters()
            .into_iter()
            .filter(|f| !caps.search_filters.contains(f))
            .partition(|f| f.is_field());
        if let Some(field) = missing_fields.first() {
            return Err(refuse(match field {
                Filter::Author => "search by author",
                Filter::Title => "search by title",
                _ => "search by container",
            }));
        }
        if query.sort != Sort::Relevance && !caps.sorts.contains(&query.sort) {
            return Err(refuse(match query.sort {
                Sort::Newest => "sorting newest first",
                _ => "sorting most cited first",
            }));
        }
        let mut sent = query.clone();
        // Newest first, records dated ahead (deposits, issues printed early,
        // typos like 2115) would lead: unless the caller says otherwise,
        // newest means up to this year.
        if sent.sort == Sort::Newest
            && caps.search_filters.contains(&Filter::Years)
            && sent.years.is_none_or(|y| y.to.is_none())
        {
            let year = (self.clock)().to_zoned(jiff::tz::TimeZone::UTC).year();
            let mut years = sent.years.unwrap_or_default();
            years.to = Some(year);
            sent.years = Some(years);
        }
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
