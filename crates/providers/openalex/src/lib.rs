//! OpenAlex provider.
//!
//! Costs (help.openalex.org, 2026-11): single-work lookups are free, list and
//! filter calls $0.0001, searches $0.001, from a free $1/day with an API key.
//! So lookups of one ID use the free singleton endpoint, batches use one
//! `filter` call per identifier kind, and search is the only real expense.
//!
//! Every call reports its cost and remaining budget in [`Usage`] (USD).
//!
//! Works have no filter on their journal's name, so a search by container
//! first finds the sources so named (one more search, $0.001) and keeps
//! works from the best 100 of them.

mod http;
mod tree;
mod wire;

use std::sync::Arc;
use std::time::Duration;

use aspero_core::http::path_segment;
use aspero_core::{
    BooleanSupport, Capabilities, Cursor, Doi, Error, FieldSet, Filter, Hit, HttpClient,
    HttpRequest, HttpResponse, IdKind, Lookup, OpenAlexId, Page, PageReq, Provider, ProviderId,
    QueryNode, Raw, Relation, ResiliencePolicy, Response, Result, SearchMode, SearchQuery, Sort,
    SourceRecord, Usage, WorkId, WorkType,
};
use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};

use crate::wire::WireList;

pub const ID: ProviderId = ProviderId::from_static("openalex");

pub const DEFAULT_BASE_URL: &str = "https://api.openalex.org";

/// Works per batch lookup: as many as one filter ORs.
const MAX_PAGE: u32 = 100;
/// OpenAlex's cap on `per-page` for lists.
const MAX_PER_PAGE: u32 = 200;
/// Most values OpenAlex takes in one filter (`a|b|…`).
const MAX_FILTER_VALUES: usize = 100;

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct OpenAlexConfig {
    /// Without a key OpenAlex allows $0.10/day; with a free key, $1/day.
    pub api_key: Option<SecretString>,
    pub base_url: String,
}

impl Default for OpenAlexConfig {
    fn default() -> Self {
        OpenAlexConfig {
            api_key: None,
            base_url: DEFAULT_BASE_URL.to_owned(),
        }
    }
}

pub struct OpenAlex {
    config: OpenAlexConfig,
    http: Arc<dyn HttpClient>,
    capabilities: Capabilities,
}

impl OpenAlex {
    pub fn new(config: OpenAlexConfig, http: Arc<dyn HttpClient>) -> Self {
        let mut capabilities = Capabilities::default();
        capabilities.lookup_kinds =
            vec![IdKind::Doi, IdKind::Arxiv, IdKind::Pmid, IdKind::OpenAlex];
        capabilities.max_batch = MAX_PAGE as usize;
        capabilities.fields = FieldSet::ALL;
        capabilities.search = true;
        capabilities.search_filters = vec![
            Filter::Years,
            Filter::WorkType,
            Filter::OpenAccess,
            Filter::Place,
            Filter::Author,
            Filter::Title,
            Filter::Container,
            Filter::Keywords,
            Filter::Abstract,
            Filter::AuthorId,
            Filter::NotRetracted,
            Filter::Language,
            Filter::HasAbstract,
        ];
        capabilities.sorts = vec![Sort::Relevance, Sort::Newest, Sort::MostCited];
        // Filters only AND; within one text field a value is a boolean
        // query, and any filter negates (see `tree`).
        capabilities.search_any = BooleanSupport::WithinField;
        capabilities.search_none = BooleanSupport::Anywhere;
        capabilities.search_nesting = true;
        capabilities.relations = vec![Relation::Cites, Relation::CitedBy];
        OpenAlex {
            config,
            http,
            capabilities,
        }
    }

    /// OpenAlex allows 30 req/s with a key; aspero stays well under.
    pub fn default_policy() -> ResiliencePolicy {
        let mut policy = ResiliencePolicy::default();
        policy.min_interval = Duration::from_millis(100);
        policy
    }

    fn request(&self, url: String) -> HttpRequest {
        let req = HttpRequest::get(url).header("accept", "application/json");
        match &self.config.api_key {
            Some(key) => req.header("authorization", format!("Bearer {}", key.expose_secret())),
            None => req,
        }
    }

    fn works_url(&self, rest: &str) -> String {
        format!("{}/works{rest}", self.config.base_url.trim_end_matches('/'))
    }

    /// The IDs of the sources (journals, repositories, …) best matching
    /// `name`, at most as many as one filter can OR.
    /// The sources (journals) so named, best first: with `exact`, only those
    /// named exactly that (ignoring case), if any are ("Nature", not Nature
    /// Communications).
    async fn sources_named(&self, name: &str, exact: Option<&str>) -> Result<(Vec<String>, Usage)> {
        let base = format!("{}/sources", self.config.base_url.trim_end_matches('/'));
        let params = [
            (
                "filter",
                format!("display_name.search:{}", filter_value(name)),
            ),
            ("select", "id,display_name".to_owned()),
            ("per-page", MAX_PAGE.to_string()),
        ];
        let url = url::Url::parse_with_params(&base, &params)
            .map_err(|e| Error::BadRequest(format!("invalid OpenAlex base URL: {e}")))?;
        let (resp, usage) = self.send(url.into()).await?;
        let list: WireList = serde_json::from_slice(&resp.body).map_err(Error::decode)?;
        let named = |v: &serde_json::Value| {
            let shown = v.get("display_name").and_then(|n| n.as_str()).unwrap_or("");
            exact.is_some_and(|e| shown.trim().eq_ignore_ascii_case(e.trim()))
        };
        let exactly: Vec<&serde_json::Value> = list.results.iter().filter(|v| named(v)).collect();
        let chosen = match exactly.is_empty() {
            true => list.results.iter().collect(),
            false => exactly,
        };
        let ids = chosen
            .into_iter()
            .filter_map(|v| v.get("id")?.as_str())
            .map(|id| id.rsplit('/').next().unwrap_or(id).to_owned())
            .collect();
        Ok((ids, usage))
    }

    /// Sends and maps non-2xx statuses to errors. Usage comes back only
    /// with a success; a spent quota's reset time comes in its error.
    async fn send(&self, url: String) -> Result<(HttpResponse, Usage)> {
        let resp = self.http.send(self.request(url)).await?;
        http::check(&resp)?;
        let usage = http::usage(&resp);
        Ok((resp, usage))
    }

    async fn lookup_one(&self, key: &Key) -> Result<(Option<SourceRecord>, Option<Usage>)> {
        let segment = path_segment(&key.path_segment());
        let (resp, usage) = match self.send(self.works_url(&format!("/{segment}"))).await {
            Ok(ok) => ok,
            Err(Error::NotFound) => return Ok((None, None)),
            Err(e) => return Err(e),
        };
        let value: serde_json::Value = serde_json::from_slice(&resp.body).map_err(Error::decode)?;
        let raw = Raw {
            media_type: "application/json".into(),
            body: resp.text(),
        };
        let record = wire::to_record(&value, raw, resp.received_at).map_err(Error::decode)?;
        Ok((Some(record), Some(usage)))
    }

    /// One list call; `params` are already-decoded query pairs.
    async fn list(&self, params: &[(&str, String)]) -> Result<(Vec<Hit>, WireList, Usage)> {
        let url = url::Url::parse_with_params(&self.works_url(""), params)
            .map_err(|e| Error::BadRequest(format!("invalid OpenAlex base URL: {e}")))?;
        let (resp, usage) = self.send(url.into()).await?;
        let list: WireList = serde_json::from_slice(&resp.body).map_err(Error::decode)?;
        let hits = list
            .results
            .iter()
            .filter_map(|v| {
                let score = v.get("relevance_score").and_then(serde_json::Value::as_f64);
                let record = wire::to_record(v, Raw::json(v), resp.received_at);
                or_skip(record).map(|r| Hit::new(r, score))
            })
            .collect();
        Ok((hits, list, usage))
    }
}

/// The record, or `None` with a warning when it doesn't decode: one bad
/// record shouldn't cost the rest of the page.
fn or_skip(record: serde_json::Result<SourceRecord>) -> Option<SourceRecord> {
    record
        .inspect_err(|e| {
            tracing::warn!(
                target: "aspero::decode",
                provider = %ID, error = %e, "skipped a record that doesn't decode"
            );
        })
        .ok()
}

/// How a requested ID is addressed at OpenAlex.
struct Key {
    /// Filter name, also the singleton path prefix.
    filter: &'static str,
    value: String,
    /// The ID a returned record must carry to answer this request.
    matches: WorkId,
}

impl Key {
    fn new(id: &WorkId) -> Result<Key> {
        Ok(match id {
            WorkId::Doi(doi) => Key::doi(doi.clone()),
            // arXiv works are indexed under their DataCite DOI.
            WorkId::Arxiv(arxiv) => Key::doi(arxiv.doi()),
            WorkId::Pmid(pmid) => Key {
                filter: "pmid",
                value: pmid.to_string(),
                matches: id.clone(),
            },
            WorkId::OpenAlex(w) => Key {
                filter: "openalex",
                value: w.to_string(),
                matches: id.clone(),
            },
            _ => {
                return Err(Error::Unsupported(
                    "OpenAlex lookup of this identifier kind",
                ));
            }
        })
    }

    fn doi(doi: Doi) -> Key {
        Key {
            filter: "doi",
            value: doi.as_str().to_owned(),
            matches: WorkId::Doi(doi),
        }
    }

    fn path_segment(&self) -> String {
        match self.filter {
            "openalex" => self.value.clone(),
            filter => format!("{filter}:{}", self.value),
        }
    }

    /// `,` and `|` are filter syntax, so such values can't be OR-ed.
    fn batchable(&self) -> bool {
        !self.value.contains([',', '|'])
    }
}

fn type_filter(t: &WorkType) -> Result<String> {
    let openalex = tree::work_type(t).map_err(Error::BadRequest)?;
    Ok(format!("type:{openalex}"))
}

/// A filter value with the commas and pipes that would split it removed.
fn filter_value(s: &str) -> String {
    s.replace([',', '|'], "")
}

fn respond<T>(value: T, usage: Option<Usage>) -> Response<T> {
    match usage {
        Some(u) => Response::with_usage(value, u),
        None => Response::new(value),
    }
}

#[async_trait]
impl Provider for OpenAlex {
    fn id(&self) -> ProviderId {
        ID
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    async fn lookup(&self, ids: &[WorkId], _fields: FieldSet) -> Result<Response<Vec<Lookup>>> {
        let keys = ids.iter().map(Key::new).collect::<Result<Vec<_>>>()?;
        let mut usage = None;

        if let [key] = keys.as_slice() {
            let (record, u) = self.lookup_one(key).await?;
            let lookup = Lookup {
                id: ids[0].clone(),
                record,
            };
            return Ok(respond(vec![lookup], u));
        }

        // One filter call per kind, in order of first appearance.
        let mut groups: Vec<(&str, Vec<usize>)> = Vec::new();
        let mut singles = Vec::new();
        for (i, key) in keys.iter().enumerate() {
            if !key.batchable() {
                singles.push(i);
            } else if let Some((_, group)) = groups.iter_mut().find(|(f, _)| *f == key.filter) {
                group.push(i);
            } else {
                groups.push((key.filter, vec![i]));
            }
        }

        let mut records: Vec<Option<SourceRecord>> = vec![None; keys.len()];
        let mut add = |u: Usage| usage = Some(usage.map_or(u, |acc: Usage| acc.then(u)));
        for (filter, group) in groups {
            for chunk in group.chunks(MAX_PAGE as usize) {
                let values: Vec<&str> = chunk.iter().map(|&i| keys[i].value.as_str()).collect();
                let params = [
                    ("filter", format!("{filter}:{}", values.join("|"))),
                    ("per-page", MAX_PAGE.to_string()),
                ];
                let (hits, _, u) = self.list(&params).await?;
                add(u);
                let mut unclaimed: Vec<SourceRecord> = Vec::new();
                for record in hits.into_iter().map(|h| h.record) {
                    let mut claimed = false;
                    for &i in chunk {
                        if records[i].is_none() && record.ids.contains(&keys[i].matches) {
                            records[i] = Some(record.clone());
                            claimed = true;
                        }
                    }
                    if !claimed {
                        unclaimed.push(record);
                    }
                }
                // OpenAlex finds works by DOIs their records no longer
                // carry (re-registered when a journal changed publisher):
                // such a record answers a key that found nothing. One of
                // each pairs up; otherwise each such key is asked alone
                // (free), which says which work it is.
                let missed: Vec<usize> = chunk
                    .iter()
                    .copied()
                    .filter(|&i| records[i].is_none())
                    .collect();
                match (missed.as_slice(), unclaimed.len()) {
                    (_, 0) | ([], _) => {}
                    ([i], 1) => records[*i] = unclaimed.pop(),
                    (missed, _) => {
                        for &i in missed {
                            let (record, u) = self.lookup_one(&keys[i]).await?;
                            records[i] = record;
                            if let Some(u) = u {
                                add(u);
                            }
                        }
                    }
                }
            }
        }
        // Asked alone, a record answers its key whatever it carries.
        for i in singles {
            let (record, u) = self.lookup_one(&keys[i]).await?;
            records[i] = record;
            if let Some(u) = u {
                add(u);
            }
        }

        let lookups = ids
            .iter()
            .zip(records)
            .map(|(id, record)| Lookup {
                id: id.clone(),
                record,
            })
            .collect();
        Ok(respond(lookups, usage))
    }

    async fn search(&self, query: &SearchQuery) -> Result<Response<Page>> {
        if query.mode != SearchMode::Keywords {
            return Err(Error::Unsupported("OpenAlex has no citation matcher"));
        }
        if query.since.is_some() {
            return Err(Error::UnsupportedQuery(self.since_unsupported()));
        }
        let mut filters = Vec::new();
        let (search, containers) = match &query.tree {
            Some(tree) => {
                let plan =
                    tree::translate(tree, &self.capabilities).map_err(Error::UnsupportedQuery)?;
                if let Some(id) = unresolved_relation(tree) {
                    return Err(Error::BadRequest(format!(
                        "OpenAlex searches relations by OpenAlex work id, not {id:?}"
                    )));
                }
                filters = plan.filters;
                (plan.search, plan.containers)
            }
            None => {
                flat_filters(query, &mut filters)?;
                let container = query.field(Filter::Container).map(|name| tree::Container {
                    names: vec![(name.to_owned(), false)],
                    negated: false,
                });
                let text = query.text.trim();
                let search = (!text.is_empty()).then(|| text.to_owned());
                (search, container.into_iter().collect())
            }
        };
        let mut usage = None;
        for container in containers {
            // OpenAlex takes at most 100 values in a filter: each journal
            // gets an equal share of its best-matching sources.
            let share = (MAX_FILTER_VALUES / container.names.len().max(1)).max(1);
            let mut sources = Vec::new();
            for (name, phrase) in container.names {
                let exact = phrase.then(|| name.replace('"', ""));
                let name = match &exact {
                    Some(exact) => format!("\"{exact}\""),
                    None => name,
                };
                let (found, u) = self.sources_named(&name, exact.as_deref()).await?;
                usage = Some(usage.map_or(u, |s: Usage| s.then(u)));
                for source in found.into_iter().take(share) {
                    if !sources.contains(&source) {
                        sources.push(source);
                    }
                }
            }
            let spent = usage.unwrap_or_default();
            match (sources.is_empty(), container.negated) {
                // No source so named: nothing to find (or to leave out).
                (true, false) => {
                    return Ok(Response::with_usage(
                        Page::new(Vec::new(), Some(0), None),
                        spent,
                    ));
                }
                (true, true) => {}
                (false, negated) => filters.push(format!(
                    "primary_location.source.id:{}{}",
                    if negated { "!" } else { "" },
                    sources.join("|")
                )),
            }
        }

        let mut params: Vec<(&str, String)> = Vec::new();
        // Relevance is the default order whenever there is text to rank by;
        // asking for it without any is an error.
        let sort = match query.sort {
            Sort::Relevance => None,
            Sort::Newest => Some("publication_date:desc"),
            Sort::MostCited => Some("cited_by_count:desc"),
            _ => return Err(Error::Unsupported("OpenAlex sort")),
        };
        if let Some(sort) = sort.filter(|_| !query.extra.contains_key("sort")) {
            params.push(("sort", sort.to_owned()));
        }
        for (key, value) in &query.extra {
            match key.as_str() {
                "filter" => filters.push(value.clone()),
                "sort" => params.push(("sort", value.clone())),
                // The fields to return, as citation links need (`id,referenced_works`).
                "select" => params.push(("select", value.clone())),
                other => {
                    return Err(Error::BadRequest(format!(
                        "unknown OpenAlex parameter {other:?} (supported: filter, select, sort)"
                    )));
                }
            }
        }
        if let Some(search) = search {
            params.push(("search", search));
        }
        if !filters.is_empty() {
            params.push(("filter", filters.join(",")));
        }
        params.extend(page_params(&query.page));

        let (hits, list, u) = self.list(&params).await?;
        let page = Page::new(hits, list.meta.count, list.meta.next_cursor.map(Cursor));
        Ok(Response::with_usage(
            page,
            usage.map_or(u, |s: Usage| s.then(u)),
        ))
    }

    fn express(&self, tree: &QueryNode) -> std::result::Result<(), String> {
        tree::translate(tree, &self.capabilities).map(|_| ())
    }

    /// `from_created_date` and `sort=created_date` are "daily sync", for
    /// Member+ and Partner plans: a free key gets "Plan upgrade required"
    /// (checked live 2026-11-13).
    fn since_unsupported(&self) -> String {
        "OpenAlex can't search by date added (it needs a paid plan)".into()
    }
}

/// A search's filters without a tree: years, type, open access, place,
/// author and title. A container needs a source lookup first.
fn flat_filters(query: &SearchQuery, filters: &mut Vec<String>) -> Result<()> {
    if let Some(years) = query.years {
        if let Some(from) = years.from {
            filters.push(format!("from_publication_date:{from:04}-01-01"));
        }
        if let Some(to) = years.to {
            filters.push(format!("to_publication_date:{to:04}-12-31"));
        }
    }
    if let Some(t) = &query.work_type {
        filters.push(type_filter(t)?);
    }
    if let Some(oa) = query.open_access {
        filters.push(format!("is_oa:{oa}"));
    }
    if let Some(place) = &query.place {
        filters.push(format!("biblio.volume:{}", filter_value(&place.volume)));
        filters.push(format!(
            "biblio.first_page:{}",
            filter_value(&place.first_page)
        ));
    }
    if let Some(author) = query.field(Filter::Author) {
        filters.push(format!("raw_author_name.search:{}", filter_value(author)));
    }
    if let Some(title) = query.field(Filter::Title) {
        filters.push(format!("title.search:{}", filter_value(title)));
    }
    Ok(())
}

/// A relation in `tree` whose work isn't named by an OpenAlex work id:
/// the engine resolves those before searching.
fn unresolved_relation(tree: &QueryNode) -> Option<&str> {
    tree.leaves().into_iter().find_map(|leaf| match leaf {
        QueryNode::Relation(r) if r.id.parse::<OpenAlexId>().is_err() => Some(r.id.as_str()),
        _ => None,
    })
}

/// Cursor paging (needs a key; unlimited depth, unlike `page=`).
fn page_params(page: &PageReq) -> [(&'static str, String); 2] {
    let cursor = page.cursor.as_ref().map_or("*", |c| c.0.as_str());
    [
        ("per-page", page.limit.clamp(1, MAX_PER_PAGE).to_string()),
        ("cursor", cursor.to_owned()),
    ]
}
