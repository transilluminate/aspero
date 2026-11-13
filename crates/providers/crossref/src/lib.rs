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
use aspero_core::query::check_supported;
use aspero_core::{
    AuthorId, Capabilities, Cursor, Doi, Error, Field, FieldSet, Filter, FilterValue, Hit,
    HttpClient, HttpRequest, HttpResponse, IdKind, Lookup, Page, Provider, ProviderId, QueryNode,
    ResiliencePolicy, Response, Result, SearchMode, SearchQuery, Sort, SourceRecord, Term,
    TermField, WorkId, YearRange,
};
use async_trait::async_trait;

use crate::wire::{Envelope, WireList};

pub const ID: ProviderId = ProviderId::from_static("crossref");

pub const DEFAULT_BASE_URL: &str = "https://api.crossref.org";

/// DOIs per batched `filter=doi:…,doi:…` call; keeps URLs a sane length.
const MAX_BATCH: usize = 50;

/// Sorts Crossref won't combine with cursor paging.
const DATE_SORTS: [&str; 4] = ["issued", "published", "published-print", "published-online"];
/// The deepest `offset` Crossref serves.
const MAX_OFFSET: u64 = 10_000;
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
            Filter::Keywords,
            Filter::AuthorId,
            Filter::HasAbstract,
            Filter::Since,
            Filter::Identifier,
        ];
        capabilities.sorts = vec![Sort::Relevance, Sort::Newest, Sort::MostCited];
        // Queries are bags of words, ANDed with filters: all of, only (but
        // for an any of DOIs: see `express`).
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
        let tree = query
            .tree
            .as_ref()
            .map(|t| express(t, &self.capabilities))
            .transpose()
            .map_err(Error::UnsupportedQuery)?;
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
        // When the DOI was first deposited. `from-index-date` moves with
        // every re-index, so it mostly finds old works (checked live
        // 2026-11-13).
        if let Some(since) = query.since {
            filters.push(format!("from-created-date:{since}"));
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
        if let Some(tree) = tree {
            params.extend(tree.params);
            filters.extend(tree.filters);
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
        let rows = query.page.limit.clamp(1, MAX_ROWS);
        params.push(("rows", rows.to_string()));
        // Crossref refuses date sorts with a cursor, so those page by offset,
        // carried in our cursor as `offset:N`. Its order among works of one
        // date isn't stable between requests, so such pages may repeat or
        // skip them; one sort key leaves no tie-break to ask for.
        let by_date = params
            .iter()
            .any(|(k, v)| *k == "sort" && DATE_SORTS.contains(&v.as_str()));
        let offset = match &query.page.cursor {
            Some(c) if by_date => {
                c.0.strip_prefix("offset:")
                    .and_then(|n| n.parse::<u64>().ok())
                    .ok_or_else(|| Error::BadRequest("not a Crossref offset cursor".into()))?
            }
            _ => 0,
        };
        match by_date {
            true => params.push(("offset", offset.to_string())),
            false => {
                let cursor = query.page.cursor.as_ref().map_or("*", |c| c.0.as_str());
                params.push(("cursor", cursor.to_owned()));
            }
        }

        let (hits, list) = self.list(&params).await?;
        // Crossref keeps returning a cursor after the last page; an empty
        // page is the real end.
        let next = match by_date {
            true => {
                let next = offset + hits.len() as u64;
                let more = !hits.is_empty()
                    && list.total_results.is_none_or(|total| next < total)
                    && next <= MAX_OFFSET;
                more.then(|| Cursor(format!("offset:{next}")))
            }
            false => list.next_cursor.filter(|_| !hits.is_empty()).map(Cursor),
        };
        Ok(Response::new(Page::new(hits, list.total_results, next)))
    }

    fn express(&self, tree: &QueryNode) -> std::result::Result<(), String> {
        express(tree, &self.capabilities).map(|_| ())
    }
}

/// A query tree as Crossref parameters.
#[derive(Debug, Default, PartialEq, Eq)]
struct Tree {
    /// `query`, `query.title`, …: one each, the terms' words together.
    params: Vec<(&'static str, String)>,
    filters: Vec<String>,
}

/// A query tree in Crossref's terms: all of words in its query fields
/// (`query`, `query.title`, `query.author`, `query.container-title`),
/// and of filters (years, one work type, one ORCID, `has-abstract`).
/// Its queries rank a bag of words, so there are no phrases, and filters
/// of one name OR (two ORCIDs would mean either). Crossref has no filter
/// for retracted works themselves (`update-type:retraction` finds the
/// notices), nor for language.
///
/// Works are named by DOI alone (`doi:`), so an any of DOIs is its one
/// any: repeated `doi:` filters OR, ANDed with the rest (checked live
/// 2026-11-14). It holds no arXiv works (their DOIs are DataCite's), and
/// its filters can't leave anything out.
fn express(tree: &QueryNode, caps: &Capabilities) -> std::result::Result<Tree, String> {
    const NAME: &str = "Crossref";
    let nodes = match tree {
        QueryNode::All(nodes) => nodes.iter().collect(),
        node => vec![node],
    };
    let doi = |t: &Term| match t.work_id()? {
        WorkId::Doi(doi) if doi.as_str().contains(',') => {
            Err(format!("{NAME} can't filter by a DOI holding a comma"))
        }
        WorkId::Doi(doi) => Ok(doi.as_str().to_owned()),
        WorkId::Arxiv(_) => Err(format!(
            "{NAME} has no arXiv works: their DOIs are DataCite's"
        )),
        _ => Err(format!("{NAME} finds works by DOI only")),
    };
    let identifier = |node: &QueryNode| match node {
        QueryNode::Term(t) if t.field == TermField::Identifier => Some(t.clone()),
        _ => None,
    };
    let mut dois: Option<Vec<String>> = None;
    let mut rest = Vec::new();
    for node in nodes {
        let found = match node {
            QueryNode::Any(any) if any.iter().all(|n| identifier(n).is_some()) => any
                .iter()
                .filter_map(identifier)
                .map(|t| doi(&t))
                .collect::<std::result::Result<Vec<_>, _>>()?,
            node => match identifier(node) {
                Some(t) => vec![doi(&t)?],
                None => {
                    rest.push(node.clone());
                    continue;
                }
            },
        };
        if dois.replace(found).is_some() {
            return Err(format!(
                "{NAME} can't require two DOIs at once, only any of them"
            ));
        }
    }
    // The rest, as all of it (a tree is normalised: no all holds an all).
    check_supported(&QueryNode::All(rest.clone()), caps, NAME)?;
    let mut out = Tree::default();
    let mut years: Option<YearRange> = None;
    let (mut work_type, mut orcid) = (None, None);
    for leaf in &rest {
        match leaf {
            QueryNode::Term(t) if t.phrase => {
                return Err(format!("{NAME} can't search for phrases"));
            }
            QueryNode::Term(t) if t.field == TermField::AuthorId => {
                let Some(AuthorId::Orcid(id)) = AuthorId::parse(&t.value) else {
                    return Err(format!(
                        "{NAME} finds authors by ORCID, not OpenAlex author ids"
                    ));
                };
                if orcid.replace(id).is_some() {
                    return Err(format!("{NAME} can't require two ORCIDs at once"));
                }
            }
            QueryNode::Term(t) => {
                let param = match t.field {
                    TermField::Keywords => "query",
                    TermField::Title => "query.title",
                    TermField::Author => "query.author",
                    TermField::Container => "query.container-title",
                    _ => return Err(format!("{NAME} can't search by {}", t.field.name())),
                };
                match out.params.iter_mut().find(|(p, _)| *p == param) {
                    Some((_, words)) => {
                        words.push(' ');
                        words.push_str(&t.value);
                    }
                    None => out.params.push((param, t.value.clone())),
                }
            }
            QueryNode::Filter(FilterValue::Years(y)) => {
                // All of several ranges: where they overlap.
                let both = |a: Option<i16>, b: Option<i16>, pick: fn(i16, i16) -> i16| match (a, b)
                {
                    (Some(a), Some(b)) => Some(pick(a, b)),
                    (a, b) => a.or(b),
                };
                years = Some(match years {
                    None => *y,
                    Some(prev) => YearRange {
                        from: both(prev.from, y.from, i16::max),
                        to: both(prev.to, y.to, i16::min),
                    },
                });
            }
            QueryNode::Filter(FilterValue::WorkType(t)) => {
                let crossref = wire::crossref_type(t)
                    .ok_or_else(|| format!("{NAME} has no work type matching {:?}", t.as_csl()))?;
                if work_type.replace(crossref).is_some() {
                    return Err(format!("{NAME} can't require two work types at once"));
                }
            }
            QueryNode::Filter(FilterValue::HasAbstract) => {
                out.filters.push("has-abstract:true".into());
            }
            other => return Err(format!("{NAME} can't search by {}", other.leaf_name())),
        }
    }
    if let Some(years) = years {
        out.filters
            .extend(years.from.map(|y| format!("from-pub-date:{y:04}")));
        out.filters
            .extend(years.to.map(|y| format!("until-pub-date:{y:04}")));
    }
    out.filters.extend(work_type.map(|t| format!("type:{t}")));
    out.filters.extend(orcid.map(|o| format!("orcid:{o}")));
    out.filters
        .extend(dois.into_iter().flatten().map(|d| format!("doi:{d}")));
    Ok(out)
}
