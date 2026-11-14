//! aspero for apps, through UniFFI: an [`Aspero`] handle whose calls
//! return typed records ([`records`]), with the reasons and failures as
//! data so the app can word and localise them.
//!
//! The app passes everything in ([`Config`]): nothing is read from the
//! environment, so the app keeps keys where it likes (the Keychain). Calls
//! run on the handle's own runtime, so they can be awaited from any
//! executor. To cancel one, pass a [`CallToken`] and cancel it: UniFFI's
//! Swift bindings don't pass a `Task`'s cancellation to Rust, so the app
//! calls `cancel()` from `withTaskCancellationHandler`. Hold one handle for
//! the app's life: calls through it share the cache, pacing and requests
//! in flight.

use std::sync::Arc;

use aspero_core::{Cursor, Error, SearchQuery, Sort, WorkType, YearRange};
use aspero_engine::{
    CachePolicy, CallOptions, Engine, EngineError, Extras, SecretString, SqliteCache,
    StandardConfig, standard,
};
use aspero_json::InputClaim;
use futures::StreamExt;
use tokio::sync::watch;
use tokio::task::JoinHandle;

mod notices;
pub mod records;

use records::{
    Analysis, AnalysisKind, Bibliography, Budget, CacheStats, CitationStyle, CiteFormat,
    CitedEntry, Claim as ClaimRecord, Dependency, DocxReferences, ExportFormat, Identification,
    KeyCheck, Line, LineReference, Lookup, Lookups, Mention, ProviderUsage, ReadError, Reference,
    ReferencesPage, SearchNode, SearchRequest, SearchResults, SourceSearch, StyleError,
    StyleSource, Unavailable, Utf16Offsets, Verification, Verifications, Work,
};

uniffi::setup_scaffolding!();

#[derive(Debug, Clone, uniffi::Record)]
pub struct Config {
    /// Contact address for providers' polite pools: the app's support
    /// address.
    pub mailto: Option<String>,
    pub openalex_key: Option<String>,
    /// Where to keep the answer cache (`cache.sqlite` inside); `None` for
    /// no cache.
    pub cache_dir: Option<String>,
    /// Stops OpenAlex's paid calls (searches, batch lookups) once today's
    /// use, as OpenAlex reports it for the key (shared with anything else
    /// using it), reaches this many US dollars, until the budget resets.
    /// Free lookups go on and checks fall back to the free sources; a
    /// search that needs OpenAlex fails as `budgetCapped`, its
    /// `Failure.budgetCap` saying the cap, what's used and when it resets.
    /// With a key, a free budget refresh decides the first paid call of
    /// the day. `None`: no cap but OpenAlex's own.
    #[uniffi(default = None)]
    pub openalex_daily_cap: Option<f64>,
    /// Sources never to ask, for lookups, checks or searches, by the names
    /// `SearchResults.provider` gives: `openalex`, `crossref`, `datacite`,
    /// `doi`, `europepmc`, `openlibrary`. `searchCapabilities` leaves them
    /// out, and a search naming one fails as `unsupported` ("Europe PMC is
    /// turned off"). An unknown name, or every source, is a setup error.
    #[uniffi(default = [])]
    pub disabled_sources: Vec<String>,
    /// How many days a found record (with its notices: retractions,
    /// corrections) is trusted before the source is asked again; older,
    /// it's still served if the source fails. 0 asks every time.
    #[uniffi(default = 7)]
    pub cache_found_days: u32,
    /// How many days a "not found" is trusted before asking again (works
    /// get registered). 0 asks every time.
    #[uniffi(default = 1)]
    pub cache_not_found_days: u32,
    /// The cache's size limit, in megabytes (1,000,000 bytes) as
    /// `CacheStats.bytes` counts them: past it, the least recently used
    /// answers go first, checked after each write. `None`: no limit. At
    /// least 1.
    #[uniffi(default = None)]
    pub cache_max_mb: Option<u64>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            mailto: None,
            openalex_key: None,
            cache_dir: None,
            openalex_daily_cap: None,
            disabled_sources: Vec::new(),
            cache_found_days: 7,
            cache_not_found_days: 1,
            cache_max_mb: None,
        }
    }
}

impl Config {
    /// How long cached answers are trusted, from the days given.
    fn cache_policy(&self) -> CachePolicy {
        let days = |n: u32| std::time::Duration::from_secs(u64::from(n) * 86_400);
        let mut policy = CachePolicy::default();
        policy.records = days(self.cache_found_days);
        policy.misses = days(self.cache_not_found_days);
        policy
    }
}

/// How patient a call is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Patience {
    /// Providers' own retries and deadlines: for batches.
    Batch,
    /// Someone is waiting: give up on a provider within seconds and fall
    /// back.
    Interactive,
    /// The cache only, however old.
    Offline,
    /// Asks the sources again, past the cache, and caches their answers:
    /// for checking again before submitting. Falls back to the cached
    /// answer, marked stale, where a source fails.
    Fresh,
}

impl Patience {
    /// The engine's options for this patience.
    fn options(self) -> CallOptions {
        match self {
            Patience::Batch => CallOptions::default(),
            Patience::Interactive => CallOptions::interactive(),
            Patience::Offline => CallOptions::offline(),
            // Batch deadlines and retries, past the cache.
            Patience::Fresh => CallOptions::fresh(),
        }
    }
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum AsperoError {
    #[error("could not set up: {0}")]
    Setup(String),
    /// Its [`CallToken`] was cancelled.
    #[error("cancelled")]
    Cancelled,
    /// A bug: the call panicked or its task was lost.
    #[error("internal error: {0}")]
    Internal(String),
    /// The cache file couldn't be read or written.
    #[error("cache error: {0}")]
    Cache(String),
}

/// Receives each verification as it finishes.
#[uniffi::export(with_foreign)]
pub trait VerifyListener: Send + Sync {
    /// One result; its `index` says which claim.
    fn on_result(&self, result: Verification);
}

/// Cancels the calls it's passed. One token may cancel several calls; a
/// call given a token already cancelled ends at once. Cancelled calls stop
/// their requests and throw [`AsperoError::Cancelled`].
#[derive(uniffi::Object)]
pub struct CallToken {
    cancelled: watch::Sender<bool>,
}

#[uniffi::export]
impl CallToken {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(CallToken {
            cancelled: watch::Sender::new(false),
        })
    }

    pub fn cancel(&self) {
        self.cancelled.send_replace(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.cancelled.borrow()
    }
}

impl CallToken {
    async fn cancelled(&self) {
        let _ = self.cancelled.subscribe().wait_for(|c| *c).await;
    }
}

#[derive(uniffi::Object)]
pub struct Aspero {
    engine: Engine,
    runtime: tokio::runtime::Runtime,
    cache: Option<Arc<SqliteCache>>,
    mailto: Option<String>,
}

#[uniffi::export]
impl Aspero {
    #[uniffi::constructor]
    pub fn new(config: Config) -> Result<Arc<Self>, AsperoError> {
        let setup = |e: String| AsperoError::Setup(e);
        let cache_policy = config.cache_policy();
        let max_bytes = match config.cache_max_mb {
            Some(0) => return Err(setup("the cache's size limit must be at least 1 MB".into())),
            mb => mb.map(|mb| mb.saturating_mul(1_000_000)),
        };
        let cache = match &config.cache_dir {
            Some(dir) => {
                let path = std::path::Path::new(dir).join("cache.sqlite");
                let cache = SqliteCache::open(&path).map_err(|e| setup(e.to_string()))?;
                cache
                    .set_max_bytes(max_bytes)
                    .map_err(|e| setup(e.to_string()))?;
                Some(Arc::new(cache))
            }
            None => None,
        };
        let mailto = config.mailto.filter(|m| !m.trim().is_empty());
        let key = config.openalex_key.filter(|k| !k.trim().is_empty());
        let mut standard_config = StandardConfig::all(key.map(SecretString::from));
        if let Some(cap) = config.openalex_daily_cap
            && !(cap.is_finite() && cap >= 0.0)
        {
            return Err(setup(format!(
                "the OpenAlex daily cap must be zero or more US dollars, not {cap}"
            )));
        }
        standard_config.openalex_daily_cap = config.openalex_daily_cap;
        standard_config
            .disable(&config.disabled_sources)
            .map_err(|e| setup(e.to_string()))?;
        standard_config.cache_policy = cache_policy;
        standard_config.mailto = mailto.clone();
        standard_config.pacing = cache.clone().map(|c| c as _);
        standard_config.cache = cache.clone().map(|c| c as _);
        let engine = standard(standard_config).map_err(|e| setup(e.to_string()))?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("aspero")
            .enable_all()
            .build()
            .map_err(|e| setup(e.to_string()))?;
        Ok(Arc::new(Aspero {
            engine,
            runtime,
            cache,
            mailto,
        }))
    }

    /// Looks up identifiers as written (DOIs, URLs, PMIDs, arXiv IDs, …).
    /// With `full`: notices in both directions, open access, disagreements
    /// with the DOI's registry, and the latest version.
    #[uniffi::method(default(cancel = None))]
    pub async fn lookup(
        &self,
        ids: Vec<String>,
        full: bool,
        patience: Patience,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<Lookups, AsperoError> {
        let engine = self.engine(patience);
        self.run(
            async move {
                let mut extras = Extras::default();
                extras.full = full;
                let report = engine.lookup_inputs(&ids, extras).await;
                Lookups {
                    results: ids
                        .iter()
                        .zip(&report.items)
                        .map(|(input, item)| Lookup::new(input, item))
                        .collect(),
                    usage: records::usage(&report.usage),
                }
            },
            cancel,
        )
        .await
    }

    /// The work a document is, from its first page's text: not the works
    /// it cites or links to. `title_hint` is the largest type at the top of
    /// the page, `title` the file's title metadata (ignored when junk).
    /// Each identifier on the page (header and footer first, at most six)
    /// must be confirmed by the title or the page; with none, the title
    /// and top of the page are searched. `None` when nothing is sure
    /// enough.
    #[uniffi::method(default(title = None, title_hint = None, cancel = None))]
    pub async fn identify(
        &self,
        first_page: String,
        title: Option<String>,
        title_hint: Option<String>,
        patience: Patience,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<Option<Identification>, AsperoError> {
        let engine = self.engine(patience);
        self.run(
            async move {
                let found = engine
                    .identify(&first_page, title.as_deref(), title_hint.as_deref())
                    .await?;
                let claim = InputClaim {
                    input: found.input(),
                    key: None,
                    claim: Ok(found.claim.clone()),
                };
                Some(Identification {
                    decided_by: found.decided_by.into(),
                    verification: Verification::new(0, &claim, &found.verification, false),
                })
            },
            cancel,
        )
        .await
    }

    /// Verifies citations, answering in claim order. With `full`, every
    /// candidate considered.
    #[uniffi::method(default(cancel = None))]
    pub async fn verify(
        &self,
        claims: Vec<ClaimRecord>,
        full: bool,
        patience: Patience,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<Verifications, AsperoError> {
        let engine = self.engine(patience);
        self.run(
            async move {
                let mut results: Vec<Option<Verification>> = vec![None; claims.len()];
                let usage = verify_all(&engine, claims, full, |v| {
                    let i = v.index as usize;
                    results[i] = Some(v);
                })
                .await;
                Verifications {
                    results: results.into_iter().flatten().collect(),
                    usage,
                }
            },
            cancel,
        )
        .await
    }

    /// As [`verify`](Self::verify), handing each result to `listener` as
    /// soon as it finishes, in any order. Returns the usage.
    #[uniffi::method(default(cancel = None))]
    pub async fn verify_each(
        &self,
        claims: Vec<ClaimRecord>,
        full: bool,
        patience: Patience,
        listener: Arc<dyn VerifyListener>,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<Vec<ProviderUsage>, AsperoError> {
        let engine = self.engine(patience);
        self.run(
            async move { verify_all(&engine, claims, full, |v| listener.on_result(v)).await },
            cancel,
        )
        .await
    }

    /// Searches one provider by keywords: `request.source`, or the first
    /// that searches. A failed search has no hits and says why.
    #[uniffi::method(default(cancel = None))]
    pub async fn search(
        &self,
        request: SearchRequest,
        patience: Patience,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<SearchResults, AsperoError> {
        let engine = self.engine(patience);
        self.run(
            async move {
                let mut query = SearchQuery::new(request.text);
                if request.year_from.is_some() || request.year_to.is_some() {
                    query.years = Some(YearRange {
                        from: request.year_from,
                        to: request.year_to,
                    });
                }
                query.work_type = request.work_type.as_deref().map(WorkType::from_csl);
                query.open_access = request.open_access_only.then_some(true);
                query.page.limit = request.limit;
                query.page.cursor = request.cursor.map(Cursor);
                query.author = request.author;
                query.title = request.title;
                query.container = request.container;
                query.sort = request.sort.map_or(Sort::Relevance, Into::into);
                query.extra = request.extra.into_iter().collect();
                query.tree = request.query.map(Into::into);
                match request.since.as_deref().map(SearchQuery::parse_since) {
                    Some(Err(reason)) => {
                        return SearchResults::new(Err(EngineError::InvalidQuery(reason)));
                    }
                    Some(Ok(since)) => query.since = Some(since),
                    None => {}
                }
                SearchResults::new(engine.search(request.source.as_deref(), &query).await)
            },
            cancel,
        )
        .await
    }

    /// A page of the works `id` (any identifier `lookup` takes) cites: up
    /// to `limit` from `cursor` (a previous page's `nextCursor`), each
    /// looked up, in OpenAlex's order. `coverage` is `unknown` when
    /// OpenAlex has no reference data for the work. A work that can't be
    /// resolved gives a `failure`.
    #[uniffi::method(default(cursor = None, cancel = None))]
    pub async fn references(
        &self,
        id: String,
        cursor: Option<String>,
        limit: u32,
        patience: Patience,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<ReferencesPage, AsperoError> {
        let engine = self.engine(patience);
        self.run(
            async move {
                let cursor = cursor.map(Cursor);
                let page = engine.references(&id, cursor.as_ref(), limit).await;
                ReferencesPage::new(&id, page)
            },
            cancel,
        )
        .await
    }

    /// Analyses of the set of works `seeds` names (identifiers as
    /// written): retracted references, odd ones out, probably missing
    /// works. Each seed resolves cheaply (asked once) or says why not;
    /// paid requests stop before `budget_usd` would be passed, and what
    /// stopped says how far it got. With `estimate_only`, nothing paid is
    /// asked: `estimate` says what a run would cost.
    #[uniffi::method(default(budget_usd = None, cancel = None))]
    pub async fn analyse(
        &self,
        seeds: Vec<String>,
        kinds: Vec<AnalysisKind>,
        budget_usd: Option<f64>,
        estimate_only: bool,
        patience: Patience,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<Analysis, AsperoError> {
        let engine = self.engine(patience);
        self.run(
            async move {
                let kinds: Vec<aspero_engine::AnalysisKind> =
                    kinds.into_iter().map(Into::into).collect();
                let analysis = engine
                    .analyse(&seeds, &kinds, budget_usd, estimate_only)
                    .await;
                Analysis::new(analysis)
            },
            cancel,
        )
        .await
    }

    /// Whether `source` (or, without one, any source: `search` then picks
    /// the first that can) can search by `query`, and with `since` (as
    /// `SearchRequest.since`), only works added since that day: `nil` if
    /// so, otherwise why, in English ("OpenAlex can't combine title and
    /// author with any", "OpenAlex can't search by date added (it needs a
    /// paid plan)"; without a source, the first's reason, ", and no other
    /// source can").
    /// Offline and instant. Searching anyway fails as `unsupported` with
    /// the same reason, or as `badRequest` for a value that can't be valid
    /// (an author id that's neither an ORCID nor an OpenAlex id, an unknown
    /// language code, a date that isn't one).
    #[uniffi::method(default(since = None))]
    pub fn search_supports(
        &self,
        source: Option<String>,
        query: SearchNode,
        since: Option<String>,
    ) -> Option<String> {
        let since = match since.as_deref().map(SearchQuery::parse_since) {
            Some(Err(reason)) => return Some(reason),
            Some(Ok(since)) => Some(since),
            None => None,
        };
        self.engine
            .search_supports_since(source.as_deref(), &query.into(), since)
    }

    /// What each source can search by, in the order `search` picks from:
    /// without a `source`, the first. Offline.
    pub fn search_capabilities(&self) -> Vec<SourceSearch> {
        self.engine
            .search_providers()
            .map(|p| SourceSearch::new(&p.id(), p.capabilities()))
            .collect()
    }

    /// The identifiers mentioned in `text`, in order, where and as written.
    /// Offline.
    pub fn scan(&self, text: String) -> Vec<Mention> {
        let mut utf16 = Utf16Offsets::new(&text);
        aspero_core::id::scan(&text)
            .iter()
            .map(|m| Mention::new(&text, m, &mut utf16))
            .collect()
    }

    /// The references in a pasted list (numbered, bulleted, one per
    /// paragraph or line; wrapped lines rejoined), each ready to verify.
    /// Offline.
    pub fn split_references(&self, text: String) -> Vec<Reference> {
        let mut utf16 = Utf16Offsets::new(&text);
        aspero_core::split_references(&text)
            .iter()
            .map(|r| Reference::new(r, &mut utf16))
            .collect()
    }

    /// A BibTeX/BibLaTeX, RIS or CSL-JSON file's entries, each a claim to
    /// verify; the format is told from the contents. Offline.
    pub fn read_references(&self, text: String) -> Result<Bibliography, ReadError> {
        let (format, entries) = aspero_bib::read(&text)?;
        Ok(Bibliography {
            format: format.into(),
            entries: entries.into_iter().map(Into::into).collect(),
        })
    }

    /// The citations Zotero or Mendeley keep in a Word document (.docx),
    /// each work once as a claim to verify, and the document's text.
    /// Throws `ReadError.Unrecognised` for anything but a .docx file.
    /// Offline.
    pub fn read_docx(&self, data: Vec<u8>) -> Result<DocxReferences, ReadError> {
        Ok(aspero_bib::docx::read_docx(&data)?.into())
    }

    /// Works as BibTeX entries (keys kept distinct) or RIS records, one
    /// after another. Offline.
    pub fn export(&self, works: Vec<Work>, format: ExportFormat) -> String {
        let mut keys = aspero_bib::Keys::default();
        let entries: Vec<String> = works
            .iter()
            .map(|work| {
                let (w, ids) = work.to_core();
                match format {
                    ExportFormat::Ris => aspero_bib::ris(&w, &ids),
                    ExportFormat::Bibtex => aspero_bib::bibtex(&w, &ids, &keys.key(&w)),
                }
            })
            .collect();
        entries.join("\n")
    }

    /// The bundled citation styles: APA, Vancouver, Harvard, Chicago,
    /// IEEE, Nature, AMA, MLA.
    pub fn citation_styles(&self) -> Vec<CitationStyle> {
        aspero_bib::cite::BUNDLED
            .iter()
            .map(|b| CitationStyle {
                id: b.id.into(),
                title: b.title.into(),
            })
            .collect()
    }

    /// Works as a reference list in `style`, one entry each, in the
    /// style's order (alphabetical, or as given). `locale` (`en-GB`) sets
    /// the language of terms like "et al."; by default, the style's own.
    /// Offline.
    #[uniffi::method(default(locale = None))]
    pub fn cite(
        &self,
        works: Vec<Work>,
        style: StyleSource,
        format: CiteFormat,
        locale: Option<String>,
    ) -> Result<Vec<CitedEntry>, StyleError> {
        use aspero_bib::cite;
        let style = match style {
            StyleSource::Bundled { id } => cite::Style::bundled(&id)?,
            StyleSource::Csl { xml } => cite::Style::from_xml(&xml)?,
        };
        let core: Vec<_> = works.iter().map(Work::to_core).collect();
        let pairs: Vec<_> = core.iter().map(|(w, ids)| (w, &ids[..])).collect();
        let markup = match format {
            CiteFormat::Text => cite::Markup::Text,
            CiteFormat::Html => cite::Markup::Html,
        };
        Ok(cite::cite(&pairs, &style, locale.as_deref(), markup)
            .into_iter()
            .map(|c| CitedEntry {
                index: c.index as u32,
                label: c.label,
                text: c.text,
                in_text: c.in_text,
            })
            .collect())
    }

    /// The bundled styles' and locales' license (CC BY-SA 3.0) and
    /// authors, to show with formatted references.
    pub fn style_credits(&self) -> String {
        aspero_bib::cite::credits()
    }

    /// What the library is built from, by name (ignoring case): every
    /// crate (its normal dependencies, for macOS) with its licence, and
    /// the bundled citation style data. Offline; generated at release
    /// (`just notices`).
    pub fn dependencies(&self) -> Vec<Dependency> {
        notices::dependencies().to_vec()
    }

    /// The full third-party notices: each licence text once, after the
    /// crates under it and their copyright lines, then the citation style
    /// data's licence and authors. For an app's acknowledgements. Offline.
    pub fn third_party_notices(&self) -> String {
        notices::NOTICES.to_owned()
    }

    /// Each provider's budget as its last response reported it, without
    /// spending any: OpenAlex's, once it has been called (or a key
    /// checked) since launch.
    pub fn budgets(&self) -> Vec<Budget> {
        self.engine
            .budgets()
            .iter()
            .map(|(p, b)| Budget::new(p, b))
            .collect()
    }

    /// Asks OpenAlex what's used and left of today's budget, for free
    /// (`/rate-limit`, with a key), then answers as `budgets` does. Without
    /// a key, or `offline`, nothing is asked: the budgets are as the last
    /// responses left them.
    #[uniffi::method(default(cancel = None))]
    pub async fn refresh_budgets(
        &self,
        patience: Patience,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<Vec<Budget>, AsperoError> {
        let engine = self.engine(patience);
        self.run(
            async move {
                engine
                    .refresh_budgets()
                    .await
                    .iter()
                    .map(|(p, b)| Budget::new(p, b))
                    .collect()
            },
            cancel,
        )
        .await
    }

    /// Whether OpenAlex accepts `key`, with one free lookup (never
    /// cached), and what it has left. Doesn't change the handle's key.
    #[uniffi::method(default(cancel = None))]
    pub async fn check_openalex_key(
        &self,
        key: String,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<KeyCheck, AsperoError> {
        let mailto = self.mailto.clone();
        let pacing = self.cache.clone().map(|c| c as _);
        let checked = self
            .run(
                async move { aspero_engine::check_openalex_key(&key, mailto, pacing).await },
                cancel,
            )
            .await?;
        let check = checked.map_err(|e| AsperoError::Setup(e.to_string()))?;
        Ok(KeyCheck {
            status: check.status.into(),
            message: check.message,
            budget: check.budget.map(|(p, b)| Budget::new(&p, &b)),
        })
    }

    /// What the cache holds; `None` without one.
    pub fn cache_stats(&self) -> Result<Option<CacheStats>, AsperoError> {
        let Some(cache) = &self.cache else {
            return Ok(None);
        };
        let s = cache
            .stats()
            .map_err(|e| AsperoError::Cache(e.to_string()))?;
        Ok(Some(CacheStats {
            records: s.records,
            identifiers: s.identifiers,
            misses: s.misses,
            searches: s.searches,
            bytes: s.bytes,
        }))
    }

    /// Empties the cache.
    pub fn clear_cache(&self) -> Result<(), AsperoError> {
        match &self.cache {
            Some(cache) => cache.clear().map_err(|e| AsperoError::Cache(e.to_string())),
            None => Ok(()),
        }
    }

    /// The references in a list's lines as laid out (from PDFKit), using
    /// their indents: hanging or first-line indents mark where references
    /// start when the text alone can't. Numbered lists, and lines all at
    /// one indent, are split as `splitReferences` splits text. Offline.
    pub fn split_lines(&self, lines: Vec<Line>) -> Vec<LineReference> {
        let lines: Vec<aspero_core::IndentedLine> = lines
            .into_iter()
            .map(|l| aspero_core::IndentedLine::new(l.text, l.indent))
            .collect();
        aspero_core::split_lines(&lines)
            .iter()
            .map(Into::into)
            .collect()
    }

    /// Providers refusing calls now, and why: a spent budget, a rejected
    /// key, repeated failures.
    pub fn unavailable(&self) -> Vec<Unavailable> {
        self.engine
            .unavailable()
            .into_iter()
            .map(|(provider, e)| Unavailable {
                provider: provider.to_string(),
                kind: e.kind().into(),
                message: e.to_string(),
                until: match e {
                    Error::QuotaExhausted { resets_at: t } | Error::CircuitOpen { until: t } => {
                        t.map(Into::into)
                    }
                    _ => None,
                },
            })
            .collect()
    }
}

impl Aspero {
    fn engine(&self, patience: Patience) -> Engine {
        self.engine.with_options(patience.options())
    }

    /// Runs `work` on the handle's runtime until it finishes or `cancel`
    /// is cancelled; dropping the returned future (where the caller's
    /// executor does) also cancels it.
    async fn run<T: Send + 'static>(
        &self,
        work: impl Future<Output = T> + Send + 'static,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<T, AsperoError> {
        if cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
            return Err(AsperoError::Cancelled);
        }
        let mut task = Abortable(self.runtime.spawn(work));
        let cancelled = async {
            match &cancel {
                Some(token) => token.cancelled().await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            biased;
            () = cancelled => Err(AsperoError::Cancelled),
            done = &mut task.0 => done.map_err(|e| AsperoError::Internal(e.to_string())),
        }
    }
}

/// Aborts its task when dropped.
struct Abortable<T>(JoinHandle<T>);

impl<T> Drop for Abortable<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Verifies every claim, handing each result to `each` as it finishes:
/// unreadable claims first, then the rest as they complete.
async fn verify_all(
    engine: &Engine,
    claims: Vec<ClaimRecord>,
    full: bool,
    mut each: impl FnMut(Verification),
) -> Vec<ProviderUsage> {
    let claims: Vec<InputClaim> = claims.into_iter().map(ClaimRecord::into_input).collect();
    let (positions, valid) = InputClaim::valid(&claims);
    for (i, claim) in claims.iter().enumerate() {
        if let Err(e) = &claim.claim {
            each(Verification::invalid(i, claim, e));
        }
    }
    let mut verified = engine.verify_batch(&valid, aspero_engine::SEARCH_LIMIT);
    while let Some((k, v)) = verified.next().await {
        let i = positions[k];
        each(Verification::new(i, &claims[i], &v, full));
    }
    records::usage(verified.usage())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_days_set_how_long_answers_are_trusted() {
        const DAY: u64 = 86_400;
        let default = Config::default().cache_policy();
        assert_eq!(default, CachePolicy::default());
        let config = Config {
            cache_found_days: 3,
            cache_not_found_days: 0,
            ..Config::default()
        };
        let policy = config.cache_policy();
        assert_eq!(policy.records.as_secs(), 3 * DAY);
        assert_eq!(policy.misses.as_secs(), 0);
        // The rest keep their defaults.
        assert_eq!(policy.searches, default.searches);
        assert_eq!(policy.references, default.references);
        assert_eq!(policy.cited_by, default.cited_by);
    }

    #[test]
    fn each_patience_has_its_options() {
        let batch = CallOptions::default();
        for patience in [
            Patience::Batch,
            Patience::Interactive,
            Patience::Offline,
            Patience::Fresh,
        ] {
            let options = patience.options();
            match patience {
                Patience::Batch => assert_eq!(options, batch),
                Patience::Interactive => assert_eq!(options, CallOptions::interactive()),
                Patience::Offline => assert!(options.offline && !options.fresh),
                Patience::Fresh => {
                    assert!(options.fresh && !options.offline);
                    // Waits as a batch does.
                    assert_eq!(options.deadline, batch.deadline);
                    assert_eq!(options.max_retries, batch.max_retries);
                }
            }
        }
    }
}
