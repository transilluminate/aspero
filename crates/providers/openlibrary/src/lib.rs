//! Open Library (Internet Archive) provider: books.
//!
//! Books mostly lack DOIs, so this fills the gap the article-centric
//! providers leave. Free and keyless; Open Library asks clients to identify
//! themselves in the User-Agent (the engine's carries the contact address).
//!
//! - Lookup by ISBN in batches: `/api/books?bibkeys=ISBN:a,ISBN:b&jscmd=data`
//!   (one edition per ISBN; ISBNs it doesn't know are simply absent).
//! - Search: `/search.json` (Solr, relevance-ordered), returning works with
//!   every edition's ISBNs; year ranges via `first_publish_year:[A TO B]`.
//!
//! Data quality varies: some ISBNs carry unrelated records, so scoring (not
//! the identifier alone) decides matches.

use std::sync::Arc;
use std::time::Duration;

use aspero_core::http::{escape_lucene, status_error};
use aspero_core::{
    Capabilities, Cursor, Error, FieldSet, Filter, Hit, HttpClient, HttpRequest, HttpResponse,
    IdKind, Isbn, Lookup, Page, PartialDate, Person, Provider, ProviderId, Raw, ResiliencePolicy,
    Response, Result, SearchMode, SearchQuery, SourceRecord, Work, WorkId, WorkType,
};
use async_trait::async_trait;
use jiff::Timestamp;
use serde::Deserialize;

pub const ID: ProviderId = ProviderId::from_static("openlibrary");

pub const DEFAULT_BASE_URL: &str = "https://openlibrary.org";

/// ISBNs per batched lookup.
const MAX_BATCH: usize = 50;

/// Results per search page (Open Library allows more; works carry long ISBN
/// lists, so pages stay modest).
const MAX_PAGE: u32 = 100;

/// ISBNs kept per searched work: enough to match editions, without
/// hundreds of translations.
const MAX_WORK_ISBNS: usize = 20;

const SEARCH_FIELDS: &str = "key,title,subtitle,author_name,first_publish_year,publish_year,isbn,publisher,number_of_pages_median";

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct OpenLibraryConfig {
    pub base_url: String,
}

impl Default for OpenLibraryConfig {
    fn default() -> Self {
        OpenLibraryConfig {
            base_url: DEFAULT_BASE_URL.to_owned(),
        }
    }
}

pub struct OpenLibrary {
    config: OpenLibraryConfig,
    http: Arc<dyn HttpClient>,
    capabilities: Capabilities,
}

impl OpenLibrary {
    pub fn new(config: OpenLibraryConfig, http: Arc<dyn HttpClient>) -> Self {
        let mut capabilities = Capabilities::default();
        capabilities.lookup_kinds = vec![IdKind::Isbn];
        capabilities.max_batch = MAX_BATCH;
        capabilities.fields = FieldSet::of(&[
            aspero_core::Field::Title,
            aspero_core::Field::Authors,
            aspero_core::Field::Issued,
            aspero_core::Field::Type,
            aspero_core::Field::Publication,
            aspero_core::Field::Url,
        ]);
        capabilities.search = true;
        capabilities.search_filters = vec![Filter::Years];
        OpenLibrary {
            config,
            http,
            capabilities,
        }
    }

    /// A volunteer-run service: keep it gentle.
    pub fn default_policy() -> ResiliencePolicy {
        let mut policy = ResiliencePolicy::default();
        policy.min_interval = Duration::from_millis(600);
        policy
    }

    async fn get(&self, path: &str, params: &[(&str, String)]) -> Result<HttpResponse> {
        let base = format!("{}{path}", self.config.base_url.trim_end_matches('/'));
        let url = url::Url::parse_with_params(&base, params)
            .map_err(|e| Error::BadRequest(format!("invalid Open Library base URL: {e}")))?;
        let resp = self
            .http
            .send(HttpRequest::get(String::from(url)).header("accept", "application/json"))
            .await?;
        check(&resp)?;
        Ok(resp)
    }
}

fn check(resp: &HttpResponse) -> Result<()> {
    if resp.is_success() {
        return Ok(());
    }
    Err(match resp.status {
        400 | 422 => Error::BadRequest("rejected by Open Library".into()),
        _ => status_error(resp, None),
    })
}

/// `/api/books?jscmd=data` record for one edition.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Edition {
    title: Option<String>,
    subtitle: Option<String>,
    authors: Vec<Named>,
    publish_date: Option<String>,
    publishers: Vec<Named>,
    identifiers: Identifiers,
    url: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Named {
    name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Identifiers {
    isbn_13: Vec<String>,
    isbn_10: Vec<String>,
}

/// `/search.json` document for one work.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SearchDoc {
    key: Option<String>,
    title: Option<String>,
    subtitle: Option<String>,
    author_name: Vec<String>,
    first_publish_year: Option<i16>,
    /// Every edition's year.
    publish_year: Vec<i16>,
    isbn: Vec<String>,
    publisher: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct SearchResults {
    num_found: Option<u64>,
    docs: Vec<serde_json::Value>,
}

/// Open Library keeps subtitles apart ("Reinforcement Learning" + "An
/// Introduction"); citations give the full title.
fn full_title(title: Option<String>, subtitle: Option<String>) -> Option<String> {
    match (title, subtitle.filter(|s| !s.trim().is_empty())) {
        (Some(t), Some(s)) => Some(format!("{t}: {s}")),
        (t, _) => t,
    }
}

fn isbns<'a>(raw: impl IntoIterator<Item = &'a String>) -> Vec<WorkId> {
    let mut ids = Vec::new();
    for isbn in raw.into_iter().filter_map(|s| s.parse::<Isbn>().ok()) {
        let id = WorkId::Isbn(isbn);
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

fn book(
    title: Option<String>,
    authors: Vec<String>,
    year: Option<i16>,
    publisher: Option<String>,
    url: Option<String>,
) -> Work {
    let mut work = Work::default();
    work.title = title;
    work.authors = authors.into_iter().map(Person::literal).collect();
    work.issued = year.map(PartialDate::from_year);
    work.publisher = publisher;
    work.work_type = Some(WorkType::Book);
    work.url = url;
    work
}

/// The first four-digit year in a free-form date ("3 January 2017").
fn year_in(date: &str) -> Option<i16> {
    date.as_bytes()
        .windows(4)
        .find(|w| w.iter().all(u8::is_ascii_digit))
        .and_then(|w| std::str::from_utf8(w).ok()?.parse().ok())
}

fn edition_record(
    requested: &Isbn,
    value: &serde_json::Value,
    at: Timestamp,
) -> serde_json::Result<SourceRecord> {
    let e = Edition::deserialize(value)?;
    let mut ids = vec![WorkId::Isbn(requested.clone())];
    for id in isbns(e.identifiers.isbn_13.iter().chain(&e.identifiers.isbn_10)) {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    let work = book(
        full_title(e.title, e.subtitle),
        e.authors.into_iter().filter_map(|a| a.name).collect(),
        e.publish_date.as_deref().and_then(year_in),
        e.publishers.into_iter().find_map(|p| p.name),
        e.url,
    );
    Ok(SourceRecord::new(
        ID,
        ids,
        work,
        FieldSet::ALL,
        Raw::json(value),
        at,
    ))
}

fn search_record(
    base: &str,
    value: &serde_json::Value,
    at: Timestamp,
) -> serde_json::Result<SourceRecord> {
    let d = SearchDoc::deserialize(value)?;
    let mut ids = isbns(&d.isbn);
    ids.truncate(MAX_WORK_ISBNS);
    let url = d.key.as_ref().map(|k| format!("{base}{k}"));
    let mut work = book(
        full_title(d.title, d.subtitle),
        d.author_name,
        d.first_publish_year,
        d.publisher.into_iter().next(),
        url,
    );
    // Later editions: a citation of the 2018 second edition is this work.
    let mut editions: Vec<i16> = d
        .publish_year
        .into_iter()
        .filter(|y| Some(*y) != d.first_publish_year)
        .collect();
    editions.sort_unstable();
    editions.dedup();
    work.other_years = editions;
    Ok(SourceRecord::new(
        ID,
        ids,
        work,
        FieldSet::ALL,
        Raw::json(value),
        at,
    ))
}

#[async_trait]
impl Provider for OpenLibrary {
    fn id(&self) -> ProviderId {
        ID
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    async fn lookup(&self, ids: &[WorkId], _fields: FieldSet) -> Result<Response<Vec<Lookup>>> {
        let isbns: Vec<&Isbn> = ids
            .iter()
            .map(|id| match id {
                WorkId::Isbn(isbn) => Ok(isbn),
                _ => Err(Error::Unsupported(
                    "Open Library lookup of non-ISBN identifiers",
                )),
            })
            .collect::<Result<_>>()?;
        let mut lookups = Vec::with_capacity(ids.len());
        for (chunk_ids, chunk) in ids.chunks(MAX_BATCH).zip(isbns.chunks(MAX_BATCH)) {
            let keys: Vec<String> = chunk.iter().map(|i| format!("ISBN:{i}")).collect();
            let params = [
                ("bibkeys", keys.join(",")),
                ("format", "json".to_owned()),
                ("jscmd", "data".to_owned()),
            ];
            let resp = self.get("/api/books", &params).await?;
            let body: serde_json::Map<String, serde_json::Value> =
                serde_json::from_slice(&resp.body).map_err(Error::decode)?;
            for ((id, isbn), key) in chunk_ids.iter().zip(chunk).zip(&keys) {
                let record = body
                    .get(key)
                    .map(|v| edition_record(isbn, v, resp.received_at))
                    .transpose()
                    .map_err(Error::decode)?;
                lookups.push(Lookup {
                    id: id.clone(),
                    record,
                });
            }
        }
        Ok(Response::new(lookups))
    }

    async fn search(&self, query: &SearchQuery) -> Result<Response<Page>> {
        if query.mode != SearchMode::Keywords {
            return Err(Error::Unsupported("Open Library has no citation matcher"));
        }
        if let Some((key, _)) = query.extra.iter().next() {
            return Err(Error::BadRequest(format!(
                "unknown Open Library parameter {key:?}"
            )));
        }
        let mut clauses = Vec::new();
        if !query.text.trim().is_empty() {
            clauses.push(format!("({})", escape_lucene(query.text.trim())));
        }
        if let Some(years) = query.years {
            let end = |y: Option<i16>| y.map_or("*".to_owned(), |y| y.to_string());
            clauses.push(format!(
                "first_publish_year:[{} TO {}]",
                end(years.from),
                end(years.to)
            ));
        }
        let page_number: u32 = match &query.page.cursor {
            None => 1,
            Some(c) => c
                .0
                .parse()
                .map_err(|_| Error::BadRequest(format!("invalid Open Library cursor {:?}", c.0)))?,
        };
        let limit = query.page.limit.clamp(1, MAX_PAGE);
        let params = [
            ("q", clauses.join(" AND ")),
            ("fields", SEARCH_FIELDS.to_owned()),
            ("limit", limit.to_string()),
            ("page", page_number.to_string()),
        ];
        let resp = self.get("/search.json", &params).await?;
        let results: SearchResults = serde_json::from_slice(&resp.body).map_err(Error::decode)?;
        let base = self.config.base_url.trim_end_matches('/');
        let hits: Vec<Hit> = results
            .docs
            .iter()
            .map(|v| search_record(base, v, resp.received_at).map(|r| Hit::new(r, None)))
            .collect::<serde_json::Result<_>>()
            .map_err(Error::decode)?;
        let seen = u64::from(page_number) * u64::from(limit);
        let more = !hits.is_empty() && results.num_found.is_some_and(|n| n > seen);
        let next = more.then(|| Cursor((page_number + 1).to_string()));
        Ok(Response::new(Page::new(hits, results.num_found, next)))
    }
}
