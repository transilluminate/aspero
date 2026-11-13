//! Crossref provider.
//!
//! Crossref is the registration agency for most journal and conference DOIs,
//! so it is authoritative for their bibliographic fields. Its
//! `query.bibliographic` matcher, built for reference strings, backs
//! [`SearchMode::Citation`].
//!
//! It only knows Crossref DOIs: arXiv's DOIs are registered with DataCite,
//! so preprints such as arXiv:1706.03762 are absent here.
//!
//! Free to use. Requests carry a `mailto`, which puts them in the "polite"
//! pool (observed 2026-11: 10 req/s for single works, 3 req/s for lists, 3
//! concurrent).

mod http;
mod wire;

use std::sync::Arc;
use std::time::Duration;

use aspero_core::http::path_segment;
use aspero_core::{
    Capabilities, Cursor, Doi, Error, Field, FieldSet, Filter, Hit, HttpClient, HttpRequest,
    HttpResponse, IdKind, Lookup, Page, Provider, ProviderId, ResiliencePolicy, Response, Result,
    SearchMode, SearchQuery, Sort, SourceRecord, WorkId,
};
use async_trait::async_trait;

use crate::wire::{Envelope, WireList};

pub const ID: ProviderId = ProviderId::from_static("crossref");

pub const DEFAULT_BASE_URL: &str = "https://api.crossref.org";

/// DOIs per batched `filter=doi:…,doi:…` call; keeps URLs a sane length.
const MAX_BATCH: usize = 50;

/// Crossref's cap on `rows`.
const MAX_ROWS: u32 = 1000;

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CrossrefConfig {
    /// Contact address for the polite pool. Without it requests share the
    /// slower public pool.
    pub mailto: Option<String>,
    pub base_url: String,
}

impl Default for CrossrefConfig {
    fn default() -> Self {
        CrossrefConfig {
            mailto: None,
            base_url: DEFAULT_BASE_URL.to_owned(),
        }
    }
}

pub struct Crossref {
    config: CrossrefConfig,
    http: Arc<dyn HttpClient>,
    capabilities: Capabilities,
}

impl Crossref {
    pub fn new(config: CrossrefConfig, http: Arc<dyn HttpClient>) -> Self {
        let mut capabilities = Capabilities::default();
        capabilities.lookup_kinds = vec![IdKind::Doi];
        capabilities.max_batch = MAX_BATCH;
        // Records say nothing about free copies.
        capabilities.fields = FieldSet::ALL.difference(FieldSet::of(&[Field::OpenAccess]));
        capabilities.search = true;
        capabilities.citation_search = true;
        capabilities.notices = true;
        capabilities.search_filters = vec![
            Filter::Years,
            Filter::WorkType,
            Filter::Author,
            Filter::Title,
            Filter::Container,
        ];
        capabilities.sorts = vec![Sort::Relevance, Sort::Newest, Sort::MostCited];
        Crossref {
            config,
            http,
            capabilities,
        }
    }

    /// Paced for the polite pool's list limit (3 req/s), the stricter of
    /// its two.
    pub fn default_policy() -> ResiliencePolicy {
        let mut policy = ResiliencePolicy::default();
        policy.min_interval = Duration::from_millis(350);
        policy
    }

    /// `/works<rest>` with `params` and the `mailto` appended.
    fn url(&self, rest: &str, params: &[(&str, String)]) -> Result<String> {
        let base = format!("{}/works{rest}", self.config.base_url.trim_end_matches('/'));
        let mut url = url::Url::parse(&base)
            .map_err(|e| Error::BadRequest(format!("invalid Crossref base URL: {e}")))?;
        {
            let mut query = url.query_pairs_mut();
            for (k, v) in params {
                query.append_pair(k, v);
            }
            if let Some(mailto) = &self.config.mailto {
                query.append_pair("mailto", mailto);
            }
        }
        // An empty query would leave a dangling `?`.
        if url.query() == Some("") {
            url.set_query(None);
        }
        Ok(url.into())
    }

    async fn get(&self, url: String) -> Result<HttpResponse> {
        let req = HttpRequest::get(url).header("accept", "application/json");
        let resp = self.http.send(req).await?;
        http::check(&resp)?;
        Ok(resp)
    }

    async fn lookup_one(&self, doi: &Doi) -> Result<Option<SourceRecord>> {
        let url = self.url(&format!("/{}", path_segment(doi.as_str())), &[])?;
        let resp = match self.get(url).await {
            Ok(resp) => resp,
            Err(Error::NotFound) => return Ok(None),
            Err(e) => return Err(e),
        };
        let envelope: Envelope<serde_json::Value> =
            serde_json::from_slice(&resp.body).map_err(Error::decode)?;
        wire::to_record(&envelope.message, resp.received_at)
            .map(Some)
            .map_err(Error::decode)
    }

    async fn list(&self, params: &[(&str, String)]) -> Result<(Vec<Hit>, WireList)> {
        let resp = self.get(self.url("", params)?).await?;
        let envelope: Envelope<WireList> =
            serde_json::from_slice(&resp.body).map_err(Error::decode)?;
        let list = envelope.message;
        let hits = list
            .items
            .iter()
            .filter_map(|v| {
                let score = v.get("score").and_then(serde_json::Value::as_f64);
                or_skip(wire::to_record(v, resp.received_at)).map(|r| Hit::new(r, score))
            })
            .collect();
        Ok((hits, list))
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

#[async_trait]
impl Provider for Crossref {
    fn id(&self) -> ProviderId {
        ID
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    async fn lookup(&self, ids: &[WorkId], _fields: FieldSet) -> Result<Response<Vec<Lookup>>> {
        let dois = ids
            .iter()
            .map(|id| match id {
                WorkId::Doi(doi) => Ok(doi),
                _ => Err(Error::Unsupported("Crossref lookup of non-DOI identifiers")),
            })
            .collect::<Result<Vec<_>>>()?;

        if let [doi] = dois.as_slice() {
            let record = self.lookup_one(doi).await?;
            return Ok(Response::new(vec![Lookup {
                id: ids[0].clone(),
                record,
            }]));
        }

        // `,` separates filters, so such DOIs can't be batched.
        let (batchable, singles): (Vec<&Doi>, Vec<&Doi>) =
            dois.iter().partition(|d| !d.as_str().contains(','));
        let mut found = Vec::new();
        for chunk in batchable.chunks(MAX_BATCH) {
            let filter = chunk
                .iter()
                .map(|d| format!("doi:{}", d.as_str()))
                .collect::<Vec<_>>()
                .join(",");
            let params = [("filter", filter), ("rows", chunk.len().to_string())];
            let (hits, _) = self.list(&params).await?;
            found.extend(hits.into_iter().map(|h| h.record));
        }
        for doi in singles {
            found.extend(self.lookup_one(doi).await?);
        }

        let lookups = ids
            .iter()
            .map(|id| Lookup {
                id: id.clone(),
                record: found.iter().find(|r| r.ids.contains(id)).cloned(),
            })
            .collect();
        Ok(Response::new(lookups))
    }

    async fn search(&self, query: &SearchQuery) -> Result<Response<Page>> {
        let mut filters = Vec::new();
        if let Some(years) = query.years {
            if let Some(from) = years.from {
                filters.push(format!("from-pub-date:{from:04}"));
            }
            if let Some(to) = years.to {
                filters.push(format!("until-pub-date:{to:04}"));
            }
        }
        if let Some(t) = &query.work_type {
            let crossref = wire::crossref_type(t).ok_or_else(|| {
                Error::BadRequest(format!(
                    "Crossref has no work type matching {:?}",
                    t.as_csl()
                ))
            })?;
            filters.push(format!("type:{crossref}"));
        }

        let mut params: Vec<(&str, String)> = Vec::new();
        for (key, value) in &query.extra {
            match key.as_str() {
                "filter" => filters.push(value.clone()),
                "sort" | "order" => params.push((key.as_str(), value.clone())),
                k if k.starts_with("query.") => params.push((k, value.clone())),
                other => {
                    return Err(Error::BadRequest(format!(
                        "unknown Crossref parameter {other:?} (supported: filter, sort, order, query.*)"
                    )));
                }
            }
        }
        let text = query.text.trim();
        if !text.is_empty() {
            let field = match query.mode {
                SearchMode::Citation => "query.bibliographic",
                _ => "query",
            };
            params.push((field, text.to_owned()));
        }
        // Field queries. `query.title` matches titles only, unlike
        // `query.bibliographic` (Swagger docs; checked live 2026-11-13).
        let fields = [
            (Filter::Author, "query.author"),
            (Filter::Title, "query.title"),
            (Filter::Container, "query.container-title"),
        ];
        for (field, param) in fields {
            if let Some(value) = query.field(field) {
                params.push((param, value.to_owned()));
            }
        }
        if !filters.is_empty() {
            params.push(("filter", filters.join(",")));
        }
        // Cursor paging silently drops relevance order (observed 2026-11-10:
        // the right match at 92.1 vanished from the top 3), so ask for it
        // explicitly whenever there is text to rank by.
        let sort = match query.sort {
            Sort::Relevance => {
                let has_query = params.iter().any(|(k, _)| k.starts_with("query"));
                has_query.then_some("score")
            }
            Sort::Newest => Some("published"),
            Sort::MostCited => Some("is-referenced-by-count"),
            _ => return Err(Error::Unsupported("Crossref sort")),
        };
        if let Some(sort) = sort.filter(|_| !params.iter().any(|(k, _)| *k == "sort")) {
            params.push(("sort", sort.into()));
            params.push(("order", "desc".into()));
        }
        params.push(("rows", query.page.limit.clamp(1, MAX_ROWS).to_string()));
        let cursor = query.page.cursor.as_ref().map_or("*", |c| c.0.as_str());
        params.push(("cursor", cursor.to_owned()));

        let (hits, list) = self.list(&params).await?;
        // Crossref keeps returning a cursor after the last page; an empty
        // page is the real end.
        let next = list.next_cursor.filter(|_| !hits.is_empty()).map(Cursor);
        Ok(Response::new(Page::new(hits, list.total_results, next)))
    }
}
