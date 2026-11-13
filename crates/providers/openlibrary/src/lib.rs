//! Open Library (Internet Archive) provider: books.
//!
//! Books mostly lack DOIs, so this fills the gap the article-centric
//! providers leave. Free and keyless; Open Library asks clients to identify
//! themselves in the User-Agent (the engine's carries the contact address).
//!
//! - Lookup by ISBN in batches: `/api/books?bibkeys=ISBN:a,ISBN:b&jscmd=data`
//!   (one edition per ISBN; ISBNs it doesn't know are simply absent).
//! - Search: `/search.json` (Solr, relevance-ordered), returning works with
//!   every edition's ISBNs; year ranges via `first_publish_year:[A TO B]`,
//!   authors and titles as `author=` and `title=` (which take any text:
//!   Open Library escapes them), newest first with `sort=new`.
//!
//! Data quality varies: some ISBNs carry unrelated records, so scoring (not
//! the identifier alone) decides matches.

use std::sync::Arc;
use std::time::Duration;

use aspero_core::csl::null_default;
use aspero_core::http::{escape_lucene, status_error};
use aspero_core::query::{check_supported, lucene_term, lucene_years, render_lucene};
use aspero_core::{
    BooleanSupport, Capabilities, Cursor, Error, FieldSet, Filter, FilterValue, Hit, HttpClient,
    HttpRequest, HttpResponse, IdKind, Isbn, Lookup, Page, PartialDate, Person, Provider,
    ProviderId, QueryNode, Raw, ResiliencePolicy, Response, Result, SearchMode, SearchQuery, Sort,
    SourceRecord, TermField, Work, WorkId, WorkType,
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
        capabilities.search_filters = vec![
            Filter::Years,
            Filter::Author,
            Filter::Title,
            Filter::Keywords,
            Filter::Language,
        ];
        capabilities.sorts = vec![Sort::Relevance, Sort::Newest];
        // Solr: AND, OR and NOT across fields, nested; a NOT needs
        // something beside it to find (see `express`).
        capabilities.search_any = BooleanSupport::Anywhere;
        capabilities.search_none = BooleanSupport::Anywhere;
        capabilities.search_nesting = true;
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
    #[serde(deserialize_with = "null_default")]
    authors: Vec<Named>,
    publish_date: Option<String>,
    #[serde(deserialize_with = "null_default")]
    publishers: Vec<Named>,
    #[serde(deserialize_with = "null_default")]
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
    #[serde(deserialize_with = "null_default")]
    isbn_13: Vec<String>,
    #[serde(deserialize_with = "null_default")]
    isbn_10: Vec<String>,
}

/// `/search.json` document for one work.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SearchDoc {
    key: Option<String>,
    title: Option<String>,
    subtitle: Option<String>,
    #[serde(deserialize_with = "null_default")]
    author_name: Vec<String>,
    first_publish_year: Option<i16>,
    /// Every edition's year.
    #[serde(deserialize_with = "null_default")]
    publish_year: Vec<i16>,
    #[serde(deserialize_with = "null_default")]
    isbn: Vec<String>,
    #[serde(deserialize_with = "null_default")]
    publisher: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct SearchResults {
    num_found: Option<u64>,
    #[serde(deserialize_with = "null_default")]
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
                    .and_then(|v| or_skip(edition_record(isbn, v, resp.received_at)));
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
        if let Some(tree) = &query.tree {
            clauses.push(express(tree, &self.capabilities).map_err(Error::UnsupportedQuery)?);
        }
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
        let mut params = vec![("q", clauses.join(" AND "))];
        if let Some(author) = query.field(Filter::Author) {
            params.push(("author", author.to_owned()));
        }
        if let Some(title) = query.field(Filter::Title) {
            params.push(("title", title.to_owned()));
        }
        match query.sort {
            Sort::Relevance => {}
            Sort::Newest => params.push(("sort", "new".to_owned())),
            _ => return Err(Error::Unsupported("Open Library sort")),
        }
        params.extend([
            ("fields", SEARCH_FIELDS.to_owned()),
            ("limit", limit.to_string()),
            ("page", page_number.to_string()),
        ]);
        let resp = self.get("/search.json", &params).await?;
        let results: SearchResults = serde_json::from_slice(&resp.body).map_err(Error::decode)?;
        let base = self.config.base_url.trim_end_matches('/');
        let hits: Vec<Hit> = results
            .docs
            .iter()
            .filter_map(|v| or_skip(search_record(base, v, resp.received_at)))
            .map(|r| Hit::new(r, None))
            .collect();
        let seen = u64::from(page_number) * u64::from(limit);
        let more = !hits.is_empty() && results.num_found.is_some_and(|n| n > seen);
        // No page follows the last one a u32 can number.
        let next = page_number
            .checked_add(1)
            .filter(|_| more)
            .map(|n| Cursor(n.to_string()));
        Ok(Response::new(Page::new(hits, results.num_found, next)))
    }

    fn express(&self, tree: &QueryNode) -> std::result::Result<(), String> {
        express(tree, &self.capabilities).map(|_| ())
    }
}

const NAME: &str = "Open Library";

/// A query tree as Open Library's `q` (Solr, checked live 2026-11-13):
/// `title`, `author_name`, `first_publish_year` and `language` (MARC's
/// three-letter codes: `fre`, `ger`) take AND, OR and NOT, nested. A
/// query that only leaves things out fails (HTTP 500), alone or as one
/// side of an OR, so a none must sit in an all beside something to find.
fn express(tree: &QueryNode, caps: &Capabilities) -> std::result::Result<String, String> {
    check_supported(tree, caps, NAME)?;
    check_negations(tree, false)?;
    render_lucene(tree, &mut |leaf| match leaf {
        QueryNode::Term(t) => {
            let prefix = match t.field {
                TermField::Keywords => None,
                TermField::Title => Some("title"),
                TermField::Author => Some("author_name"),
                _ => return Err(format!("{NAME} can't search by {}", t.field.name())),
            };
            Ok(lucene_term(prefix, &t.value, t.phrase))
        }
        QueryNode::Filter(f) => Ok(match f {
            FilterValue::Years(years) => lucene_years("first_publish_year", *years),
            FilterValue::Language(code) => {
                let (marc, _) = aspero_core::query::language(code)
                    .ok_or_else(|| format!("{code:?} is not an ISO 639-1 language code"))?;
                format!("language:{marc}")
            }
            _ => return Err(format!("{NAME} can't filter by {}", leaf.leaf_name())),
        }),
        other => Err(format!("{NAME} can't search by {}", other.leaf_name())),
    })
}

/// Every none is in an all beside a node that isn't a none.
fn check_negations(node: &QueryNode, beside_positive: bool) -> std::result::Result<(), String> {
    match node {
        QueryNode::None(_) if !beside_positive => Err(format!(
            "{NAME} can only leave out terms beside something to find, in an all"
        )),
        QueryNode::All(nodes) => {
            let positive = nodes.iter().any(|n| !matches!(n, QueryNode::None(_)));
            nodes.iter().try_for_each(|n| check_negations(n, positive))
        }
        QueryNode::Any(nodes) | QueryNode::None(nodes) => {
            nodes.iter().try_for_each(|n| check_negations(n, false))
        }
        _ => Ok(()),
    }
}
