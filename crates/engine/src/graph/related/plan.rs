//! Which pages of citing works to read: works cited little share filters,
//! others are read alone, at most [`PAGES_PER_WORK`] pages each and
//! [`CITING_PAGES`] for the set.

use aspero_core::WorkId;

use super::super::{MAX_FILTER, any_of};

/// Works per page of citing works: OpenAlex's most.
pub(super) const PAGE: u64 = 200;

/// Pages of one work's citing works at most: its 1,000 most cited.
pub(super) const PAGES_PER_WORK: usize = 5;

/// Pages of citing works for the whole set at most. A set of 25 works cited
/// 300 times each reads 50 ($0.005), all of them.
pub(super) const CITING_PAGES: usize = 75;

/// A work of the set and how many works cite it (its `cited_by_count`).
pub(super) type Counted = (WorkId, u64);

/// A `cites:` filter of works of the set, and the pages of it to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Cites {
    pub(super) works: Vec<WorkId>,
    pub(super) pages: usize,
}

impl Cites {
    pub(super) fn filter(&self) -> String {
        cites(&self.works)
    }
}

/// The filter of the works citing any of `works`.
pub(super) fn cites(works: &[WorkId]) -> String {
    format!("cites:{}", any_of(works))
}

/// Works cited at most a page's worth of times (each with its count),
/// packed into filters that one page holds (most cited first, ties by id;
/// each into the first filter it fits).
pub(super) fn bins(works: &[(WorkId, u64)]) -> Vec<Vec<WorkId>> {
    let mut small: Vec<&(WorkId, u64)> = works.iter().collect();
    small.sort_by(|(a, x), (b, y)| y.cmp(x).then(a.cmp(b)));
    let mut bins: Vec<(Vec<WorkId>, u64)> = Vec::new();
    for (work, count) in small {
        let fits = bins
            .iter_mut()
            .find(|(ws, sum)| sum + count <= PAGE && ws.len() < MAX_FILTER);
        match fits {
            Some((ws, sum)) => {
                ws.push(work.clone());
                *sum += count;
            }
            None => bins.push((vec![work.clone()], *count)),
        }
    }
    bins.into_iter().map(|(ws, _)| ws).collect()
}

/// The filters reading the citing works of `works` (each with its
/// `cited_by_count`; uncited works need none): shared filters of a page
/// first, then works alone, least cited first, at most [`PAGES_PER_WORK`]
/// pages each. Over [`CITING_PAGES`] in all, pages come off the works read
/// most (the most cited of them first), down to none.
pub(super) fn plan(works: &[(WorkId, u64)], per_work: usize) -> Vec<Cites> {
    let (small, large): (Vec<Counted>, Vec<Counted>) = works
        .iter()
        .filter(|(_, count)| *count > 0)
        .cloned()
        .partition(|(_, count)| *count <= PAGE);
    let mut shared: Vec<Cites> = bins(&small)
        .into_iter()
        .map(|works| Cites { works, pages: 1 })
        .collect();
    let mut alone: Vec<(u64, Cites)> = large
        .into_iter()
        .map(|(work, count)| {
            let pages = (count.div_ceil(PAGE) as usize).min(per_work);
            (
                count,
                Cites {
                    works: vec![work],
                    pages,
                },
            )
        })
        .collect();
    alone.sort_by(|(x, a), (y, b)| x.cmp(y).then(a.works.cmp(&b.works)));
    let mut total = shared.len() + alone.iter().map(|(_, c)| c.pages).sum::<usize>();
    while total > CITING_PAGES {
        // The last of the most pages: the most cited of them.
        match alone
            .iter_mut()
            .filter(|(_, c)| c.pages > 0)
            .max_by_key(|(_, c)| c.pages)
        {
            Some((_, c)) => c.pages -= 1,
            None => {
                shared.pop();
            }
        }
        total -= 1;
    }
    shared
        .into_iter()
        .chain(alone.into_iter().map(|(_, c)| c))
        .collect()
}

/// The citing works of `work` (cited `count` times) a plan reads: all of
/// them, or a sample of its pages' worth.
pub(super) fn target(plan: &[Cites], work: &WorkId, count: u64) -> u64 {
    let pages = plan
        .iter()
        .find(|c| c.works.contains(work))
        .map_or(0, |c| c.pages);
    count.min(pages as u64 * PAGE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(n: u64) -> WorkId {
        format!("W{n}").parse().unwrap()
    }

    #[test]
    fn works_cited_little_share_a_page_and_classics_are_capped() {
        let works = [
            (w(1), 120),
            (w(2), 80),
            (w(3), 90),
            (w(4), 0),
            (w(5), 300),
            (w(6), 90_000),
            (w(7), 30),
        ];
        let plan = plan(&works, PAGES_PER_WORK);
        let shape: Vec<(Vec<WorkId>, usize)> =
            plan.iter().map(|c| (c.works.clone(), c.pages)).collect();
        assert_eq!(
            shape,
            [
                // 120 + 80 fill a page; 90 + 30 the next.
                (vec![w(1), w(2)], 1),
                (vec![w(3), w(7)], 1),
                (vec![w(5)], 2),
                (vec![w(6)], PAGES_PER_WORK),
            ]
        );
        assert_eq!(target(&plan, &w(6), 90_000), 1_000);
        assert_eq!(target(&plan, &w(5), 300), 300);
        assert_eq!(target(&plan, &w(4), 0), 0);
    }

    #[test]
    fn the_set_reads_at_most_its_cap_taken_off_the_most_read() {
        // 30 works cited 1,000 times: 150 pages wanted, 75 read.
        let works: Vec<(WorkId, u64)> = (1..=30).map(|n| (w(n), 1_000)).collect();
        let plan = plan(&works, PAGES_PER_WORK);
        let pages: Vec<usize> = plan.iter().map(|c| c.pages).collect();
        assert_eq!(pages.iter().sum::<usize>(), CITING_PAGES);
        // Taken evenly, the last (ties by id) first.
        assert_eq!(pages[..15], [3; 15]);
        assert_eq!(pages[15..], [2; 15]);
    }
}
