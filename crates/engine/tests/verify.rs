//! `resolve` and `verify` against scripted providers that replay the
//! situations seen live (DESIGN §9): lookalike re-deposits, citation
//! matchers that lack DataCite DOIs, invented and misattached DOIs.

use std::sync::{Arc, Mutex};

use aspero_core::{
    Agreement, Capabilities, Claim, Error, FieldSet, Hit, IdKind, Lookup, Page, PartialDate,
    Person, Provider, ProviderId, Raw, ResiliencePolicy, Response, Result, SearchMode, SearchQuery,
    SourceRecord, Work, WorkId,
};
use aspero_engine::{Engine, Verdict, Via};
use async_trait::async_trait;
use jiff::Timestamp;

/// A provider with a fixed set of works. Lookups find works by ID; citation
/// matching returns `matcher_hits` whatever the query; keyword search returns
/// works whose title equals the query (case-insensitively).
struct Scripted {
    id: ProviderId,
    caps: Capabilities,
    works: Vec<SourceRecord>,
    matcher_hits: Vec<SourceRecord>,
    queries: Mutex<Vec<SearchQuery>>,
}

impl Scripted {
    fn new(id: &'static str) -> Self {
        let mut caps = Capabilities::default();
        caps.lookup_kinds = vec![IdKind::Doi, IdKind::Arxiv];
        caps.max_batch = 10;
        Scripted {
            id: ProviderId::from_static(id),
            caps,
            works: Vec::new(),
            matcher_hits: Vec::new(),
            queries: Mutex::default(),
        }
    }

    fn matcher(mut self, hits: Vec<SourceRecord>) -> Self {
        self.caps.search = true;
        self.caps.citation_search = true;
        self.matcher_hits = hits;
        self
    }

    fn keyword(mut self) -> Self {
        self.caps.search = true;
        self
    }

    fn with(mut self, works: Vec<SourceRecord>) -> Self {
        self.works = works
            .into_iter()
            .map(|mut w| {
                w.source = self.id.clone();
                w
            })
            .collect();
        self
    }

    fn queries(&self) -> Vec<(SearchMode, String)> {
        let q = self.queries.lock().unwrap();
        q.iter().map(|q| (q.mode, q.text.clone())).collect()
    }
}

#[async_trait]
impl Provider for Scripted {
    fn id(&self) -> ProviderId {
        self.id.clone()
    }

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    async fn lookup(&self, ids: &[WorkId], _: FieldSet) -> Result<Response<Vec<Lookup>>> {
        let lookups = ids
            .iter()
            .map(|id| Lookup {
                id: id.clone(),
                record: self.works.iter().find(|w| w.ids.contains(id)).cloned(),
            })
            .collect();
        Ok(Response::new(lookups))
    }

    async fn search(&self, query: &SearchQuery) -> Result<Response<Page>> {
        self.queries.lock().unwrap().push(query.clone());
        let records: Vec<SourceRecord> = match query.mode {
            SearchMode::Citation => self.matcher_hits.clone(),
            _ => self
                .works
                .iter()
                .filter(|w| {
                    w.work.title.as_deref().map(str::to_lowercase)
                        == Some(query.text.to_lowercase())
                })
                .cloned()
                .collect(),
        };
        let hits = records
            .into_iter()
            .enumerate()
            .map(|(i, r)| Hit::new(r, Some(100.0 - i as f64)))
            .collect();
        Ok(Response::new(Page::new(hits, None, None)))
    }
}

fn work(
    source: &'static str,
    ids: &[&str],
    title: &str,
    authors: &[&str],
    year: i16,
) -> SourceRecord {
    let mut w = Work::default();
    w.title = Some(title.into());
    w.authors = authors.iter().map(|a| Person::literal(*a)).collect();
    w.issued = Some(PartialDate::from_year(year));
    SourceRecord::new(
        ProviderId::from_static(source),
        ids.iter().map(|i| i.parse().unwrap()).collect(),
        w,
        FieldSet::ALL,
        Raw {
            media_type: "application/json".into(),
            body: "{}".into(),
        },
        Timestamp::UNIX_EPOCH,
    )
}

const VASWANI: [&str; 3] = ["Ashish Vaswani", "Noam Shazeer", "Niki Parmar"];
const LECUN: [&str; 3] = ["Yann LeCun", "Yoshua Bengio", "Geoffrey Hinton"];

fn attention_real() -> SourceRecord {
    work(
        "openalex",
        &[
            "W2626778328",
            "arXiv:1706.03762",
            "10.48550/arXiv.1706.03762",
        ],
        "Attention Is All You Need",
        &VASWANI,
        2017,
    )
}

fn attention_lookalike() -> SourceRecord {
    work(
        "crossref",
        &["10.65215/r5bs2d54"],
        "Attention Is All You Need",
        &VASWANI,
        2025,
    )
}

fn deep_learning() -> SourceRecord {
    work(
        "crossref",
        &["10.1038/nature14539"],
        "Deep learning",
        &LECUN,
        2015,
    )
}

fn deep_learning_for_ai() -> SourceRecord {
    work(
        "crossref",
        &["10.1145/3448250"],
        "Deep learning for AI",
        &LECUN,
        2021,
    )
}

fn engine(providers: Vec<Arc<Scripted>>) -> Engine {
    providers
        .into_iter()
        .enumerate()
        .fold(Engine::builder(), |b, (i, p)| {
            b.provider(p, ResiliencePolicy::default(), i as i32)
        })
        .build()
}

/// The live setup: OpenAlex (keyword, indexes arXiv) then Crossref (matcher,
/// returns the lookalike).
fn live_like() -> (Engine, Arc<Scripted>, Arc<Scripted>) {
    let openalex = Arc::new(
        Scripted::new("openalex")
            .keyword()
            .with(vec![attention_real(), deep_learning()]),
    );
    let crossref = Arc::new(
        Scripted::new("crossref")
            .matcher(vec![attention_lookalike()])
            .with(vec![
                attention_lookalike(),
                deep_learning(),
                deep_learning_for_ai(),
            ]),
    );
    (
        engine(vec![openalex.clone(), crossref.clone()]),
        openalex,
        crossref,
    )
}

#[tokio::test]
async fn a_lookalike_from_the_matcher_leads_to_the_real_paper() {
    let (engine, openalex, _) = live_like();
    let v = engine
        .verify(
            &Claim::from_text("Vaswani 2017 Attention is all you need"),
            5,
        )
        .await;

    assert_eq!(v.verdict, Verdict::Verified, "{}", v.reason);
    let best = &v.report.candidates[v.best.unwrap()];
    assert!(best.ids.contains(&"arXiv:1706.03762".parse().unwrap()));
    // The matcher's (lookalike) title became the OpenAlex query.
    assert_eq!(
        openalex.queries(),
        [(SearchMode::Keywords, "Attention Is All You Need".to_owned())]
    );
    let lookalike = v
        .report
        .candidates
        .iter()
        .find(|c| c.record.work.issued.unwrap().year() == 2025)
        .unwrap();
    assert_eq!(lookalike.assessment.year.agreement, Agreement::Mismatch);
}

#[tokio::test]
async fn verify_skips_paid_searches_once_a_free_source_verifies() {
    let openalex = Arc::new(
        Scripted::new("openalex")
            .keyword()
            .with(vec![deep_learning()]),
    );
    let crossref = Arc::new(Scripted::new("crossref").matcher(vec![deep_learning()]));
    let engine = engine(vec![openalex.clone(), crossref]);
    let claim =
        Claim::from_text("LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015;521:436-444.");

    let v = engine.verify(&claim, 5).await;
    assert_eq!(v.verdict, Verdict::Verified, "{}", v.reason);
    assert!(openalex.queries().is_empty(), "no OpenAlex search needed");

    // resolve, by contrast, gathers everything.
    engine.resolve(&claim, 5).await;
    assert_eq!(openalex.queries().len(), 1);
}

#[tokio::test]
async fn an_invented_doi_is_a_mismatch_pointing_at_the_real_work() {
    let (engine, _, _) = live_like();
    let claim = Claim::from_text(
        "LeCun, Bengio, Hinton. Deep learning. Nature 2015. doi:10.1038/nature99999",
    );
    let crossref_matches_real = Arc::new(Scripted::new("crossref").matcher(vec![deep_learning()]));
    let engine2 = self::engine(vec![crossref_matches_real]);

    let v = engine2.verify(&claim, 5).await;
    assert_eq!(v.verdict, Verdict::Mismatch, "{}", v.reason);
    assert!(v.reason.contains("does not exist"), "{}", v.reason);
    assert!(v.reason.contains("10.1038/nature14539"), "{}", v.reason);

    // With no matching work anywhere, it's simply not found.
    let nothing =
        Claim::from_text("Smith J. A paper that was never written. 2019. doi:10.9999/nope");
    let v = engine.verify(&nothing, 5).await;
    assert_eq!(v.verdict, Verdict::NotFound, "{}", v.reason);
}

#[tokio::test]
async fn a_real_doi_attached_to_the_wrong_citation_is_a_mismatch() {
    let crossref = Arc::new(
        Scripted::new("crossref")
            .matcher(vec![deep_learning()])
            .with(vec![deep_learning(), deep_learning_for_ai()]),
    );
    let engine = engine(vec![crossref]);
    // The DOI is real but belongs to the 2021 CACM article.
    let claim = Claim::from_text(
        "LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015. doi:10.1145/3448250",
    );

    let v = engine.verify(&claim, 5).await;
    assert_eq!(v.verdict, Verdict::Mismatch, "{}", v.reason);
    assert!(v.reason.contains("Deep learning for AI"), "{}", v.reason);
    let best = &v.report.candidates[v.best.unwrap()];
    assert_eq!(
        best.record.ids,
        ["10.1038/nature14539".parse::<WorkId>().unwrap()]
    );
}

#[tokio::test]
async fn identifier_only_claims_check_existence_without_searching() {
    let (engine, openalex, crossref) = live_like();
    let mut claim = Claim::default();
    claim.ids = vec!["10.1038/nature14539".parse().unwrap()];

    let v = engine.verify(&claim, 5).await;
    assert_eq!(v.verdict, Verdict::Verified, "{}", v.reason);
    assert!(v.reason.contains("exists"));
    assert!(openalex.queries().is_empty() && crossref.queries().is_empty());
}

#[tokio::test]
async fn records_sharing_an_identifier_merge_into_one_candidate() {
    let openalex = Arc::new(
        Scripted::new("openalex")
            .keyword()
            .with(vec![deep_learning()]),
    );
    let crossref = Arc::new(Scripted::new("crossref").matcher(vec![deep_learning()]));
    let engine = engine(vec![openalex, crossref]);

    let report = engine
        .resolve(&Claim::from_text("Deep learning, LeCun 2015"), 5)
        .await;

    assert_eq!(report.candidates.len(), 1);
    let vias: Vec<Via> = report.candidates[0]
        .sightings
        .iter()
        .map(|s| s.via)
        .collect();
    assert_eq!(vias, [Via::CitationMatch, Via::TitleSearch]);
}

#[tokio::test]
async fn provider_failures_make_an_empty_result_unverifiable() {
    struct Down(Capabilities);
    #[async_trait]
    impl Provider for Down {
        fn id(&self) -> ProviderId {
            ProviderId::from_static("down")
        }
        fn capabilities(&self) -> &Capabilities {
            &self.0
        }
        async fn search(&self, _: &SearchQuery) -> Result<Response<Page>> {
            Err(Error::AuthRejected { message: None })
        }
    }
    let mut caps = Capabilities::default();
    caps.search = true;
    caps.citation_search = true;
    let engine = Engine::builder()
        .provider(Arc::new(Down(caps)), ResiliencePolicy::default(), 0)
        .build();

    let v = engine
        .verify(&Claim::from_text("Anything at all, 2020"), 5)
        .await;
    assert_eq!(v.verdict, Verdict::Unverifiable, "{}", v.reason);
    assert_eq!(v.report.errors.len(), 1);
}
