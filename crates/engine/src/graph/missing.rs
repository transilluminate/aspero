//! Probably missing (works several seeds cite that the set lacks) and the
//! odd one out (seeds linked to no other).

use std::collections::{BTreeMap, BTreeSet};

use aspero_core::matching::same_work;
use aspero_core::{FieldSet, SourceRecord, WorkId};

use super::seeds::Group;
use super::spend::{Spend, TheSet, failed_lookups};
use super::{
    Incomplete, IncompleteKind, MAX_FILTER, MIN_CITING, Missing, OddOne, OddReason, Section, Seed,
};
use crate::{Engine, Entry};

impl Engine {
    /// The works cited by at least two works of the set and not in it,
    /// most cited first (ties by id), the best `limit` looked up and
    /// merged where they are one work (or a seed).
    pub(super) async fn probably_missing(
        &self,
        entry: &Entry,
        seeds: &[Seed],
        groups: &[Group],
        limit: u32,
        spend: &mut Spend,
    ) -> Section<Missing> {
        let in_set: BTreeSet<&WorkId> = groups.iter().flat_map(|g| &g.works).collect();
        let mut citing: BTreeMap<&WorkId, Vec<usize>> = BTreeMap::new();
        for (g, group) in groups.iter().enumerate() {
            for cited in group.references.iter().flatten() {
                if !in_set.contains(cited) {
                    citing.entry(cited).or_default().push(g);
                }
            }
        }
        let mut ranked: Vec<(&WorkId, Vec<usize>)> = citing
            .into_iter()
            .filter(|(_, by)| by.len() >= MIN_CITING)
            .collect();
        ranked.sort_by(|(a, x), (b, y)| y.len().cmp(&x.len()).then(a.cmp(b)));
        // Looked up in whole batches, so that works merging into seeds or
        // each other leave others to take their place at no extra cost.
        let limit = limit as usize;
        let fetch = ranked.len().min(limit.div_ceil(MAX_FILTER) * MAX_FILTER);
        let ids: Vec<WorkId> = ranked[..fetch]
            .iter()
            .map(|(id, _)| (*id).clone())
            .collect();

        let provider = entry.provider.id();
        let uncached = self
            .cached_lookups(&provider, &ids, FieldSet::DEFAULT)
            .iter()
            .filter(|c| c.is_none())
            .count();
        let paid = match uncached {
            0 | 1 => 0,
            n => n.div_ceil(MAX_FILTER),
        };
        let mut incomplete = None;
        let mut records: Vec<Option<SourceRecord>> = vec![None; fetch];
        if spend.estimating {
            spend.requests += paid as u64;
        } else if !ids.is_empty() {
            if spend.allows(paid) {
                let mut usage = Vec::new();
                let answers = self
                    .lookup_at(entry, &ids, FieldSet::DEFAULT, &mut usage)
                    .await;
                spend.add(usage, paid);
                let failed = failed_lookups(&answers, "probably missing works' lookup failed");
                if failed > 0 {
                    incomplete = Some(
                        Incomplete::new(IncompleteKind::ProviderFailed, fetch - failed, fetch)
                            .from(provider.clone()),
                    );
                }
                records = answers.into_iter().map(|(_, r)| r).collect();
            } else {
                incomplete = Some(Incomplete::new(IncompleteKind::Budget, 0, fetch));
            }
        }

        // Merge: works that are a seed leave (one work, or sharing any
        // identifier with any seed, resolved or not: its input, or any id
        // its record carries); works that are one work join.
        let set = TheSet::new(seeds);
        let mut found: Vec<(Missing, BTreeSet<usize>)> = Vec::new();
        for ((id, by), record) in ranked.into_iter().take(fetch).zip(records) {
            if set.has(id, record.as_ref()) {
                continue;
            }
            let same = record.as_ref().and_then(|r| {
                found
                    .iter_mut()
                    .find(|(m, _)| m.record.as_ref().is_some_and(|other| same_work(other, r)))
            });
            match same {
                Some((missing, by_groups)) => {
                    missing.ids.push(id.clone());
                    by_groups.extend(by);
                }
                None => found.push((
                    Missing {
                        id: id.clone(),
                        ids: vec![id.clone()],
                        record,
                        cited_by: Vec::new(),
                    },
                    by.into_iter().collect(),
                )),
            }
        }
        let mut items: Vec<Missing> = found
            .into_iter()
            .map(|(mut m, by)| {
                m.cited_by = by.into_iter().map(|g| groups[g].members[0]).collect();
                m
            })
            .collect();
        items.sort_by(|a, b| b.count().cmp(&a.count()).then(a.id.cmp(&b.id)));
        items.truncate(limit);
        Section::new(items, incomplete)
    }
}

/// Seeds linked to no other: coverage-aware, so unknown references are
/// `no_reference_data` unless another seed cites the work. Nothing is odd
/// in a set of fewer than two works.
pub(super) fn odd_one_out(groups: &[Group]) -> Section<OddOne> {
    let mut items = Vec::new();
    if groups.len() < 2 {
        return Section::new(items, None);
    }
    for (g, group) in groups.iter().enumerate() {
        let linked = groups.iter().enumerate().any(|(h, other)| {
            h != g && (group.cites(other) || other.cites(group) || group.shares_references(other))
        });
        if linked {
            continue;
        }
        let reason = match group.references {
            Some(_) => OddReason::NoLinks,
            None => OddReason::NoReferenceData,
        };
        items.extend(group.members.iter().map(|&seed| OddOne { seed, reason }));
    }
    items.sort_by_key(|o| o.seed);
    Section::new(items, None)
}

#[cfg(test)]
mod tests {
    use aspero_core::{FieldSet, Person, ProviderId, Raw, Work};
    use jiff::Timestamp;

    use super::super::{OPENALEX, merge_seeds};
    use super::*;

    fn record(ids: &[&str], title: &str, cites: Option<&[&str]>) -> SourceRecord {
        let mut work = Work::default();
        work.title = Some(title.into());
        work.authors = vec![Person::literal("Jane Doe")];
        work.issued = Some(aspero_core::PartialDate::from_year(2020));
        SourceRecord::new(
            ProviderId::from_static(OPENALEX),
            ids.iter().map(|i| i.parse().unwrap()).collect(),
            work,
            FieldSet::ALL,
            Raw::json(&serde_json::Value::Null),
            Timestamp::UNIX_EPOCH,
        )
        .with_references(cites.map(|c| c.iter().map(|i| i.parse().unwrap()).collect()))
    }

    fn seed(record: SourceRecord) -> Seed {
        Seed {
            input: record.ids[0].to_string(),
            record: Some(record),
            problem: None,
            message: None,
            same_as: None,
        }
    }

    #[test]
    fn odd_ones_are_told_apart_by_coverage() {
        let mut seeds = vec![
            seed(record(&["W1"], "One", Some(&["W2", "W100"]))),
            seed(record(&["W2"], "Two", Some(&["W101"]))),
            // Shares W100 with W1.
            seed(record(&["W3"], "Three", Some(&["W100"]))),
            // Cites nothing of the set.
            seed(record(&["W4"], "Four", Some(&["W200"]))),
            // Unknown references, cited by none.
            seed(record(&["W5"], "Five", None)),
            // Unknown references, but W6 cites it: not odd.
            seed(record(&["W7"], "Seven", None)),
            seed(record(&["W6"], "Six", Some(&["W7"]))),
        ];
        let groups = merge_seeds(&mut seeds);
        let odd = odd_one_out(&groups);
        assert_eq!(
            odd.items,
            [
                OddOne {
                    seed: 3,
                    reason: OddReason::NoLinks
                },
                OddOne {
                    seed: 4,
                    reason: OddReason::NoReferenceData
                },
            ]
        );
    }
}
