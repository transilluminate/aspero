//! Citation links: references pages and analyses of a set, against a
//! scripted OpenAlex that charges as the real one does.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use aspero_core::{
    Capabilities, Cursor, Error, FieldSet, Hit, IdKind, Lookup, Page, PartialDate, Person,
    Provider, ProviderId, Raw, ResiliencePolicy, Response, Result, SearchQuery, SourceRecord,
    Update, UpdateKind, Usage, Work, WorkId,
};
use aspero_engine::{
    Analysis, AnalysisKind, Cache, Engine, IncompleteKind, OddOne, OddReason, SeedProblem,
    SqliteCache,
};
use async_trait::async_trait;
use jiff::Timestamp;

const OPENALEX: ProviderId = ProviderId::from_static("openalex");
const CROSSREF: ProviderId = ProviderId::from_static("crossref");

/// OpenAlex as the engine sees it: lookups of one id free, batches and
/// filters $0.0001, the retraction filter answered from the works it
/// knows.
struct Alex {
    caps: Capabilities,
    works: Vec<SourceRecord>,
    failures: Mutex<VecDeque<Error>>,
    lookups: Mutex<Vec<Vec<WorkId>>>,
    searches: Mutex<Vec<String>>,
    /// Identifiers its lookups don't find works by.
    blind: Vec<WorkId>,
}

impl Alex {
    fn new(works: Vec<SourceRecord>) -> Self {
        let mut caps = Capabilities::default();
        caps.lookup_kinds = vec![IdKind::Doi, IdKind::Arxiv, IdKind::Pmid, IdKind::OpenAlex];
        caps.max_batch = 100;
        caps.fields = FieldSet::ALL;
        caps.search = true;
        Alex {
            caps,
            works,
            failures: Mutex::default(),
            lookups: Mutex::default(),
            searches: Mutex::default(),
            blind: Vec::new(),
        }
    }

    /// Finds no work by `ids`, as OpenAlex's batch filter finds none by
    /// a DOI it doesn't index.
    fn blind(mut self, ids: &[&str]) -> Self {
        self.blind = ids.iter().map(|i| i.parse().unwrap()).collect();
        self
    }

    fn failing(self, errors: impl IntoIterator<Item = Error>) -> Self {
        self.failures.lock().unwrap().extend(errors);
        self
    }

    fn lookups(&self) -> Vec<usize> {
        self.lookups.lock().unwrap().iter().map(Vec::len).collect()
    }

    fn searches(&self) -> Vec<String> {
        self.searches.lock().unwrap().clone()
    }

    /// A page of the works citing any of `cited` (`W1|W2`), as OpenAlex
    /// answers `select=id,referenced_works`: lean works, most cited first
    /// (ties by id), a cursor to the next page.
    fn citing(&self, cited: &str, query: &SearchQuery) -> Page {
        assert_eq!(
            query.extra.get("select").map(String::as_str),
            Some("id,referenced_works")
        );
        assert_eq!(
            query.extra.get("sort").map(String::as_str),
            Some("cited_by_count:desc")
        );
        assert_eq!(query.page.limit, 200);
        let cited: Vec<WorkId> = cited.split('|').map(|w| w.parse().unwrap()).collect();
        let mut citing: Vec<&SourceRecord> = self
            .works
            .iter()
            .filter(|w| w.references.iter().flatten().any(|r| cited.contains(r)))
            .collect();
        citing.sort_by(|a, b| {
            let count = |w: &SourceRecord| w.work.cited_by_count.unwrap_or(0);
            count(b).cmp(&count(a)).then(a.ids[0].cmp(&b.ids[0]))
        });
        let start: usize = query
            .page
            .cursor
            .as_ref()
            .map_or(0, |c| c.0.parse().unwrap());
        let end = (start + query.page.limit as usize).min(citing.len());
        let hits = citing[start..end]
            .iter()
            .map(|w| {
                let lean = SourceRecord::new(
                    OPENALEX,
                    vec![w.ids[0].clone()],
                    Work::default(),
                    FieldSet::ALL,
                    Raw::json(&serde_json::Value::Null),
                    Timestamp::UNIX_EPOCH,
                )
                .with_references(w.references.clone());
                Hit::new(lean, None)
            })
            .collect();
        let next = (end < citing.len()).then(|| Cursor(end.to_string()));
        Page::new(hits, Some(citing.len() as u64), next)
    }

    /// The `cites:` filters asked for, in order.
    fn citing_pages(&self) -> Vec<String> {
        self.searches()
            .into_iter()
            .filter(|f| f.starts_with("cites:"))
            .collect()
    }
}

fn usage(cost: f64) -> Usage {
    let mut usage = Usage::default();
    usage.cost = Some(cost);
    usage
}

#[async_trait]
impl Provider for Alex {
    fn id(&self) -> ProviderId {
        OPENALEX
    }

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    async fn lookup(&self, ids: &[WorkId], _: FieldSet) -> Result<Response<Vec<Lookup>>> {
        self.lookups.lock().unwrap().push(ids.to_vec());
        if let Some(e) = self.failures.lock().unwrap().pop_front() {
            return Err(e);
        }
        let lookups = ids
            .iter()
            .map(|id| Lookup {
                id: id.clone(),
                record: self
                    .works
                    .iter()
                    .find(|w| w.ids.contains(id) && !self.blind.contains(id))
                    .cloned(),
            })
            .collect();
        let cost = if ids.len() > 1 { 0.0001 } else { 0.0 };
        Ok(Response::with_usage(lookups, usage(cost)))
    }

    async fn search(&self, query: &SearchQuery) -> Result<Response<Page>> {
        let filter = query.extra.get("filter").cloned().unwrap_or_default();
        self.searches.lock().unwrap().push(filter.clone());
        if let Some(e) = self.failures.lock().unwrap().pop_front() {
            return Err(e);
        }
        if let Some(cited) = filter.strip_prefix("cites:") {
            return Ok(Response::with_usage(
                self.citing(cited, query),
                usage(0.0001),
            ));
        }
        let ids = filter
            .strip_prefix("openalex:")
            .and_then(|f| f.strip_suffix(",is_retracted:true"))
            .expect("a retraction check");
        let ids: Vec<WorkId> = ids.split('|').map(|w| w.parse().unwrap()).collect();
        let hits = self
            .works
            .iter()
            .filter(|w| w.ids.iter().any(|i| ids.contains(i)) && w.work.retraction().is_some())
            .map(|w| Hit::new(w.clone(), None))
            .collect();
        Ok(Response::with_usage(
            Page::new(hits, None, None),
            usage(0.0001),
        ))
    }
}

fn work(
    ids: &[&str],
    title: &str,
    author: &str,
    year: i16,
    cites: Option<&[&str]>,
) -> SourceRecord {
    let mut w = Work::default();
    w.title = Some(title.into());
    w.authors = vec![Person::literal(author)];
    w.issued = Some(PartialDate::from_year(year));
    SourceRecord::new(
        OPENALEX,
        ids.iter().map(|i| i.parse().unwrap()).collect(),
        w,
        FieldSet::ALL,
        Raw {
            media_type: "application/json".into(),
            body: "{}".into(),
        },
        Timestamp::UNIX_EPOCH,
    )
    .with_references(cites.map(|c| c.iter().map(|i| i.parse().unwrap()).collect()))
}

fn retracted(mut record: SourceRecord, source: &str) -> SourceRecord {
    let mut update = Update::new(UpdateKind::Retraction);
    update.source = Some(source.into());
    record.work.updates.push(update);
    record
}

fn policy() -> ResiliencePolicy {
    let mut p = ResiliencePolicy::default();
    p.min_interval = std::time::Duration::ZERO;
    p
}

fn engine(alex: Arc<Alex>, cache: Option<Arc<SqliteCache>>) -> Engine {
    let mut builder = Engine::builder().provider(alex, policy(), 10);
    if let Some(cache) = cache {
        builder = builder.cache(cache);
    }
    builder.build()
}

fn inputs(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

/// A set: a preprint (W1) and its published version (W2: the same title,
/// first author and nearly the year), and two more papers; and works they
/// cite.
fn library() -> Vec<SourceRecord> {
    vec![
        work(
            &["W1", "10.48550/arXiv.2001.00001", "arXiv:2001.00001"],
            "Sepsis in mice",
            "Jane Doe",
            2020,
            Some(&["W10", "W11", "W20", "W30"]),
        ),
        work(
            &["W2", "10.1000/published"],
            "Sepsis in mice",
            "Jane Doe",
            2021,
            Some(&["W10"]),
        ),
        work(
            &["W3", "10.1000/b"],
            "Fluids in sepsis",
            "Ann Smith",
            2022,
            Some(&["W10", "W11", "W12", "W31"]),
        ),
        work(
            &["W4", "10.1000/c"],
            "Vasopressors in sepsis",
            "Bo Chen",
            2023,
            Some(&["W11", "W12", "W13", "W20", "W30", "W31"]),
        ),
        // Cited works. W20 is seed 2's own preprint: in the set already.
        work(&["W10"], "Ten", "X Ten", 2010, None),
        work(&["W11"], "Eleven", "X Eleven", 2011, None),
        work(&["W12"], "Twelve", "X Twelve", 2012, None),
        work(&["W13"], "Thirteen", "X Thirteen", 2013, None),
        work(
            &["W20", "10.1101/b"],
            "Fluids in sepsis",
            "Ann Smith",
            2021,
            None,
        ),
        // One work twice: a preprint (W30) and its journal version (W31).
        work(
            &["W30", "10.1101/thirty"],
            "Thirty years of sepsis",
            "Y Thirty",
            2019,
            None,
        ),
        work(
            &["W31", "10.1000/thirty"],
            "Thirty years of sepsis",
            "Y Thirty",
            2020,
            None,
        ),
    ]
}

const SEEDS: [&str; 4] = [
    "arXiv:2001.00001",
    "10.1000/published",
    "10.1000/b",
    "10.1000/c",
];

#[tokio::test(start_paused = true)]
async fn probably_missing_ranks_by_seeds_citing_and_leaves_out_the_set() {
    let alex = Arc::new(Alex::new(library()));
    let engine = engine(alex.clone(), None);
    let kinds = [AnalysisKind::ProbablyMissing { limit: 10 }];
    let analysis = engine
        .analyse(&inputs(&SEEDS), &kinds, None, false)
        .await
        .unwrap();

    // The preprint and its published version are one work of the set.
    let same: Vec<Option<usize>> = analysis.seeds.iter().map(|s| s.same_as).collect();
    assert_eq!(same, [None, Some(0), None, None]);
    assert_eq!(analysis.with_references, 3);

    let missing = analysis.probably_missing.unwrap();
    assert!(missing.complete());
    let found: Vec<(String, Vec<String>, Vec<usize>)> = missing
        .items
        .iter()
        .map(|m| {
            let ids = m.ids.iter().map(ToString::to_string).collect();
            (m.id.to_string(), ids, m.cited_by.clone())
        })
        .collect();
    let w = |s: &str| format!("openalex:{s}");
    assert_eq!(
        found,
        [
            // Cited by all three works of the set (W1 and W2 count once).
            (w("W11"), vec![w("W11")], vec![0, 2, 3]),
            // Two versions of one work, merged: cited by all three.
            (w("W30"), vec![w("W30"), w("W31")], vec![0, 2, 3]),
            (w("W10"), vec![w("W10")], vec![0, 2]),
            (w("W12"), vec![w("W12")], vec![2, 3]),
            // W13 is cited once; W20 is seed 2's preprint, so in the set.
        ]
    );
    // Seeds in one batch per kind (arXiv IDs are DOIs there), then the
    // candidates in one.
    assert_eq!(alex.lookups(), [4, 6]);
}

#[tokio::test(start_paused = true)]
async fn odd_ones_out_are_coverage_aware() {
    let mut works = library();
    works.push(work(
        &["W5", "10.1000/off"],
        "Knitting",
        "Q Wool",
        2020,
        Some(&["W99"]),
    ));
    works.push(work(
        &["W6", "10.1000/book"],
        "A book",
        "R Page",
        2001,
        None,
    ));
    // No references known, but seed 3 cites it.
    works.push(work(
        &["W7", "10.1000/cited"],
        "Cited",
        "S Cite",
        2001,
        None,
    ));
    works[3]
        .references
        .as_mut()
        .unwrap()
        .push("W7".parse().unwrap());
    let alex = Arc::new(Alex::new(works));
    let mut seeds = SEEDS.to_vec();
    seeds.extend(["10.1000/off", "10.1000/book", "10.1000/cited"]);
    let analysis = engine(alex, None)
        .analyse(&inputs(&seeds), &[AnalysisKind::OddOneOut], None, false)
        .await
        .unwrap();
    let odd = analysis.odd_one_out.unwrap();
    assert_eq!(
        odd.items,
        [
            OddOne::new(4, OddReason::NoLinks),
            OddOne::new(5, OddReason::NoReferenceData),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn retracted_references_come_from_openalex_and_cached_notices() {
    let mut works = library();
    works.push(work(&["W40"], "Forty", "Z", 2015, None));
    let fortieth = works.len() - 1;
    works[fortieth] = retracted(works[fortieth].clone(), "openalex");
    works[2]
        .references
        .as_mut()
        .unwrap()
        .push("W40".parse().unwrap());
    // W41's retraction is known only to Crossref, from an earlier lookup.
    let w41 = work(&["W41", "10.1000/w41"], "Forty-one", "Z", 2016, None);
    works.push(w41.clone());
    works[3]
        .references
        .as_mut()
        .unwrap()
        .push("W41".parse().unwrap());

    let cache = Arc::new(SqliteCache::open_in_memory().unwrap());
    let now = Timestamp::now();
    let id41: WorkId = "W41".parse().unwrap();
    cache
        .put_lookups(&OPENALEX, &[(&id41, Some(&w41))], now)
        .unwrap();
    let doi: WorkId = "10.1000/w41".parse().unwrap();
    let mut registered = retracted(w41.clone(), "retraction-watch");
    registered.source = CROSSREF;
    cache
        .put_lookups(&CROSSREF, &[(&doi, Some(&registered))], now)
        .unwrap();

    let alex = Arc::new(Alex::new(works));
    // A registry, so its cached notices count; never asked.
    let mut crossref = aspero_core::Capabilities::default();
    crossref.notices = true;
    let registry = Arc::new(Registry(crossref));
    let engine = Engine::builder()
        .provider(alex.clone(), policy(), 10)
        .provider(registry, policy(), 20)
        .cache(cache)
        .build();
    let analysis = engine
        .analyse(
            &inputs(&SEEDS),
            &[AnalysisKind::RetractedReferences],
            None,
            false,
        )
        .await
        .unwrap();
    let found = analysis.retracted_references.unwrap();
    assert!(found.complete());
    let pairs: Vec<(usize, String, String)> = found
        .items
        .iter()
        .map(|r| (r.seed, r.cites.ids[0].to_string(), r.source.clone()))
        .collect();
    assert_eq!(
        pairs,
        [
            (2, "openalex:W40".into(), "openalex".into()),
            (3, "openalex:W41".into(), "retraction-watch".into()),
        ]
    );
    // The union of the seeds' references, in one filter.
    let searches = alex.searches();
    assert_eq!(searches.len(), 1);
    assert!(searches[0].starts_with("openalex:W10|W11|W12|W13|W20|W30|W31|W40|W41,"));
}

/// A registry that is never asked: its cached records are what count.
struct Registry(Capabilities);

#[async_trait]
impl Provider for Registry {
    fn id(&self) -> ProviderId {
        CROSSREF
    }

    fn capabilities(&self) -> &Capabilities {
        &self.0
    }
}

#[tokio::test(start_paused = true)]
async fn a_budget_stops_paid_calls_and_says_how_far_it_got() {
    let alex = Arc::new(Alex::new(library()));
    let engine = engine(alex.clone(), None);
    let kinds = [
        AnalysisKind::RetractedReferences,
        AnalysisKind::ProbablyMissing { limit: 5 },
    ];
    // Enough for the seeds' batch only.
    let analysis = engine
        .analyse(&inputs(&SEEDS), &kinds, Some(0.0001), false)
        .await
        .unwrap();
    assert!(analysis.seeds.iter().all(|s| s.problem.is_none()));
    assert!(!analysis.complete());

    let missing = analysis.probably_missing.unwrap();
    let stop = missing.incomplete.unwrap();
    assert_eq!(stop.kind, IncompleteKind::Budget);
    assert_eq!((stop.done, stop.total), (0, 6));
    // Ranked all the same, from the seeds' references (free), unlooked-up.
    assert_eq!(missing.items.len(), 5);
    assert!(missing.items.iter().all(|m| m.record.is_none()));

    let retracted = analysis.retracted_references.unwrap();
    let stop = retracted.incomplete.unwrap();
    assert_eq!(stop.kind, IncompleteKind::Budget);
    assert_eq!((stop.done, stop.total), (0, 7));
    assert!(alex.searches().is_empty());
    assert_eq!(alex.lookups(), [4]);
    let spent: f64 = analysis.usage.iter().filter_map(|(_, u)| u.cost).sum();
    assert!((spent - 0.0001).abs() < 1e-12);
}

#[tokio::test(start_paused = true)]
async fn estimates_make_no_paid_calls() {
    let mut works = library();
    // 150 more references: two retraction checks in all.
    let many: Vec<String> = (1000..1150).map(|n| format!("W{n}")).collect();
    works[3]
        .references
        .as_mut()
        .unwrap()
        .extend(many.iter().map(|w| w.parse::<WorkId>().unwrap()));
    let alex = Arc::new(Alex::new(works));
    let cache = Arc::new(SqliteCache::open_in_memory().unwrap());
    let engine = engine(alex.clone(), Some(cache));
    let kinds = [
        AnalysisKind::RetractedReferences,
        AnalysisKind::OddOneOut,
        AnalysisKind::ProbablyMissing { limit: 5 },
    ];
    let analysis = engine
        .analyse(&inputs(&SEEDS), &kinds, None, true)
        .await
        .unwrap();
    // Seeds one at a time (free); nothing else asked.
    assert_eq!(alex.lookups(), [1, 1, 1, 1]);
    assert!(alex.searches().is_empty());
    let estimate = analysis.estimate.unwrap();
    // Two retraction checks (157 references) and one batch of candidates.
    assert_eq!(estimate.requests, 3);
    assert!((estimate.usd - 0.0003).abs() < 1e-12);
    assert!(estimate.exact);
    assert!(analysis.retracted_references.is_none() && analysis.probably_missing.is_none());
    let spent: f64 = analysis.usage.iter().filter_map(|(_, u)| u.cost).sum();
    assert_eq!(spent, 0.0);

    // The run finds the seeds cached and spends what was estimated.
    let run = engine
        .analyse(&inputs(&SEEDS), &kinds, None, false)
        .await
        .unwrap();
    assert!(run.complete());
    let spent: f64 = run.usage.iter().filter_map(|(_, u)| u.cost).sum();
    assert!((spent - 0.0003).abs() < 1e-12, "{spent}");
    assert_eq!(alex.lookups(), [1, 1, 1, 1, 6]);
}

#[tokio::test(start_paused = true)]
async fn unresolvable_seeds_say_why_and_are_asked_once() {
    let alex = Arc::new(Alex::new(library()).failing([Error::Server {
        status: 503,
        message: None,
    }]));
    let engine = engine(alex.clone(), None);
    let seeds = inputs(&[
        "10.1000/b",
        "10.1000/c",
        "pmid:123",
        "pmid:456",
        "isbn:9780262033848",
        "not an id",
    ]);
    let analysis = engine
        .analyse(&seeds, &[AnalysisKind::OddOneOut], None, false)
        .await
        .unwrap();
    let problems: Vec<Option<SeedProblem>> = analysis.seeds.iter().map(|s| s.problem).collect();
    assert_eq!(
        problems,
        [
            Some(SeedProblem::ProviderFailed),
            Some(SeedProblem::ProviderFailed),
            Some(SeedProblem::NotFound),
            Some(SeedProblem::NotFound),
            Some(SeedProblem::UnsupportedId),
            Some(SeedProblem::InvalidId),
        ]
    );
    // The DOIs' batch failed once and wasn't retried; the PMIDs' batch
    // answered.
    assert_eq!(alex.lookups(), [2, 2]);
    assert!(!analysis.complete());
}

#[tokio::test(start_paused = true)]
async fn references_page_through_a_work_and_say_when_unknown() {
    let alex = Arc::new(Alex::new(library()));
    let cache = Arc::new(SqliteCache::open_in_memory().unwrap());
    let engine = engine(alex.clone(), Some(cache.clone()));

    let page = engine.references("10.1000/c", None, 2).await.unwrap();
    assert_eq!(page.work.to_string(), "openalex:W4");
    assert_eq!(page.coverage.as_str(), "known");
    assert_eq!(page.total, Some(6));
    let ids: Vec<String> = page.references.iter().map(|r| r.id.to_string()).collect();
    assert_eq!(ids, ["openalex:W11", "openalex:W12"]);
    assert!(page.references.iter().all(|r| r.record.is_some()));
    let next = page.next_cursor.unwrap();
    assert_eq!(next.0, "2");

    // By its OpenAlex id, its references are cached: only the page is
    // looked up (in one batch).
    let before = alex.lookups().len();
    let last = engine
        .references("W4", Some(&aspero_core::Cursor("4".into())), 2)
        .await
        .unwrap();
    assert_eq!(last.references.len(), 2);
    assert!(last.next_cursor.is_none());
    assert!(last.record.is_none());
    assert_eq!(alex.lookups()[before..], [2]);

    let unknown = engine.references("W10", None, 25).await.unwrap();
    assert_eq!(unknown.coverage.as_str(), "unknown");
    assert_eq!(unknown.total, None);
    assert!(unknown.references.is_empty());

    let edges = cache
        .get_edges(
            aspero_engine::Direction::References,
            &["W4".parse().unwrap()],
        )
        .unwrap();
    assert_eq!(edges[0].as_ref().unwrap().value.ids.len(), 6);
}

/// A DOI registry (as Crossref): free lookups of the works it knows.
struct Doiorg {
    caps: Capabilities,
    works: Vec<SourceRecord>,
    lookups: Mutex<Vec<Vec<WorkId>>>,
}

impl Doiorg {
    fn new(works: Vec<SourceRecord>) -> Self {
        let mut caps = Capabilities::default();
        caps.lookup_kinds = vec![IdKind::Doi];
        caps.max_batch = 20;
        Doiorg {
            caps,
            works,
            lookups: Mutex::default(),
        }
    }
}

#[async_trait]
impl Provider for Doiorg {
    fn id(&self) -> ProviderId {
        CROSSREF
    }

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    async fn lookup(&self, ids: &[WorkId], _: FieldSet) -> Result<Response<Vec<Lookup>>> {
        self.lookups.lock().unwrap().push(ids.to_vec());
        let lookups = ids
            .iter()
            .map(|id| Lookup {
                id: id.clone(),
                record: self.works.iter().find(|w| w.ids.contains(id)).cloned(),
            })
            .collect();
        Ok(Response::new(lookups))
    }
}

/// The library, plus a work OpenAlex holds under its new DOI (W90: the
/// journal changed publisher), which the registry knows by its old DOI and
/// PMID; and a work (W91) carrying a DOI OpenAlex's lookups miss and no
/// other source knows. Seeds 2 and 3 cite both.
fn moved() -> (Vec<SourceRecord>, Vec<SourceRecord>) {
    let mut works = library();
    for seed in [2, 3] {
        let cited = works[seed].references.as_mut().unwrap();
        cited.extend(["W90", "W91"].map(|w| w.parse::<WorkId>().unwrap()));
    }
    works.push(work(
        &["W90", "10.2000/new", "pmid:777"],
        "Triage in the emergency department",
        "Kim Lee",
        2004,
        None,
    ));
    works.push(work(
        &["W91", "10.2000/gone"],
        "A work by a dead DOI",
        "Ola Nord",
        2009,
        None,
    ));
    let mut old = work(
        &["10.2000/old", "pmid:777"],
        "Triage in the emergency department",
        "Kim Lee",
        2004,
        None,
    );
    old.source = CROSSREF;
    (works, vec![old])
}

fn moved_engine(cache: Option<Arc<SqliteCache>>) -> (Engine, Arc<Alex>, Arc<Doiorg>) {
    let (works, registered) = moved();
    // OpenAlex's batch filter finds nothing by the old DOI, nor by the
    // dead one.
    let alex = Arc::new(Alex::new(works).blind(&["10.2000/gone"]));
    let doiorg = Arc::new(Doiorg::new(registered));
    let mut builder = Engine::builder()
        .provider(alex.clone(), policy(), 10)
        .provider(doiorg.clone(), policy(), 20);
    if let Some(cache) = cache {
        builder = builder.cache(cache);
    }
    (builder.build(), alex, doiorg)
}

const MOVED_SEEDS: [&str; 7] = [
    "arXiv:2001.00001",
    "10.1000/published",
    "10.1000/b",
    "10.1000/c",
    "10.2000/old",
    "10.2000/gone",
    "pmid:404",
];

#[tokio::test(start_paused = true)]
async fn seeds_missed_by_their_doi_resolve_as_lookup_finds_them() {
    let (engine, alex, doiorg) = moved_engine(None);
    let kinds = [AnalysisKind::ProbablyMissing { limit: 20 }];
    let analysis = engine
        .analyse(&inputs(&MOVED_SEEDS), &kinds, None, false)
        .await
        .unwrap();
    let old = &analysis.seeds[4];
    assert_eq!(old.problem, None, "{:?}", old.message);
    assert_eq!(old.work().unwrap().to_string(), "openalex:W90");
    // The registry was asked about the DOIs OpenAlex missed, once.
    let asked: Vec<Vec<String>> = doiorg
        .lookups
        .lock()
        .unwrap()
        .iter()
        .map(|b| b.iter().map(ToString::to_string).collect())
        .collect();
    assert_eq!(asked, [["doi:10.2000/old", "doi:10.2000/gone"]]);
    // OpenAlex: the seeds' DOIs and PMID in a batch each; the moved work
    // by the PMID the registry gave (free); the candidates.
    assert_eq!(alex.lookups(), [6, 1, 1, 7]);
    assert_eq!(analysis.seeds[5].problem, Some(SeedProblem::NotFound));
    assert_eq!(analysis.seeds[6].problem, Some(SeedProblem::NotFound));

    // Neither the moved work (a seed now) nor the work carrying an
    // unresolved seed's DOI is probably missing.
    let missing: Vec<String> = analysis
        .probably_missing
        .unwrap()
        .items
        .iter()
        .map(|m| m.id.to_string())
        .collect();
    assert!(!missing.contains(&"openalex:W90".into()), "{missing:?}");
    assert!(!missing.contains(&"openalex:W91".into()), "{missing:?}");
    assert!(missing.contains(&"openalex:W11".into()));
}

/// What `estimate_only` counts is what the run then spends, with a cache
/// (seeds resolved by the estimate are found there) and without one.
#[tokio::test(start_paused = true)]
async fn estimates_agree_with_spend_on_a_mixed_set() {
    let kinds = [
        AnalysisKind::RetractedReferences,
        AnalysisKind::OddOneOut,
        AnalysisKind::ProbablyMissing { limit: 20 },
    ];
    let mut seeds = inputs(&MOVED_SEEDS);
    seeds.extend(inputs(&["not an id", "isbn:9780262033848", "pmid:405"]));
    for cache in [Some(Arc::new(SqliteCache::open_in_memory().unwrap())), None] {
        let cached = cache.is_some();
        let (engine, _, _) = moved_engine(cache);
        let estimate = engine
            .analyse(&seeds, &kinds, None, true)
            .await
            .unwrap()
            .estimate
            .unwrap();
        assert!(estimate.exact);
        let run = engine.analyse(&seeds, &kinds, None, false).await.unwrap();
        let spent: f64 = run.usage.iter().filter_map(|(_, u)| u.cost).sum();
        assert!(
            (spent - estimate.usd).abs() < 1e-12,
            "cached: {cached}, estimated {estimate:?}, spent {spent}"
        );
        // Without a cache, the seeds' batches are paid again by the run.
        let expected = if cached { 2 } else { 4 };
        assert_eq!(estimate.requests, expected, "cached: {cached}");
    }
}

// Related works.

/// Sets each work's `cited_by_count` to the works of `works` citing it,
/// unless it has one (a classic cited far more often than listed).
fn counted(mut works: Vec<SourceRecord>) -> Vec<SourceRecord> {
    let counts: Vec<u64> = works
        .iter()
        .map(|w| {
            works
                .iter()
                .filter(|c| c.references.iter().flatten().any(|r| r == &w.ids[0]))
                .count() as u64
        })
        .collect();
    for (w, n) in works.iter_mut().zip(counts) {
        w.work.cited_by_count.get_or_insert(n);
    }
    works
}

fn spent(a: &Analysis) -> f64 {
    a.usage.iter().filter_map(|(_, u)| u.cost).sum()
}

/// Three papers (W1–W3), the works citing them (W101–W106) and what those
/// cite, works citing the papers' shared references (W201, W202), and
/// works around them: W60 (and W67, its preprint), W61 (whose own
/// references overlap the set's), W64 (retracted), W65 (a version of W1)
/// and W66 (carrying the DOI of a seed OpenAlex can't find).
fn around() -> Vec<SourceRecord> {
    let mut works = vec![
        work(
            &["W1", "10.1000/one"],
            "Fluids in sepsis",
            "Ann Smith",
            2018,
            Some(&["W50", "W51", "W52"]),
        ),
        work(
            &["W2", "10.1000/two"],
            "Vasopressors in sepsis",
            "Bo Chen",
            2019,
            Some(&["W50", "W51", "W53"]),
        ),
        work(
            &["W3", "10.1000/three"],
            "Steroids in sepsis",
            "Cy Dee",
            2020,
            Some(&["W51", "W54"]),
        ),
        // Citing the set.
        work(
            &["W101"],
            "C1",
            "A",
            2021,
            Some(&["W1", "W2", "W60", "W61", "W50"]),
        ),
        work(
            &["W102"],
            "C2",
            "A",
            2021,
            Some(&["W1", "W60", "W62", "W50", "W51"]),
        ),
        work(&["W103"], "C3", "A", 2021, Some(&["W3", "W60", "W63"])),
        work(&["W104"], "C4", "A", 2021, Some(&["W2", "W61", "W64"])),
        work(&["W105"], "C5", "A", 2021, Some(&["W3", "W65", "W66"])),
        work(&["W106"], "C6", "A", 2021, Some(&["W1", "W67"])),
        // Citing the shared references, not the set.
        work(&["W201"], "K1", "A", 2017, Some(&["W50", "W51", "W68"])),
        work(&["W202"], "K2", "A", 2017, Some(&["W51"])),
        // Around the set.
        work(
            &["W60", "10.1000/sixty"],
            "Early sepsis care",
            "Di Rivers",
            2019,
            None,
        ),
        work(
            &["W67", "10.1101/sixty"],
            "Early sepsis care",
            "Di Rivers",
            2018,
            None,
        ),
        work(
            &["W61"],
            "Sepsis definitions",
            "Ed Singer",
            2016,
            Some(&["W50", "W51", "W99"]),
        ),
        retracted(
            work(&["W64"], "A retracted trial", "Fay Fake", 2015, None),
            "openalex",
        ),
        work(
            &["W65", "10.1101/one"],
            "Fluids in sepsis",
            "Ann Smith",
            2017,
            None,
        ),
        work(
            &["W66", "10.1000/gone"],
            "A seed by a DOI OpenAlex misses",
            "Gil Gone",
            2014,
            None,
        ),
    ];
    for (n, title) in [(50, "Fifty"), (51, "Fifty-one"), (62, "C"), (63, "D")] {
        works.push(work(&[&format!("W{n}")], title, "Z", 2010, None));
    }
    counted(works)
}

const AROUND: [&str; 4] = [
    "10.1000/one",
    "10.1000/two",
    "10.1000/three",
    "10.1000/gone",
];

fn around_engine(cache: Option<Arc<SqliteCache>>) -> (Engine, Arc<Alex>) {
    let alex = Arc::new(Alex::new(around()).blind(&["10.1000/gone"]));
    (engine(alex.clone(), cache), alex)
}

/// (id, score, co-cited with, shared references, seeds) of each work.
type Found = (String, f64, usize, usize, Vec<usize>);

fn found(a: &Analysis) -> Vec<Found> {
    a.related
        .as_ref()
        .unwrap()
        .items
        .iter()
        .map(|r| {
            (
                r.id.to_string().replace("openalex:", ""),
                r.score,
                r.co_cited_with,
                r.shared_references,
                r.seeds.clone(),
            )
        })
        .collect()
}

#[tokio::test(start_paused = true)]
async fn related_works_rank_by_co_citation_and_coupling() {
    let (engine, alex) = around_engine(None);
    let kinds = [AnalysisKind::Related { limit: 20 }];
    let analysis = engine
        .analyse(&inputs(&AROUND), &kinds, None, false)
        .await
        .unwrap();
    assert_eq!(analysis.seeds[3].problem, Some(SeedProblem::NotFound));
    let related = analysis.related.as_ref().unwrap();
    assert!(related.complete(), "{:?}", related.incomplete);
    assert!(analysis.complete());
    let s = |n: &[usize]| n.to_vec();
    assert_eq!(
        found(&analysis),
        [
            // Co-cited twice (by W101, W104), and its own references share
            // two of the set's: the highest of both counts' mean.
            ("W61".into(), 0.75, 2, 2, s(&[0, 1, 2])),
            // Co-cited by four, with its preprint (W67): the most.
            ("W60".into(), 0.5, 4, 0, s(&[0, 1, 2])),
            // Coupled: they cite two of the set's references.
            ("W102".into(), 0.5, 0, 2, s(&[0, 1, 2])),
            ("W201".into(), 0.5, 0, 2, s(&[0, 1, 2])),
            ("W50".into(), 0.25, 2, 0, s(&[0, 1])),
            ("W51".into(), 0.125, 1, 0, s(&[0])),
            ("W62".into(), 0.125, 1, 0, s(&[0])),
            ("W63".into(), 0.125, 1, 0, s(&[2])),
            // Left out: W64 (retracted), W65 (W1's preprint), W66 (the
            // missing seed's DOI); W202 shares one reference only.
        ]
    );
    let w60 = &related.items[1];
    let ids: Vec<String> = w60.ids.iter().map(ToString::to_string).collect();
    assert_eq!(ids, ["openalex:W60", "openalex:W67"]);
    assert!(w60.record.is_some());

    // The three works of the set share a page (cited 3, 2 and 2 times);
    // the works citing their shared references (W51 by all three, W50 by
    // two) come from one filter.
    assert_eq!(alex.citing_pages(), ["cites:W1|W2|W3", "cites:W51|W50"]);
    // Seeds in a batch, the candidates in another.
    assert_eq!(alex.lookups(), [4, 12]);
    assert!(
        (spent(&analysis) - 0.0004).abs() < 1e-12,
        "{}",
        spent(&analysis)
    );
}

#[tokio::test(start_paused = true)]
async fn related_works_are_the_same_every_time_and_reuse_the_cache() {
    let cache = Arc::new(SqliteCache::open_in_memory().unwrap());
    let (engine, alex) = around_engine(Some(cache));
    let kinds = [AnalysisKind::Related { limit: 3 }];
    // Not the missing seed: the cache would find it by the DOI W66 carries.
    let seeds = inputs(&AROUND[..3]);
    let first = engine.analyse(&seeds, &kinds, None, false).await.unwrap();
    // Ties by id: W60, W102 and W201 score 0.5.
    let ids: Vec<String> = found(&first).into_iter().map(|f| f.0).collect();
    assert_eq!(ids, ["W61", "W60", "W102"]);
    let pages = alex.citing_pages().len();

    let again = engine.analyse(&seeds, &kinds, None, false).await.unwrap();
    assert_eq!(found(&again), found(&first));
    // Citing works, coupling's page and the candidates all cached.
    assert_eq!(
        alex.citing_pages().len(),
        pages,
        "{:?}",
        alex.citing_pages()
    );
    assert_eq!(spent(&again), 0.0);
}

/// A classic (cited 50,000 times, as its record says) among ordinary
/// papers; 1,200 of its citing works are listed, each also citing W51 (a
/// reference the papers share) and one of seven works.
fn with_classic() -> Vec<SourceRecord> {
    let mut works = around();
    let mut classic = work(
        &["W9000", "10.1038/classic"],
        "Deep learning",
        "Yann LeCun",
        2015,
        Some(&["W51", "W52"]),
    );
    classic.work.cited_by_count = Some(50_000);
    works.push(classic);
    for k in 0..1_200u64 {
        let mut citing = work(
            &[&format!("W{}", 10_000 + k)],
            "Citing the classic",
            "A",
            2020,
            Some(&["W9000", "W51", &format!("W{}", 20_000 + k % 7)]),
        );
        citing.work.cited_by_count = Some(k % 50);
        works.push(citing);
    }
    works
}

const CLASSIC: [&str; 4] = [
    "10.1000/one",
    "10.1000/two",
    "10.1000/three",
    "10.1038/classic",
];

#[tokio::test(start_paused = true)]
async fn a_classic_is_sampled_and_says_how_much() {
    let alex = Arc::new(Alex::new(with_classic()));
    let engine = engine(alex.clone(), None);
    let kinds = [AnalysisKind::Related { limit: 5 }];
    let analysis = engine
        .analyse(&inputs(&CLASSIC), &kinds, None, false)
        .await
        .unwrap();
    let related = analysis.related.as_ref().unwrap();
    let stop = related.incomplete.unwrap();
    assert_eq!(stop.kind, IncompleteKind::Capped);
    // The three papers' 7 citing works, and the classic's 1,000 most cited
    // of its 50,000.
    assert_eq!((stop.done, stop.total), (1_007, 50_007));
    assert!(!analysis.complete());
    // One page for the papers, five for the classic; five of the works
    // citing the shared references (W51 by all four, W50 and W52 by two).
    let pages = alex.citing_pages();
    assert_eq!(pages.iter().filter(|p| *p == "cites:W1|W2|W3").count(), 1);
    assert_eq!(pages.iter().filter(|p| *p == "cites:W9000").count(), 5);
    assert_eq!(
        pages.iter().filter(|p| *p == "cites:W51|W50|W52").count(),
        5
    );
    // W51 is co-cited by each of the classic's citing works read, and by
    // W102.
    let w51 = found(&analysis).into_iter().find(|f| f.0 == "W51").unwrap();
    assert_eq!(w51.2, 1_001);
}

#[tokio::test(start_paused = true)]
async fn a_budget_shares_out_pages_and_says_how_far_they_got() {
    let alex = Arc::new(Alex::new(with_classic()));
    let engine = engine(alex.clone(), None);
    let kinds = [AnalysisKind::Related { limit: 5 }];
    // The seeds' batch, the lookup kept back, and two pages: each
    // filter's first.
    let analysis = engine
        .analyse(&inputs(&CLASSIC), &kinds, Some(0.0004), false)
        .await
        .unwrap();
    let related = analysis.related.as_ref().unwrap();
    let stop = related.incomplete.unwrap();
    assert_eq!(stop.kind, IncompleteKind::Budget);
    assert_eq!((stop.done, stop.total), (207, 50_007));
    let mut pages = alex.citing_pages();
    pages.sort();
    assert_eq!(pages, ["cites:W1|W2|W3", "cites:W9000"]);
    // Found all the same, and looked up (in the lookup kept back).
    assert!(!related.items.is_empty());
    assert_eq!(alex.lookups().len(), 2);
    assert!((spent(&analysis) - 0.0004).abs() < 1e-12);
}

#[tokio::test(start_paused = true)]
async fn failing_pages_say_so() {
    let cache = Arc::new(SqliteCache::open_in_memory().unwrap());
    let (engine, alex) = around_engine(Some(cache));
    let seeds = inputs(&AROUND[..3]);
    // The seeds resolve (and are cached) first; then OpenAlex fails.
    engine
        .analyse(&seeds, &[AnalysisKind::OddOneOut], None, false)
        .await
        .unwrap();
    alex.failures.lock().unwrap().extend(
        std::iter::repeat_with(|| Error::Server {
            status: 503,
            message: None,
        })
        .take(50),
    );
    let analysis = engine
        .analyse(&seeds, &[AnalysisKind::Related { limit: 5 }], None, false)
        .await
        .unwrap();
    let related = analysis.related.unwrap();
    let stop = related.incomplete.unwrap();
    assert_eq!(stop.kind, IncompleteKind::ProviderFailed);
    assert_eq!((stop.done, stop.total), (0, 7));
    assert!(related.items.is_empty());
}

/// What `estimate_only` counts is what the run spends: the citing works'
/// pages exactly (one for the papers, the classic's five), coupling's five
/// and the lookup as a ceiling, which a set this big fills.
#[tokio::test(start_paused = true)]
async fn related_estimates_agree_with_spend_on_a_mixed_set() {
    let kinds = [
        AnalysisKind::ProbablyMissing { limit: 20 },
        AnalysisKind::Related { limit: 20 },
    ];
    for cache in [Some(Arc::new(SqliteCache::open_in_memory().unwrap())), None] {
        let cached = cache.is_some();
        let alex = Arc::new(Alex::new(with_classic()));
        let engine = engine(alex.clone(), cache);
        let estimate = engine
            .analyse(&inputs(&CLASSIC), &kinds, None, true)
            .await
            .unwrap()
            .estimate
            .unwrap();
        assert!(!estimate.exact, "coupling and lookups are a ceiling");
        assert!(alex.citing_pages().is_empty());
        let run = engine
            .analyse(&inputs(&CLASSIC), &kinds, None, false)
            .await
            .unwrap();
        assert!(
            (spent(&run) - estimate.usd).abs() < 1e-12,
            "cached: {cached}, estimated {estimate:?}, spent {}",
            spent(&run)
        );
        // 1 + 5 citing pages, 5 coupling, a lookup each for probably
        // missing and related; and the seeds' batch without a cache.
        let expected = if cached { 13 } else { 14 };
        assert_eq!(estimate.requests, expected, "cached: {cached}");
    }
}
