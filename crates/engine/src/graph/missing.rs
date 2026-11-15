//! Probably missing (works several seeds cite that the set lacks) and the
//! odd one out (seeds linked to no other).

use std::collections::{BTreeMap, BTreeSet};

use aspero_core::WorkId;

use super::seeds::Group;
use super::spend::{Spend, TheSet, merge_versions};
use super::{MAX_FILTER, MIN_CITING, Missing, OddOne, OddReason, Section, Seed};
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

        let looked = self
            .look_up(entry, &ids, spend, "probably missing works' lookup failed")
            .await;

        // Merge: works that are a seed leave (one work, or sharing any
        // identifier with any seed, resolved or not: its input, or any id
        // its record carries); works that are one work join.
        let set = TheSet::new(seeds);
        let kept = ranked
            .into_iter()
            .take(fetch)
            .zip(looked.records)
            .filter(|((id, _), record)| !set.has(id, record.as_ref()))
            .map(|((id, by), record)| (id.clone(), record, by.into_iter().collect()))
            .collect();
        let found = merge_versions(kept, |a: &mut BTreeSet<usize>, b| a.extend(b));
        let mut items: Vec<Missing> = found
            .into_iter()
            .map(|m| Missing {
                id: m.id,
                ids: m.ids,
                record: m.record,
                cited_by: m.value.into_iter().map(|g| groups[g].members[0]).collect(),
            })
            .collect();
        items.sort_by(|a, b| b.count().cmp(&a.count()).then(a.id.cmp(&b.id)));
        items.truncate(limit);
        Section::new(items, looked.stop)
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
    use aspero_core::{FieldSet, Person, ProviderId, Raw, SourceRecord, Work};
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

    fn issued(mut record: SourceRecord, year: i16) -> SourceRecord {
        record.work.issued = Some(aspero_core::PartialDate::from_year(year));
        record
    }

    /// A (a DOI, 2021) and B (an arXiv ID, 2019) aren't one work, but C
    /// carries both their identifiers: the three are one.
    fn bridged() -> [SourceRecord; 3] {
        [
            issued(record(&["W1", "10.1000/d1"], "Same", Some(&["W9"])), 2021),
            issued(record(&["W2", "arXiv:2001.00001"], "Same", None), 2019),
            record(&["W3", "10.1000/d1", "arXiv:2001.00001"], "Same", None),
        ]
    }

    #[test]
    fn seeds_are_one_work_through_another() {
        let mut seeds = bridged().map(seed).to_vec();
        let groups = merge_seeds(&mut seeds);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].members, [0, 1, 2]);
        let same: Vec<Option<usize>> = seeds.iter().map(|s| s.same_as).collect();
        assert_eq!(same, [None, Some(0), Some(0)]);
    }

    #[test]
    fn seeds_without_openalex_ids_are_not_one_work_for_that() {
        let mut seeds = vec![
            seed(record(&["10.1000/x"], "Alpha", None)),
            seed(record(&["10.1000/y"], "Beta", None)),
        ];
        assert_eq!(merge_seeds(&mut seeds).len(), 2);
        assert!(seeds.iter().all(|s| s.same_as.is_none()));
    }

    #[test]
    fn versions_are_one_work_through_another() {
        let found = bridged()
            .into_iter()
            .map(|r| (r.ids[0].clone(), Some(r), 1))
            .collect();
        let versions = merge_versions(found, |a, b| *a += b);
        assert_eq!(versions.len(), 1);
        let ids: Vec<String> = versions[0].ids.iter().map(ToString::to_string).collect();
        assert_eq!(ids, ["openalex:W1", "openalex:W2", "openalex:W3"]);
        assert_eq!(versions[0].value, 3);
    }
}
