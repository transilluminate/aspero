//! Citation links: references pages and analyses of a set, against a
//! scripted OpenAlex that charges as the real one does.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use aspero_core::{
    Capabilities, Error, FieldSet, Hit, IdKind, Lookup, Page, PartialDate, Person, Provider,
    ProviderId, Raw, ResiliencePolicy, Response, Result, SearchQuery, SourceRecord, Update,
    UpdateKind, Usage, Work, WorkId,
};
use aspero_engine::{
    AnalysisKind, Cache, Engine, IncompleteKind, OddOne, OddReason, SeedProblem, SqliteCache,
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
        }
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
                record: self.works.iter().find(|w| w.ids.contains(id)).cloned(),
            })
            .collect();
        let cost = if ids.len() > 1 { 0.0001 } else { 0.0 };
        Ok(Response::with_usage(lookups, usage(cost)))
    }

    async fn search(&self, query: &SearchQuery) -> Result<Response<Page>> {
        let filter = query.extra.get("filter").cloned().unwrap_or_default();
        self.searches.lock().unwrap().push(filter.clone());
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
