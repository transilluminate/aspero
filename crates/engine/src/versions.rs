//! Following newer versions to the latest: Cochrane reviews are superseded
//! `.pub6` → `.pub7` → …, each version's record naming only the next.

use aspero_core::{FieldSet, ProviderId, SourceRecord, Usage, WorkId};

use crate::Engine;

/// Most versions followed per work; a loop guard, never reached in
/// practice.
const MAX_HOPS: usize = 20;

#[derive(Debug, Clone, Default)]
pub struct VersionsReport {
    /// Per work, in order: the latest version found, if the work has a
    /// newer one. When the chain breaks (a version can't be looked up), the
    /// last one found.
    pub latest: Vec<Option<SourceRecord>>,
    pub usage: Vec<(ProviderId, Usage)>,
}

impl Engine {
    /// The latest version of each work its record says is superseded
    /// ([`Work::newer_version`](aspero_core::Work::newer_version)), asking
    /// the providers that report notices. One batched lookup per step along
    /// the chains.
    pub async fn latest_versions(&self, works: &[&SourceRecord]) -> VersionsReport {
        let mut latest: Vec<Option<SourceRecord>> = vec![None; works.len()];
        let mut usage = Vec::new();
        let Some(entry) = self
            .entries
            .iter()
            .find(|e| e.provider.capabilities().notices)
        else {
            return VersionsReport { latest, usage };
        };
        // (work index, next version, versions seen) per chain still going.
        let mut frontier: Vec<(usize, WorkId, Vec<WorkId>)> = works
            .iter()
            .enumerate()
            .filter_map(|(i, r)| Some((i, r.work.newer_version()?.clone(), r.ids.clone())))
            .collect();
        for _ in 0..MAX_HOPS {
            if frontier.is_empty() {
                break;
            }
            let ids: Vec<WorkId> = frontier.iter().map(|(_, id, _)| id.clone()).collect();
            let answers = self.lookup_at(entry, &ids, FieldSet::ALL, &mut usage).await;
            let mut next = Vec::new();
            for ((i, id, mut seen), (_, record)) in frontier.into_iter().zip(answers) {
                let Some(record) = record else { continue };
                seen.push(id);
                if let Some(newer) = record.work.newer_version()
                    && !seen.contains(newer)
                {
                    next.push((i, newer.clone(), seen));
                }
                latest[i] = Some(record);
            }
            frontier = next;
        }
        VersionsReport { latest, usage }
    }
}
