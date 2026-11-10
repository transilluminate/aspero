//! OpenAlex provider.
//!
//! Costs (help.openalex.org, 2026-11): single-work lookups are free, list and
//! filter calls $0.0001, searches $0.001, from a free $1/day with an API key.
//! So lookups of one ID use the free singleton endpoint, batches use one
//! `filter` call per identifier kind, and search is the only real expense.
//!
//! Every call reports its cost and remaining budget in [`Usage`] (USD).

mod http;
mod wire;

use std::sync::Arc;
use std::time::Duration;

use aspero_core::{
    Capabilities, Cursor, Doi, Error, FieldSet, Filter, HttpClient, HttpRequest, HttpResponse,
    IdKind, Lookup, Page, PageReq, Provider, ProviderId, Raw, ResiliencePolicy, Response, Result,
    SearchQuery, SourceRecord, Usage, WorkId, WorkType,
};
use async_trait::async_trait;
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use secrecy::{ExposeSecret, SecretString};

use crate::wire::{ARXIV_DOI_PREFIX, WireList};

pub const ID: ProviderId = ProviderId::from_static("openalex");

pub const DEFAULT_BASE_URL: &str = "https://api.openalex.org";

/// OpenAlex's cap on `per-page` and on values OR-ed in one filter.
const MAX_PAGE: u32 = 100;

/// Characters escaped in a `/works/<key>` path segment. `/` and `:` stay
/// literal: OpenAlex expects `doi:10.1038/nature14539` as written.
const PATH_SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

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
        capabilities.search_filters = vec![Filter::Years, Filter::WorkType, Filter::OpenAccess];
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

    /// Sends and maps non-2xx statuses to errors, keeping usage either way.
    async fn send(&self, url: String) -> Result<(HttpResponse, Usage)> {
        let resp = self.http.send(self.request(url)).await?;
        http::check(&resp)?;
        let usage = http::usage(&resp);
        Ok((resp, usage))
    }

    async fn lookup_one(&self, key: &Key) -> Result<(Option<SourceRecord>, Option<Usage>)> {
        let segment = utf8_percent_encode(&key.path_segment(), PATH_SEGMENT).to_string();
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
    async fn list(
        &self,
        params: &[(&str, String)],
    ) -> Result<(Vec<SourceRecord>, WireList, Usage)> {
        let url = url::Url::parse_with_params(&self.works_url(""), params)
            .map_err(|e| Error::BadRequest(format!("invalid OpenAlex base URL: {e}")))?;
        let (resp, usage) = self.send(url.into()).await?;
        let list: WireList = serde_json::from_slice(&resp.body).map_err(Error::decode)?;
        let records = list
            .results
            .iter()
            .map(|v| wire::to_record(v, Raw::json(v), resp.received_at))
            .collect::<serde_json::Result<_>>()
            .map_err(Error::decode)?;
        Ok((records, list, usage))
    }
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
            WorkId::Arxiv(arxiv) => {
                let doi = format!("{ARXIV_DOI_PREFIX}/arxiv.{}", arxiv.id());
                Key::doi(doi.parse::<Doi>()?)
            }
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

/// Sums cost; keeps the most recent remaining budget and reset time.
fn add_usage(total: &mut Option<Usage>, next: Option<Usage>) {
    let Some(next) = next else { return };
    let Some(acc) = total else {
        *total = Some(next);
        return;
    };
    acc.cost = match (acc.cost, next.cost) {
        (Some(a), Some(b)) => Some(a + b),
        (a, b) => a.or(b),
    };
    acc.remaining = next.remaining.or(acc.remaining);
    acc.resets_at = next.resets_at.or(acc.resets_at);
}

fn type_filter(t: &WorkType) -> Result<String> {
    let openalex = match t {
        WorkType::JournalArticle => "article",
        WorkType::ConferencePaper => {
            return Ok("type:article,primary_location.source.type:conference".into());
        }
        WorkType::Preprint => "preprint",
        WorkType::Book => "book",
        WorkType::Chapter => "book-chapter",
        WorkType::Thesis => "dissertation",
        WorkType::Report => "report",
        WorkType::Dataset => "dataset",
        WorkType::Other(other) => other.as_str(),
        _ => {
            return Err(Error::BadRequest(format!(
                "OpenAlex has no work type matching {:?}",
                t.as_csl()
            )));
        }
    };
    Ok(format!("type:{openalex}"))
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
        let mut groups: Vec<(&str, Vec<&Key>)> = Vec::new();
        let mut singles = Vec::new();
        for key in &keys {
            if !key.batchable() {
                singles.push(key);
            } else if let Some((_, group)) = groups.iter_mut().find(|(f, _)| *f == key.filter) {
                group.push(key);
            } else {
                groups.push((key.filter, vec![key]));
            }
        }

        let mut found = Vec::new();
        for (filter, group) in groups {
            for chunk in group.chunks(MAX_PAGE as usize) {
                let values: Vec<&str> = chunk.iter().map(|k| k.value.as_str()).collect();
                let params = [
                    ("filter", format!("{filter}:{}", values.join("|"))),
                    ("per-page", MAX_PAGE.to_string()),
                ];
                let (records, _, u) = self.list(&params).await?;
                found.extend(records);
                add_usage(&mut usage, Some(u));
            }
        }
        for key in singles {
            let (record, u) = self.lookup_one(key).await?;
            found.extend(record);
            add_usage(&mut usage, u);
        }

        let lookups = ids
            .iter()
            .zip(&keys)
            .map(|(id, key)| Lookup {
                id: id.clone(),
                record: found.iter().find(|r| r.ids.contains(&key.matches)).cloned(),
            })
            .collect();
        Ok(respond(lookups, usage))
    }

    async fn search(&self, query: &SearchQuery) -> Result<Response<Page>> {
        let mut filters = Vec::new();
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

        let mut params: Vec<(&str, String)> = Vec::new();
        for (key, value) in &query.extra {
            match key.as_str() {
                "filter" => filters.push(value.clone()),
                "sort" => params.push(("sort", value.clone())),
                other => {
                    return Err(Error::BadRequest(format!(
                        "unknown OpenAlex parameter {other:?} (supported: filter, sort)"
                    )));
                }
            }
        }
        if !query.text.trim().is_empty() {
            params.push(("search", query.text.trim().to_owned()));
        }
        if !filters.is_empty() {
            params.push(("filter", filters.join(",")));
        }
        params.extend(page_params(&query.page));

        let (records, list, usage) = self.list(&params).await?;
        let page = Page::new(records, list.meta.count, list.meta.next_cursor.map(Cursor));
        Ok(Response::with_usage(page, usage))
    }
}

/// Cursor paging (needs a key; unlimited depth, unlike `page=`).
fn page_params(page: &PageReq) -> [(&'static str, String); 2] {
    let cursor = page.cursor.as_ref().map_or("*", |c| c.0.as_str());
    [
        ("per-page", page.limit.clamp(1, MAX_PAGE).to_string()),
        ("cursor", cursor.to_owned()),
    ]
}
