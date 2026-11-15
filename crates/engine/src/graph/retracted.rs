//! Retracted references: which seed cites which retracted work.

use std::collections::{BTreeMap, BTreeSet};

use aspero_core::{ErrorKind, IdKind, ProviderId, SearchQuery, SourceRecord, Update, WorkId};

use super::{
    Incomplete, IncompleteKind, MAX_FILTER, OPENALEX, RetractedReference, Section, Seed, Spend,
    any_of, openalex_id,
};
use crate::{Engine, Served};

impl Engine {
    /// Which seed cites which retracted work: the seeds' references checked
    /// at OpenAlex 100 at a time, and against notices already cached.
    pub(super) async fn retracted_references(
        &self,
        seeds: &[Seed],
        spend: &mut Spend,
    ) -> Section<RetractedReference> {
        let cited: BTreeSet<&WorkId> = seeds
            .iter()
            .filter_map(Seed::references)
            .flatten()
            .collect();
        let cited: Vec<&WorkId> = cited.into_iter().collect();
        let provider = ProviderId::from_static(OPENALEX);
        let mut retracted: BTreeMap<WorkId, (SourceRecord, String)> = BTreeMap::new();
        let (mut done, mut failed, mut budget) = (0, false, false);
        for chunk in cited.chunks(MAX_FILTER) {
            let query = retracted_query(chunk);
            let paid = match self.cached_search(&provider, &query) {
                Some((_, true)) => 0,
                _ => 1,
            };
            if spend.estimating {
                spend.requests += paid;
                continue;
            }
            if !spend.allows(paid as usize) {
                budget = true;
                break;
            }
            match self.search(Some(OPENALEX), &query).await {
                Ok(report) => {
                    let usage = report.usage.map(|u| (provider.clone(), u));
                    let live = usize::from(report.served == Served::Live);
                    spend.add(usage.into_iter().collect(), live);
                    for hit in report.page.hits {
                        if let Some(id) = openalex_id(&hit.record).cloned() {
                            retracted.insert(id, (hit.record, OPENALEX.into()));
                        }
                    }
                    done += chunk.len();
                }
                Err(e) if e.kind() == ErrorKind::BudgetCapped => {
                    tracing::info!(target: "aspero::graph", error = %e, "retraction check stopped at the daily cap");
                    budget = true;
                    break;
                }
                Err(e) => {
                    tracing::warn!(target: "aspero::graph", error = %e, "retraction check failed");
                    failed = true;
                }
            }
        }
        // A failure first, as related works say it; the budget too, logged.
        let incomplete = match (failed, budget) {
            (true, also) => {
                if also {
                    tracing::warn!(target: "aspero::graph", done, total = cited.len(), "retraction check stopped at the budget too");
                }
                Some(
                    Incomplete::new(IncompleteKind::ProviderFailed, done, cited.len())
                        .from(provider.clone()),
                )
            }
            (false, true) => Some(Incomplete::new(IncompleteKind::Budget, done, cited.len())),
            (false, false) => None,
        };
        if spend.estimating {
            return Section::new(Vec::new(), None);
        }

        // Notices aspero already has: cached records' own flags, and the
        // registry's (Crossref) notices for their DOIs. No calls.
        let unflagged: Vec<WorkId> = cited
            .iter()
            .filter(|id| !retracted.contains_key(id))
            .map(|id| (*id).clone())
            .collect();
        let records: Vec<(&WorkId, SourceRecord)> = unflagged
            .iter()
            .zip(self.stale_lookups(&provider, &unflagged))
            .filter_map(|(id, record)| Some((id, record?)))
            .collect();
        // The registry's notices for the DOIs of those OpenAlex doesn't
        // flag, in one read of the cache.
        let registry = self.registry().map(|e| e.provider.id());
        let dois: Vec<WorkId> = records
            .iter()
            .filter(|(_, r)| r.work.retraction().is_none())
            .flat_map(|(_, r)| r.ids.iter().filter(|i| i.kind() == IdKind::Doi).cloned())
            .collect::<BTreeSet<WorkId>>()
            .into_iter()
            .collect();
        let notices: BTreeMap<WorkId, String> = match &registry {
            Some(registry) if !dois.is_empty() => dois
                .iter()
                .zip(self.stale_lookups(registry, &dois))
                .filter_map(|(doi, r)| {
                    let source = notice_source(r?.work.retraction()?, registry.as_str());
                    Some((doi.clone(), source))
                })
                .collect(),
            _ => BTreeMap::new(),
        };
        for (id, record) in records {
            let notice = match record.work.retraction() {
                Some(u) => Some(notice_source(u, OPENALEX)),
                None => record.ids.iter().find_map(|i| notices.get(i).cloned()),
            };
            if let Some(source) = notice {
                retracted.insert(id.clone(), (record, source));
            }
        }

        let mut items = Vec::new();
        for (i, seed) in seeds.iter().enumerate() {
            let mut seen = BTreeSet::new();
            for id in seed.references().into_iter().flatten() {
                if let Some((record, source)) = retracted.get(id)
                    && seen.insert(id)
                {
                    items.push(RetractedReference {
                        seed: i,
                        cites: record.clone(),
                        source: source.clone(),
                    });
                }
            }
        }
        Section::new(items, incomplete)
    }
}

/// Who reported a retraction: the notice's source, or `fallback`.
fn notice_source(update: &Update, fallback: &str) -> String {
    update.source.clone().unwrap_or_else(|| fallback.to_owned())
}

/// The search finding which of `ids` OpenAlex flags as retracted.
fn retracted_query(ids: &[&WorkId]) -> SearchQuery {
    let mut query = SearchQuery::new("");
    query.page.limit = MAX_FILTER as u32;
    query.extra.insert(
        "filter".into(),
        format!("openalex:{},is_retracted:true", any_of(ids.iter().copied())),
    );
    query
}
