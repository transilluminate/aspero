//! Searching one provider.

use std::sync::Arc;

use aspero_core::{
    Error, FieldSet, Filter, FilterValue, IdKind, Page, Provider, ProviderId, QueryNode,
    SearchMode, SearchQuery, Sort, Usage, WorkId, YearRange,
};
use jiff::civil::Date;

use crate::{Engine, EngineError, Entry, Served};

#[derive(Debug, Clone)]
pub struct SearchReport {
    pub provider: ProviderId,
    pub page: Page,
    /// Query filters the provider can't apply; they were not sent. Fields
    /// (author, title, container) are never ignored: a provider lacking
    /// one fails. Nor is anything in a query tree.
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

    /// Whether `source` (or, without one, the provider [`search`](Self::search)
    /// would pick) can search by `tree`: `None` if so, otherwise why, in
    /// English. Offline and instant; searching anyway fails with the same
    /// reason. Relations' works aren't looked up, so one that doesn't exist
    /// fails only when searched.
    pub fn search_supports(&self, source: Option<&str>, tree: &QueryNode) -> Option<String> {
        self.search_supports_since(source, tree, None)
    }

    /// [`search_supports`](Self::search_supports) for a search that also
    /// asks for only works added since a date
    /// ([`SearchQuery::since`]): without a `source`, the provider
    /// [`search`](Self::search) would pick for it.
    pub fn search_supports_since(
        &self,
        source: Option<&str>,
        tree: &QueryNode,
        since: Option<Date>,
    ) -> Option<String> {
        let result = self
            .search_entry(source, false, since.is_some())
            .and_then(|entry| {
                check_since(entry, since)?;
                self.prepare_tree(entry, tree.clone(), None)
            });
        match result {
            Ok(Some(_)) => None,
            Ok(None) => Some(EngineError::NothingToSearch.to_string()),
            Err(EngineError::Provider {
                error: Error::UnsupportedQuery(reason),
                ..
            }) => Some(reason),
            Err(e) => Some(e.to_string()),
        }
    }

    /// Searches one provider: `source` if given, otherwise the first that
    /// supports the query's mode (and [`SearchQuery::since`], if set).
    /// Fields and sorts it lacks fail with [`Error::Unsupported`]; other
    /// filters it lacks are ignored, but for `since`. A query tree it
    /// can't say, or a `since` it can't apply, fails with
    /// [`Error::UnsupportedQuery`], as
    /// [`search_supports`](Self::search_supports) tells.
    pub async fn search(
        &self,
        source: Option<&str>,
        query: &SearchQuery,
    ) -> Result<SearchReport, EngineError> {
        let citation = query.mode == SearchMode::Citation;
        let fields = [Filter::Author, Filter::Title, Filter::Container];
        let has_field = fields.iter().any(|f| query.field(*f).is_some());
        if query.tree.is_some() {
            if !query.text.trim().is_empty() || has_field || query.place.is_some() {
                return Err(EngineError::InvalidQuery(
                    "a query replaces the keywords, author, title and container: give one or \
                     the other"
                        .into(),
                ));
            }
            if citation {
                return Err(EngineError::InvalidQuery(
                    "a citation search takes a reference, not a query".into(),
                ));
            }
        } else if query.text.trim().is_empty()
            && query.place.is_none()
            && query.extra.is_empty()
            && !has_field
        {
            // Keywords, a field, (verifying) a place, or an expert's own
            // parameters (an OpenAlex `filter` alone): something to find.
            return Err(EngineError::NothingToSearch);
        }
        let entry = self.search_entry(source, citation, query.since.is_some())?;
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
        // Never ignored: a watch would see old works as new.
        check_since(entry, query.since)?;

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
        for filter in &ignored_filters {
            match filter {
                Filter::Years => sent.years = None,
                Filter::WorkType => sent.work_type = None,
                Filter::OpenAccess => sent.open_access = None,
                Filter::Place => sent.place = None,
                _ => {}
            }
        }
        // Newest first, records dated ahead (deposits, issues printed early,
        // typos like 2115) would lead: unless the caller says otherwise,
        // newest means up to this year.
        let cap = (sent.sort == Sort::Newest && caps.search_filters.contains(&Filter::Years))
            .then(|| (self.clock)().to_zoned(jiff::tz::TimeZone::UTC).year());
        let mut lookup_usage = Vec::new();
        if let Some(tree) = sent.tree.take() {
            // The request's own filters join the tree, as more of all of it.
            let filters = [
                sent.years.take().map(FilterValue::Years),
                sent.work_type.take().map(FilterValue::WorkType),
                sent.open_access.take().map(FilterValue::OpenAccess),
            ];
            let mut all = vec![tree];
            all.extend(filters.into_iter().flatten().map(QueryNode::Filter));
            match self.prepare_tree(entry, QueryNode::All(all), cap)? {
                Some(mut tree) => {
                    lookup_usage = self.resolve_relations(&mut tree).await?;
                    sent.tree = Some(tree);
                }
                None if sent.extra.is_empty() => return Err(EngineError::NothingToSearch),
                None => {}
            }
        } else if let Some(year) = cap
            && sent.years.is_none_or(|y| y.to.is_none())
        {
            let mut years = sent.years.unwrap_or_default();
            years.to = Some(year);
            sent.years = Some(years);
        }
        let lookup_usage = lookup_usage
            .into_iter()
            .find(|(p, _)| *p == provider)
            .map(|(_, u)| u);

        let report = |page, usage: Option<Usage>, served| SearchReport {
            provider: provider.clone(),
            page,
            ignored_filters: ignored_filters.clone(),
            usage: match (lookup_usage, usage) {
                (Some(l), Some(u)) => Some(l.then(u)),
                (l, u) => l.or(u),
            },
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

    /// The provider named `source`, or the first that searches (by citation,
    /// and by date added, if asked).
    fn search_entry(
        &self,
        source: Option<&str>,
        citation: bool,
        since: bool,
    ) -> Result<&Entry, EngineError> {
        match source {
            Some(name) => self
                .entries
                .iter()
                .find(|e| e.provider.id().as_str() == name)
                .ok_or_else(|| EngineError::UnknownProvider(name.to_owned())),
            None => self
                .entries
                .iter()
                .find(|e| {
                    let caps = e.provider.capabilities();
                    caps.search
                        && (!citation || caps.citation_search)
                        && (!since || caps.search_filters.contains(&Filter::Since))
                })
                .ok_or(EngineError::NoCapableProvider(match (citation, since) {
                    (true, false) => "citation search",
                    (true, true) => "citation search by date added",
                    (false, true) => "search by date added",
                    (false, false) => "search",
                })),
        }
    }

    /// A tree as `entry` will search it: normalised, its leaves checked,
    /// capped at `newest_cap` (see [`cap_years`]), and accepted by the
    /// provider. `None` when nothing is left to search by. The one
    /// decision behind both [`search`](Self::search) and
    /// [`search_supports`](Self::search_supports).
    fn prepare_tree(
        &self,
        entry: &Entry,
        tree: QueryNode,
        newest_cap: Option<i16>,
    ) -> Result<Option<QueryNode>, EngineError> {
        let provider = entry.provider.id();
        if !entry.provider.capabilities().search {
            return Err(EngineError::Provider {
                provider,
                error: Error::Unsupported("search"),
            });
        }
        let Some(tree) = tree.normalised() else {
            return Ok(None);
        };
        tree.validate().map_err(EngineError::InvalidQuery)?;
        let tree = match newest_cap {
            Some(year) => cap_years(tree, year),
            None => tree,
        };
        entry
            .provider
            .express(&tree)
            .map_err(|reason| EngineError::Provider {
                provider,
                error: Error::UnsupportedQuery(reason),
            })?;
        Ok(Some(tree))
    }

    /// Replaces each relation's identifier with its work's OpenAlex id, the
    /// one id a relation is searched by. Usage of the lookups is returned.
    async fn resolve_relations(
        &self,
        tree: &mut QueryNode,
    ) -> Result<Vec<(ProviderId, Usage)>, EngineError> {
        let mut usage = Vec::new();
        let mut pending = Vec::new();
        relation_ids(tree, &mut pending);
        for id in pending {
            let resolved = self.openalex_work(id, &mut usage).await?;
            *id = resolved;
        }
        Ok(usage)
    }

    /// The OpenAlex work id of the work `id` names: as given, or looked up
    /// (through the cache), and if the record found carries no OpenAlex id,
    /// looked up again by its DOI or PMID.
    pub(crate) async fn openalex_work(
        &self,
        id: &str,
        usage: &mut Vec<(ProviderId, Usage)>,
    ) -> Result<String, EngineError> {
        let unresolved = |reason: String, kind| EngineError::Unresolved {
            id: id.to_owned(),
            reason,
            kind,
        };
        let work: WorkId = id.parse().map_err(|e: aspero_core::IdError| {
            unresolved(e.to_string(), aspero_core::ErrorKind::InvalidId)
        })?;
        if let WorkId::OpenAlex(w) = &work {
            return Ok(w.to_string());
        }
        let mut asked = vec![work.clone()];
        let mut next = work;
        loop {
            let report = self
                .lookup(std::slice::from_ref(&next), FieldSet::DEFAULT)
                .await;
            for (provider, u) in &report.usage {
                crate::add_usage(usage, provider, *u);
            }
            let result = &report.results[0];
            let Some(record) = &result.record else {
                if let Some(reason) = result.unsupported() {
                    return Err(unresolved(reason, aspero_core::ErrorKind::Unsupported));
                }
                if result.is_not_found() {
                    return Err(unresolved(
                        "no such work found".into(),
                        aspero_core::ErrorKind::NotFound,
                    ));
                }
                let failed = result.attempts.iter().find_map(|a| match &a.outcome {
                    crate::Outcome::Failed(e) => Some(e),
                    _ => None,
                });
                return Err(match failed {
                    Some(e) => unresolved(format!("the lookup failed: {e}"), e.kind()),
                    None => unresolved(
                        "no such work found".into(),
                        aspero_core::ErrorKind::NotFound,
                    ),
                });
            };
            if let Some(w) = record.ids.iter().find_map(|i| match i {
                WorkId::OpenAlex(w) => Some(w),
                _ => None,
            }) {
                return Ok(w.to_string());
            }
            // Found elsewhere (Europe PMC for a PMCID): ask again by an
            // identifier OpenAlex looks up.
            match record
                .ids
                .iter()
                .find(|i| matches!(i.kind(), IdKind::Doi | IdKind::Pmid) && !asked.contains(i))
            {
                Some(other) => {
                    asked.push(other.clone());
                    next = other.clone();
                }
                None => {
                    return Err(unresolved(
                        "OpenAlex has no record of it".into(),
                        aspero_core::ErrorKind::NotFound,
                    ));
                }
            }
        }
    }

    /// A cached page and whether it is still fresh.
    pub(crate) fn cached_search(
        &self,
        provider: &ProviderId,
        query: &SearchQuery,
    ) -> Option<(Page, bool)> {
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

/// Refuses a `since` that `entry` can't apply, saying why
/// ([`Provider::since_unsupported`]).
fn check_since(entry: &Entry, since: Option<Date>) -> Result<(), EngineError> {
    let caps = entry.provider.capabilities();
    match since.is_some() && !caps.search_filters.contains(&Filter::Since) {
        true => Err(EngineError::Provider {
            provider: entry.provider.id(),
            error: Error::UnsupportedQuery(entry.provider.since_unsupported()),
        }),
        false => Ok(()),
    }
}

/// Every relation's identifier in `tree`, to resolve in place.
fn relation_ids<'a>(tree: &'a mut QueryNode, out: &mut Vec<&'a mut String>) {
    match tree {
        QueryNode::All(nodes) | QueryNode::Any(nodes) | QueryNode::None(nodes) => {
            nodes.iter_mut().for_each(|n| relation_ids(n, out));
        }
        QueryNode::Relation(r) => out.push(&mut r.id),
        _ => {}
    }
}

/// Newest first, a tree searches up to `year` unless its own years say
/// how far: a years filter that always applies (the tree itself, or one of
/// its top-level all) with an upper year leaves the tree as it is; one
/// with only a first year gains `year` as its last; otherwise the tree is
/// ANDed with "up to `year`". Years under any or none don't count: they
/// don't bound every hit.
pub(crate) fn cap_years(tree: QueryNode, year: i16) -> QueryNode {
    let mut all = match tree {
        QueryNode::All(nodes) => nodes,
        other => vec![other],
    };
    let years = |n: &QueryNode| matches!(n, QueryNode::Filter(FilterValue::Years(_)));
    let bounded = all
        .iter()
        .any(|n| matches!(n, QueryNode::Filter(FilterValue::Years(y)) if y.to.is_some()));
    if !bounded {
        match all.iter_mut().find(|n| years(n)) {
            Some(QueryNode::Filter(FilterValue::Years(y))) => y.to = Some(year),
            _ => all.push(QueryNode::Filter(FilterValue::Years(YearRange {
                from: None,
                to: Some(year),
            }))),
        }
    }
    match all.len() {
        1 => all.pop().expect("one node"),
        _ => QueryNode::All(all),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aspero_core::TermField;

    fn years(from: Option<i16>, to: Option<i16>) -> QueryNode {
        QueryNode::Filter(FilterValue::Years(YearRange { from, to }))
    }

    #[test]
    fn newest_caps_trees_unless_their_years_say_otherwise() {
        let title = QueryNode::term(TermField::Title, "x");
        assert_eq!(
            cap_years(title.clone(), 2026),
            QueryNode::All(vec![title.clone(), years(None, Some(2026))])
        );
        // A first year alone gains this year as its last.
        let from = QueryNode::All(vec![title.clone(), years(Some(2017), None)]);
        assert_eq!(
            cap_years(from, 2026),
            QueryNode::All(vec![title.clone(), years(Some(2017), Some(2026))])
        );
        // An upper year of its own stands, even a later one.
        let to = QueryNode::All(vec![title.clone(), years(None, Some(2030))]);
        assert_eq!(cap_years(to.clone(), 2026), to);
        assert_eq!(
            cap_years(years(None, Some(2030)), 2026),
            years(None, Some(2030))
        );
        // Years under none don't bound every hit.
        let none = QueryNode::None(vec![years(None, Some(2000))]);
        assert_eq!(
            cap_years(none.clone(), 2026),
            QueryNode::All(vec![none, years(None, Some(2026))])
        );
    }
}
