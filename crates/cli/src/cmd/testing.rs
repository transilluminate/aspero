//! A scripted provider for pinning commands' output without a network.

use std::sync::Arc;

use aspero_core::{
    Capabilities, FieldSet, Hit, IdKind, Lookup, Page, PartialDate, Person, Provider, ProviderId,
    Raw, ResiliencePolicy, Response, Result, SearchQuery, SourceRecord, Usage, Work, WorkId,
};
use aspero_engine::Engine;
use async_trait::async_trait;
use jiff::Timestamp;

/// Lookups by identifier and searches by exact title over a few works,
/// each call at a cost. Costs are binary fractions, so totals don't
/// depend on the order calls finish in.
pub struct Scripted {
    id: ProviderId,
    caps: Capabilities,
    works: Vec<SourceRecord>,
}

impl Scripted {
    pub fn new(id: &'static str, works: Vec<SourceRecord>) -> Self {
        let mut caps = Capabilities::default();
        caps.lookup_kinds = vec![IdKind::Doi, IdKind::Arxiv, IdKind::OpenAlex];
        caps.max_batch = 10;
        caps.fields = FieldSet::ALL;
        caps.search = true;
        let id = ProviderId::from_static(id);
        let works = works
            .into_iter()
            .map(|mut w| {
                w.source = id.clone();
                w
            })
            .collect();
        Scripted { id, caps, works }
    }

    /// The registry that DOIs' records are checked against, as Crossref.
    pub fn registry(mut self) -> Self {
        self.caps.notices = true;
        self
    }
}

fn costing<T>(value: T, cost: f64) -> Response<T> {
    let mut usage = Usage::default();
    usage.cost = Some(cost);
    let mut response = Response::new(value);
    response.usage = Some(usage);
    response
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
        Ok(costing(lookups, 0.0625))
    }

    async fn search(&self, query: &SearchQuery) -> Result<Response<Page>> {
        // OpenAlex's retraction check: `openalex:W1|W2,is_retracted:true`.
        if let Some(filter) = query.extra.get("filter") {
            let ids: Vec<WorkId> = filter
                .trim_start_matches("openalex:")
                .trim_end_matches(",is_retracted:true")
                .split('|')
                .filter_map(|w| w.parse().ok())
                .collect();
            let hits = self
                .works
                .iter()
                .filter(|w| w.ids.iter().any(|i| ids.contains(i)) && w.work.retraction().is_some())
                .map(|w| Hit::new(w.clone(), None))
                .collect();
            return Ok(costing(Page::new(hits, None, None), 0.5));
        }
        let title = |w: &SourceRecord| w.work.title.as_deref().map(str::to_lowercase);
        let hits = self
            .works
            .iter()
            .filter(|w| title(w) == Some(query.text.to_lowercase()))
            .map(|w| Hit::new(w.clone(), Some(100.0)))
            .collect();
        Ok(costing(Page::new(hits, None, None), 0.5))
    }
}

pub fn work(ids: &[&str], title: &str, authors: &[&str], year: i16) -> SourceRecord {
    let mut w = Work::default();
    w.title = Some(title.into());
    w.authors = authors.iter().map(|a| Person::literal(*a)).collect();
    w.issued = Some(PartialDate::from_year(year));
    SourceRecord::new(
        ProviderId::from_static("openalex"),
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

/// An engine over `providers`, in priority order.
pub fn engine(providers: Vec<Scripted>) -> Engine {
    providers
        .into_iter()
        .enumerate()
        .fold(Engine::builder(), |b, (i, p)| {
            b.provider(Arc::new(p), ResiliencePolicy::default(), i as i32)
        })
        .build()
}

/// Compares `actual` with `tests/<name>`; `ASPERO_BLESS=1` rewrites the
/// file instead, after an intended change.
pub fn pinned(name: &str, actual: &str) {
    let file = format!("{}/tests/{name}", env!("CARGO_MANIFEST_DIR"));
    if std::env::var_os("ASPERO_BLESS").is_some() {
        std::fs::write(&file, actual).unwrap();
    }
    let expected = std::fs::read_to_string(&file).unwrap();
    assert_eq!(actual, expected, "{name} changed");
}
