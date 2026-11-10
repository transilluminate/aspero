//! Europe PMC provider: biomedical and life-science literature.
//!
//! Covers PubMed, PubMed Central and preprints; free and keyless. Adds
//! what the other providers lack: PMCID lookup, broad abstract coverage
//! for biomedical work, and "Retracted Publication" flags.
//!
//! - Lookups batch PMIDs, PMCIDs and DOIs into one OR query
//!   (`(EXT_ID:… AND SRC:MED) OR PMCID:… OR DOI:"…"`); results come back
//!   unordered and are matched by identifier.
//! - Search is relevance-ordered and pages by `cursorMark` *without*
//!   losing that order (checked 2026-11-10, unlike Crossref and DataCite).

use std::sync::Arc;
use std::time::Duration;

use aspero_core::csl::strip_markup;
use aspero_core::http::escape_lucene;
use aspero_core::{
    Capabilities, Cursor, Doi, Error, FieldSet, Filter, Hit, HttpClient, HttpRequest, HttpResponse,
    IdKind, Lookup, Page, PartialDate, Person, Pmcid, Pmid, Provider, ProviderId, Raw,
    ResiliencePolicy, Response, Result, SearchMode, SearchQuery, SourceRecord, Update, UpdateKind,
    Work, WorkId, WorkType, tidy_abstract,
};
use async_trait::async_trait;
use jiff::Timestamp;
use serde::Deserialize;

pub const ID: ProviderId = ProviderId::from_static("europepmc");

pub const DEFAULT_BASE_URL: &str = "https://www.ebi.ac.uk/europepmc/webservices/rest";

/// Identifiers per batched lookup.
const MAX_BATCH: usize = 50;

/// Europe PMC's page-size cap.
const MAX_PAGE: u32 = 1000;

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct EuropePmcConfig {
    pub base_url: String,
}

impl Default for EuropePmcConfig {
    fn default() -> Self {
        EuropePmcConfig {
            base_url: DEFAULT_BASE_URL.to_owned(),
        }
    }
}

pub struct EuropePmc {
    config: EuropePmcConfig,
    http: Arc<dyn HttpClient>,
    capabilities: Capabilities,
}

impl EuropePmc {
    pub fn new(config: EuropePmcConfig, http: Arc<dyn HttpClient>) -> Self {
        let mut capabilities = Capabilities::default();
        capabilities.lookup_kinds = vec![IdKind::Pmid, IdKind::Pmcid, IdKind::Doi];
        capabilities.max_batch = MAX_BATCH;
        capabilities.fields = FieldSet::ALL;
        capabilities.search = true;
        capabilities.search_filters = vec![Filter::Years, Filter::OpenAccess];
        EuropePmc {
            config,
            http,
            capabilities,
        }
    }

    pub fn default_policy() -> ResiliencePolicy {
        let mut policy = ResiliencePolicy::default();
        policy.min_interval = Duration::from_millis(200);
        policy
    }

    async fn search_raw(&self, params: &[(&str, String)]) -> Result<(WireResults, Timestamp)> {
        let base = format!("{}/search", self.config.base_url.trim_end_matches('/'));
        let url = url::Url::parse_with_params(&base, params)
            .map_err(|e| Error::BadRequest(format!("invalid Europe PMC base URL: {e}")))?;
        let resp = self
            .http
            .send(HttpRequest::get(String::from(url)).header("accept", "application/json"))
            .await?;
        check(&resp)?;
        let results = serde_json::from_slice(&resp.body).map_err(Error::decode)?;
        Ok((results, resp.received_at))
    }
}

fn check(resp: &HttpResponse) -> Result<()> {
    if resp.is_success() {
        return Ok(());
    }
    Err(match resp.status {
        400 => Error::BadRequest("rejected by Europe PMC".into()),
        404 => Error::NotFound,
        429 => Error::RateLimited {
            retry_after: resp
                .header("retry-after")
                .and_then(|v| v.trim().parse().ok())
                .map(Duration::from_secs),
        },
        status => Error::Server {
            status,
            message: None,
        },
    })
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct WireResults {
    hit_count: Option<u64>,
    next_cursor_mark: Option<String>,
    result_list: ResultList,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ResultList {
    result: Vec<serde_json::Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct WireRecord {
    id: Option<String>,
    source: Option<String>,
    pmid: Option<String>,
    pmcid: Option<String>,
    doi: Option<String>,
    title: Option<String>,
    author_list: AuthorList,
    pub_year: Option<String>,
    first_publication_date: Option<String>,
    journal_info: JournalInfo,
    page_info: Option<String>,
    abstract_text: Option<String>,
    pub_type_list: PubTypes,
    cited_by_count: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct AuthorList {
    author: Vec<Author>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Author {
    full_name: Option<String>,
    first_name: Option<String>,
    last_name: Option<String>,
    collective_name: Option<String>,
    author_id: Option<AuthorId>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct AuthorId {
    #[serde(rename = "type")]
    id_type: Option<String>,
    value: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct JournalInfo {
    volume: Option<String>,
    issue: Option<String>,
    journal: Journal,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Journal {
    title: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct PubTypes {
    pub_type: Vec<String>,
}

impl Author {
    fn person(self) -> Option<Person> {
        let mut person = Person::default();
        match (self.last_name, self.collective_name, self.full_name) {
            (Some(last), _, _) => {
                person.family = Some(last);
                person.given = self.first_name;
            }
            (None, Some(name), _) | (None, None, Some(name)) => person.literal = Some(name),
            (None, None, None) => return None,
        }
        person.orcid = self
            .author_id
            .filter(|id| id.id_type.as_deref() == Some("ORCID"))
            .and_then(|id| id.value);
        Some(person)
    }
}

fn to_record(value: &serde_json::Value, at: Timestamp) -> serde_json::Result<SourceRecord> {
    let r = WireRecord::deserialize(value)?;
    let mut ids: Vec<WorkId> = Vec::new();
    ids.extend(
        r.pmid
            .as_deref()
            .and_then(|p| p.parse::<Pmid>().ok())
            .map(WorkId::Pmid),
    );
    ids.extend(
        r.pmcid
            .as_deref()
            .and_then(|p| p.parse::<Pmcid>().ok())
            .map(WorkId::Pmcid),
    );
    ids.extend(
        r.doi
            .as_deref()
            .and_then(|d| d.parse::<Doi>().ok())
            .map(WorkId::Doi),
    );

    let types = &r.pub_type_list.pub_type;
    let has = |t: &str| types.iter().any(|p| p.eq_ignore_ascii_case(t));
    let mut work = Work::default();
    work.title = r.title.map(|t| t.trim_end_matches('.').to_owned());
    work.authors = r
        .author_list
        .author
        .into_iter()
        .filter_map(Author::person)
        .collect();
    work.issued = r
        .first_publication_date
        .as_deref()
        .and_then(|d| d.parse::<PartialDate>().ok())
        .or_else(|| {
            r.pub_year
                .as_deref()?
                .parse()
                .ok()
                .map(PartialDate::from_year)
        });
    work.work_type = if r.source.as_deref() == Some("PPR") || has("Preprint") {
        Some(WorkType::Preprint)
    } else if has("Journal Article") || has("research-article") || has("Review") {
        Some(WorkType::JournalArticle)
    } else {
        None
    };
    work.container_title = r.journal_info.journal.title;
    work.volume = r.journal_info.volume;
    work.issue = r.journal_info.issue;
    work.page = r.page_info;
    work.abstract_text = r
        .abstract_text
        .as_deref()
        .map(|a| tidy_abstract(&strip_markup(a)))
        .filter(|a| !a.is_empty());
    work.url = match (r.source.as_deref(), r.id.as_deref()) {
        (Some(source), Some(id)) => Some(format!("https://europepmc.org/article/{source}/{id}")),
        _ => None,
    };
    work.cited_by_count = r.cited_by_count;
    if has("Retracted Publication") {
        let mut retraction = Update::new(UpdateKind::Retraction);
        retraction.source = Some("europepmc".into());
        work.updates.push(retraction);
    }
    Ok(SourceRecord::new(
        ID,
        ids,
        work,
        FieldSet::ALL,
        Raw::json(value),
        at,
    ))
}

/// The query clause that finds `id`.
fn clause(id: &WorkId) -> Result<String> {
    Ok(match id {
        WorkId::Pmid(p) => format!("(EXT_ID:{p} AND SRC:MED)"),
        WorkId::Pmcid(p) => format!("PMCID:{p}"),
        WorkId::Doi(d) => format!("DOI:\"{}\"", d.as_str().replace('"', "\\\"")),
        _ => {
            return Err(Error::Unsupported(
                "Europe PMC lookup of this identifier kind",
            ));
        }
    })
}

#[async_trait]
impl Provider for EuropePmc {
    fn id(&self) -> ProviderId {
        ID
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    async fn lookup(&self, ids: &[WorkId], _fields: FieldSet) -> Result<Response<Vec<Lookup>>> {
        let clauses = ids.iter().map(clause).collect::<Result<Vec<_>>>()?;
        let mut found = Vec::new();
        for chunk in clauses.chunks(MAX_BATCH) {
            let params = [
                ("query", chunk.join(" OR ")),
                ("resultType", "core".to_owned()),
                ("format", "json".to_owned()),
                ("pageSize", chunk.len().to_string()),
            ];
            let (results, at) = self.search_raw(&params).await?;
            for v in &results.result_list.result {
                found.push(to_record(v, at).map_err(Error::decode)?);
            }
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
        if query.mode != SearchMode::Keywords {
            return Err(Error::Unsupported("Europe PMC has no citation matcher"));
        }
        let mut clauses = Vec::new();
        if !query.text.trim().is_empty() {
            clauses.push(format!("({})", escape_lucene(query.text.trim())));
        }
        for (key, value) in &query.extra {
            match key.as_str() {
                // Raw Europe PMC query syntax, ANDed with the rest.
                "query" => clauses.push(format!("({value})")),
                other => {
                    return Err(Error::BadRequest(format!(
                        "unknown Europe PMC parameter {other:?} (supported: query)"
                    )));
                }
            }
        }
        if let Some(years) = query.years {
            let end = |y: Option<i16>| y.map_or("*".to_owned(), |y| y.to_string());
            clauses.push(format!(
                "PUB_YEAR:[{} TO {}]",
                end(years.from),
                end(years.to)
            ));
        }
        if let Some(oa) = query.open_access {
            clauses.push(format!("OPEN_ACCESS:{}", if oa { "y" } else { "n" }));
        }
        let cursor = query.page.cursor.as_ref().map_or("*", |c| c.0.as_str());
        let params = [
            ("query", clauses.join(" AND ")),
            ("resultType", "core".to_owned()),
            ("format", "json".to_owned()),
            ("pageSize", query.page.limit.clamp(1, MAX_PAGE).to_string()),
            ("cursorMark", cursor.to_owned()),
        ];
        let (results, at) = self.search_raw(&params).await?;
        let hits: Vec<Hit> = results
            .result_list
            .result
            .iter()
            .map(|v| to_record(v, at).map(|r| Hit::new(r, None)))
            .collect::<serde_json::Result<_>>()
            .map_err(Error::decode)?;
        // The last page repeats its cursor; an empty page is also the end.
        let next = results
            .next_cursor_mark
            .filter(|next| !hits.is_empty() && next != cursor)
            .map(Cursor);
        Ok(Response::new(Page::new(hits, results.hit_count, next)))
    }
}
