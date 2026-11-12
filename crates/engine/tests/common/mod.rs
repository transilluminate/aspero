//! Scripted providers and fixture works shared by the engine's tests.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use aspero_core::{
    Capabilities, FieldSet, Hit, IdKind, Lookup, Page, PartialDate, Person, Provider, ProviderId,
    Raw, ResiliencePolicy, Response, Result, SearchMode, SearchQuery, SourceRecord, Work, WorkId,
};
use aspero_engine::Engine;
use async_trait::async_trait;
use jiff::Timestamp;

/// A provider with a fixed set of works. Lookups find works by ID; citation
/// matching returns `matcher_hits` whatever the query; keyword search returns
/// works whose title equals the query (case-insensitively).
pub struct Scripted {
    pub id: ProviderId,
    pub caps: Capabilities,
    pub works: Vec<SourceRecord>,
    pub matcher_hits: Vec<SourceRecord>,
    pub queries: Mutex<Vec<SearchQuery>>,
    /// How long each search takes.
    pub search_delay: std::time::Duration,
    /// What each lookup costs.
    pub lookup_cost: Option<f64>,
}

impl Scripted {
    pub fn new(id: &'static str) -> Self {
        let mut caps = Capabilities::default();
        caps.lookup_kinds = vec![IdKind::Doi, IdKind::Arxiv];
        caps.max_batch = 10;
        caps.fields = FieldSet::ALL;
        Scripted {
            id: ProviderId::from_static(id),
            caps,
            works: Vec::new(),
            matcher_hits: Vec::new(),
            queries: Mutex::default(),
            search_delay: std::time::Duration::ZERO,
            lookup_cost: None,
        }
    }

    /// Each lookup costs `cost`.
    pub fn costing(mut self, cost: f64) -> Self {
        self.lookup_cost = Some(cost);
        self
    }

    /// Searches take `delay` (as Crossref's matcher can).
    pub fn slow(mut self, delay: std::time::Duration) -> Self {
        self.search_delay = delay;
        self
    }

    pub fn matcher(mut self, hits: Vec<SourceRecord>) -> Self {
        self.caps.search = true;
        self.caps.citation_search = true;
        self.matcher_hits = hits;
        self
    }

    pub fn keyword(mut self) -> Self {
        self.caps.search = true;
        self
    }

    /// Searches by volume and first page too (`SearchQuery::place`).
    pub fn placer(mut self) -> Self {
        self.caps.search = true;
        self.caps
            .search_filters
            .extend([aspero_core::Filter::Years, aspero_core::Filter::Place]);
        self
    }

    pub fn sent(&self) -> Vec<SearchQuery> {
        self.queries.lock().unwrap().clone()
    }

    pub fn with(mut self, works: Vec<SourceRecord>) -> Self {
        self.works = works
            .into_iter()
            .map(|mut w| {
                w.source = self.id.clone();
                w
            })
            .collect();
        self
    }

    pub fn queries(&self) -> Vec<(SearchMode, String)> {
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
        let mut response = Response::new(lookups);
        response.usage = self.lookup_cost.map(|cost| {
            let mut usage = aspero_core::Usage::default();
            usage.cost = Some(cost);
            usage
        });
        Ok(response)
    }

    async fn search(&self, query: &SearchQuery) -> Result<Response<Page>> {
        self.queries.lock().unwrap().push(query.clone());
        tokio::time::sleep(self.search_delay).await;
        let first_page = |w: &SourceRecord| {
            w.work
                .page
                .as_deref()
                .map(|p| aspero_core::publication::first_page(p).to_owned())
        };
        let records: Vec<SourceRecord> = match (query.mode, &query.place) {
            (SearchMode::Citation, _) => self.matcher_hits.clone(),
            (_, Some(place)) => self
                .works
                .iter()
                .filter(|w| {
                    w.work.volume.as_deref() == Some(place.volume.as_str())
                        && first_page(w).as_deref() == Some(place.first_page.as_str())
                })
                .cloned()
                .collect(),
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

pub fn work(
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

pub const VASWANI: [&str; 3] = ["Ashish Vaswani", "Noam Shazeer", "Niki Parmar"];
pub const LECUN: [&str; 3] = ["Yann LeCun", "Yoshua Bengio", "Geoffrey Hinton"];

pub fn attention_real() -> SourceRecord {
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

pub fn attention_lookalike() -> SourceRecord {
    work(
        "crossref",
        &["10.65215/r5bs2d54"],
        "Attention Is All You Need",
        &VASWANI,
        2025,
    )
}

pub fn deep_learning() -> SourceRecord {
    work(
        "crossref",
        &["10.1038/nature14539"],
        "Deep learning",
        &LECUN,
        2015,
    )
}

pub fn deep_learning_for_ai() -> SourceRecord {
    work(
        "crossref",
        &["10.1145/3448250"],
        "Deep learning for AI",
        &LECUN,
        2021,
    )
}

pub fn engine(providers: Vec<Arc<Scripted>>) -> Engine {
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
pub fn live_like() -> (Engine, Arc<Scripted>, Arc<Scripted>) {
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

pub fn with_abstract(mut record: SourceRecord, text: &str) -> SourceRecord {
    record.work.abstract_text = Some(text.into());
    record
}
