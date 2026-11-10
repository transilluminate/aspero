//! DataCite provider.
//!
//! DataCite registers DOIs for arXiv, Zenodo, Figshare, Dryad and most
//! research data and software. Its REST API (JSON:API) gives lookups,
//! batched lookups, and keyword search with year and type filters; records
//! carry abstracts (as "Abstract" descriptions) and stated arXiv IDs. Free;
//! about 3,000 requests per 5 minutes.
//!
//! Observed 2026-11-10: **relevance ranking only works with page-number
//! paging** (`sort=relevance&page[number]=N`); with cursor paging results
//! come back by creation date regardless. So the provider's [`Cursor`] is
//! the next page number. Year ranges use the query syntax
//! (`publicationYear:[2017 TO *]`); `published=` only takes year lists.

mod wire;

use std::sync::Arc;
use std::time::Duration;

use aspero_core::http::path_segment;
use aspero_core::{
    Capabilities, Cursor, Doi, Error, FieldSet, Filter, Hit, HttpClient, HttpRequest, HttpResponse,
    IdKind, Lookup, Page, Provider, ProviderId, ResiliencePolicy, Response, Result, SearchMode,
    SearchQuery, SourceRecord, WorkId,
};
use async_trait::async_trait;

use crate::wire::WireList;

pub const ID: ProviderId = ProviderId::from_static("datacite");

pub const DEFAULT_BASE_URL: &str = "https://api.datacite.org";

pub(crate) const ARXIV_DOI_PREFIX: &str = "10.48550";

/// DOIs per batched `query=doi:(…)` lookup.
const MAX_BATCH: usize = 50;

/// DataCite's page-size cap.
const MAX_PAGE: u32 = 1000;

/// Extra parameters passed through to DataCite's search filters.
const PASSTHROUGH: &[&str] = &["client-id", "prefix", "affiliation-id", "has-citations"];

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DataciteConfig {
    pub base_url: String,
}

impl Default for DataciteConfig {
    fn default() -> Self {
        DataciteConfig {
            base_url: DEFAULT_BASE_URL.to_owned(),
        }
    }
}

pub struct Datacite {
    config: DataciteConfig,
    http: Arc<dyn HttpClient>,
    capabilities: Capabilities,
}

impl Datacite {
    pub fn new(config: DataciteConfig, http: Arc<dyn HttpClient>) -> Self {
        let mut capabilities = Capabilities::default();
        capabilities.lookup_kinds = vec![IdKind::Doi, IdKind::Arxiv];
        capabilities.max_batch = MAX_BATCH;
        capabilities.fields = FieldSet::ALL;
        capabilities.search = true;
        capabilities.search_filters = vec![Filter::Years, Filter::WorkType];
        Datacite {
            config,
            http,
            capabilities,
        }
    }

    /// About 10 req/s allowed; stay well under.
    pub fn default_policy() -> ResiliencePolicy {
        let mut policy = ResiliencePolicy::default();
        policy.min_interval = Duration::from_millis(200);
        policy
    }

    fn url(&self, rest: &str, params: &[(&str, String)]) -> Result<String> {
        let base = format!("{}/dois{rest}", self.config.base_url.trim_end_matches('/'));
        let url = if params.is_empty() {
            url::Url::parse(&base)
        } else {
            url::Url::parse_with_params(&base, params)
        };
        url.map(String::from)
            .map_err(|e| Error::BadRequest(format!("invalid DataCite base URL: {e}")))
    }

    async fn get(&self, url: String) -> Result<HttpResponse> {
        let req = HttpRequest::get(url).header("accept", "application/vnd.api+json");
        let resp = self.http.send(req).await?;
        check(&resp)?;
        Ok(resp)
    }

    async fn lookup_one(&self, doi: &Doi) -> Result<Option<SourceRecord>> {
        let resp = match self
            .get(self.url(&format!("/{}", path_segment(doi.as_str())), &[])?)
            .await
        {
            Ok(resp) => resp,
            Err(Error::NotFound) => return Ok(None),
            Err(e) => return Err(e),
        };
        let body: serde_json::Value = serde_json::from_slice(&resp.body).map_err(Error::decode)?;
        let data = body.get("data").ok_or_else(|| Error::decode(MissingData))?;
        wire::to_record(data, resp.received_at)
            .map(Some)
            .map_err(Error::decode)
    }

    async fn list(&self, params: &[(&str, String)]) -> Result<(Vec<Hit>, wire::WireMeta)> {
        let resp = self.get(self.url("", params)?).await?;
        let list: WireList = serde_json::from_slice(&resp.body).map_err(Error::decode)?;
        let hits = list
            .data
            .iter()
            .map(|v| wire::to_record(v, resp.received_at).map(|r| Hit::new(r, None)))
            .collect::<serde_json::Result<_>>()
            .map_err(Error::decode)?;
        Ok((hits, list.meta))
    }
}

#[derive(Debug, thiserror::Error)]
#[error("DataCite response has no `data`")]
struct MissingData;

fn to_doi(id: &WorkId) -> Result<Doi> {
    match id {
        WorkId::Doi(doi) => Ok(doi.clone()),
        WorkId::Arxiv(arxiv) => Ok(format!("{ARXIV_DOI_PREFIX}/arxiv.{}", arxiv.id()).parse()?),
        _ => Err(Error::Unsupported("DataCite lookup of non-DOI identifiers")),
    }
}

/// Escapes Lucene query syntax in user text, so "BERT: Pre-training" is
/// searched as words rather than parsed as a field query.
fn escape_query(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if "+-&|!(){}[]^\"~*?:\\/".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn check(resp: &HttpResponse) -> Result<()> {
    if resp.is_success() {
        return Ok(());
    }
    let message = message(resp);
    Err(match resp.status {
        400 | 422 => Error::BadRequest(message.unwrap_or_else(|| "rejected by DataCite".into())),
        401 | 403 => Error::AuthRejected { message },
        404 => Error::NotFound,
        429 => Error::RateLimited {
            retry_after: resp
                .header("retry-after")
                .and_then(|v| v.trim().parse().ok())
                .map(Duration::from_secs),
        },
        status => Error::Server { status, message },
    })
}

/// The `title` of a JSON:API `errors` value, which DataCite sends as an
/// object or an array.
fn message(resp: &HttpResponse) -> Option<String> {
    let json: serde_json::Value = serde_json::from_slice(&resp.body).ok()?;
    let errors = json.get("errors")?;
    let first = errors.as_array().and_then(|a| a.first()).unwrap_or(errors);
    first.get("title")?.as_str().map(str::to_owned)
}

#[async_trait]
impl Provider for Datacite {
    fn id(&self) -> ProviderId {
        ID
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    async fn lookup(&self, ids: &[WorkId], _fields: FieldSet) -> Result<Response<Vec<Lookup>>> {
        let dois = ids.iter().map(to_doi).collect::<Result<Vec<_>>>()?;
        if let [doi] = dois.as_slice() {
            let record = self.lookup_one(doi).await?;
            return Ok(Response::new(vec![Lookup {
                id: ids[0].clone(),
                record,
            }]));
        }
        let mut found = Vec::new();
        for chunk in dois.chunks(MAX_BATCH) {
            let terms: Vec<String> = chunk
                .iter()
                .map(|d| format!("\"{}\"", d.as_str().replace('"', "\\\"")))
                .collect();
            let params = [
                ("query", format!("doi:({})", terms.join(" OR "))),
                ("page[size]", chunk.len().to_string()),
            ];
            let (hits, _) = self.list(&params).await?;
            found.extend(hits.into_iter().map(|h| h.record));
        }
        let lookups = ids
            .iter()
            .zip(&dois)
            .map(|(id, doi)| Lookup {
                id: id.clone(),
                record: found
                    .iter()
                    .find(|r| {
                        r.ids
                            .iter()
                            .any(|i| matches!(i, WorkId::Doi(d) if d == doi))
                    })
                    .cloned(),
            })
            .collect();
        Ok(Response::new(lookups))
    }

    async fn search(&self, query: &SearchQuery) -> Result<Response<Page>> {
        if query.mode != SearchMode::Keywords {
            return Err(Error::Unsupported("DataCite has no citation matcher"));
        }
        let mut clauses = Vec::new();
        if !query.text.trim().is_empty() {
            clauses.push(format!("({})", escape_query(query.text.trim())));
        }
        if let Some(years) = query.years {
            let end = |y: Option<i16>| y.map_or("*".to_owned(), |y| y.to_string());
            clauses.push(format!(
                "publicationYear:[{} TO {}]",
                end(years.from),
                end(years.to)
            ));
        }
        let mut params: Vec<(&str, String)> = Vec::new();
        for (key, value) in &query.extra {
            match key.as_str() {
                // Raw query syntax, ANDed with the rest.
                "query" => clauses.push(format!("({value})")),
                k if PASSTHROUGH.contains(&k) => params.push((k, value.clone())),
                other => {
                    return Err(Error::BadRequest(format!(
                        "unknown DataCite parameter {other:?} (supported: query, {})",
                        PASSTHROUGH.join(", ")
                    )));
                }
            }
        }
        if !clauses.is_empty() {
            params.insert(0, ("query", clauses.join(" AND ")));
        }
        if let Some(t) = &query.work_type {
            let id = wire::resource_type_id(t).ok_or_else(|| {
                Error::BadRequest(format!(
                    "DataCite has no resource type matching {:?}",
                    t.as_csl()
                ))
            })?;
            params.push(("resource-type-id", id.to_owned()));
        }
        let page_number: u32 = match &query.page.cursor {
            None => 1,
            Some(c) => {
                c.0.parse()
                    .map_err(|_| Error::BadRequest(format!("invalid DataCite cursor {:?}", c.0)))?
            }
        };
        params.push(("sort", "relevance".into()));
        params.push(("page[number]", page_number.to_string()));
        params.push((
            "page[size]",
            query.page.limit.clamp(1, MAX_PAGE).to_string(),
        ));

        let (hits, meta) = self.list(&params).await?;
        let more = meta.total_pages.is_some_and(|pages| page_number < pages) && !hits.is_empty();
        let next = more.then(|| Cursor((page_number + 1).to_string()));
        Ok(Response::new(Page::new(hits, meta.total, next)))
    }
}
