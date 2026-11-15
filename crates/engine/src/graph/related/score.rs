//! Scores: co-citation strength and the coupling count, each as a share
//! of the highest found, weighted [`CO_CITATION`] and [`COUPLING`].

use std::collections::{BTreeMap, BTreeSet};

use aspero_core::WorkId;

/// References in common for a work to count as coupled: one is chance.
pub(super) const MIN_SHARED: usize = 2;

/// The weights of co-citation and coupling in a score. Co-citation leads:
/// what the citing literature reads together. Coupling counts favour long
/// reference lists, so weighted evenly (tried on two sets, 2026-11) they
/// ranked broad reviews, theses and off-topic works (AI in food science)
/// above the field's core works; they still rank works no one cites yet.
pub(super) const CO_CITATION: f64 = 0.8;
pub(super) const COUPLING: f64 = 0.2;

/// Counts for one work around the set.
#[derive(Debug, Clone, Default)]
pub(super) struct Tally {
    /// The citing works (indexes) that cite it and a work of the set.
    pub(super) citing: Vec<u32>,
    /// The set's references it cites too, when at least [`MIN_SHARED`].
    pub(super) shared: BTreeSet<WorkId>,
    /// The works of the set (groups) linking it.
    pub(super) groups: BTreeSet<usize>,
}

impl Tally {
    /// Counts `refs` (a work's references) as coupling, with `cited` the
    /// works of the set citing each of the set's references. It links the
    /// works of the set it shares at least [`MIN_SHARED`] references with.
    pub(super) fn couple(
        &mut self,
        refs: &[WorkId],
        cited: &BTreeMap<&WorkId, BTreeSet<usize>>,
    ) -> bool {
        let shared: Vec<&WorkId> = refs.iter().filter(|r| cited.contains_key(r)).collect();
        if shared.len() < MIN_SHARED {
            return false;
        }
        let mut with: BTreeMap<usize, usize> = BTreeMap::new();
        for r in shared {
            self.shared.insert(r.clone());
            for &g in &cited[r] {
                *with.entry(g).or_default() += 1;
            }
        }
        self.groups.extend(
            with.into_iter()
                .filter(|(_, n)| *n >= MIN_SHARED)
                .map(|(g, _)| g),
        );
        true
    }

    pub(super) fn join(&mut self, other: Tally) {
        self.citing.extend(other.citing);
        self.citing.sort_unstable();
        self.citing.dedup();
        self.shared.extend(other.shared);
        self.groups.extend(other.groups);
    }
}

/// Co-citation strength ([`co_citation`]) and the coupling count, each as
/// a share of the highest, weighted [`CO_CITATION`] and [`COUPLING`] (all
/// of it to one when no work has the other), to four places.
pub(super) fn score(t: &Tally, weights: &[f64], (most_cited, most_shared): (f64, usize)) -> f64 {
    let co_cited = co_citation(t, weights);
    let shared = t.shared.len() as f64;
    let score = match (most_cited > 0.0, most_shared > 0) {
        (true, true) => {
            CO_CITATION * co_cited / most_cited + COUPLING * shared / most_shared as f64
        }
        (true, false) => co_cited / most_cited,
        (false, true) => shared / most_shared as f64,
        (false, false) => 0.0,
    };
    (score * 10_000.0).round() / 10_000.0
}

/// What a citing work's co-citation is worth: 1/√(its references). A work
/// listing hundreds co-cites everything it lists: a handbook's chapters
/// (the Oxford Handbook of Clinical Specialties, 2020: 26 chapter records,
/// each with the whole book's ~970 references) made CRASH-2, an airway
/// guideline and parvovirus B19 "related" to ED analgesia trials. Tried on
/// two sets (2026-11): 1/n favoured citing works listing a few references
/// (partial data), and 1/ln(e + n) left the chapters' picks in the top 20;
/// 1/√n dropped them and kept a set of sepsis trials' top 20 as it was.
pub(super) fn citing_weight(references: usize) -> f64 {
    1.0 / (references.max(1) as f64).sqrt()
}

/// A work's co-citation strength: its citing works' weights, summed.
pub(super) fn co_citation(t: &Tally, weights: &[f64]) -> f64 {
    t.citing.iter().map(|&i| weights[i as usize]).sum()
}

pub(super) fn highest<'a>(
    tallies: impl Iterator<Item = &'a Tally>,
    weights: &[f64],
) -> (f64, usize) {
    tallies.fold((0.0, 0), |(c, s), t| {
        (c.max(co_citation(t, weights)), s.max(t.shared.len()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn citing_works_listing_hundreds_count_for_less() {
        // Three papers of 30 references co-cite A; five chapters of a
        // handbook, each listing its 900, co-cite B.
        let weights: Vec<f64> = [30, 30, 30, 900, 900, 900, 900, 900]
            .map(citing_weight)
            .to_vec();
        let a = Tally {
            citing: vec![0, 1, 2],
            ..Tally::default()
        };
        let b = Tally {
            citing: vec![3, 4, 5, 6, 7],
            ..Tally::default()
        };
        let most = highest([&a, &b].into_iter(), &weights);
        assert_eq!(score(&a, &weights, most), 1.0);
        // 5/√900 of 3/√30: about a third.
        assert_eq!(score(&b, &weights, most), 0.3043);
        assert_eq!(citing_weight(0), 1.0, "no references: no division by zero");
    }
}
