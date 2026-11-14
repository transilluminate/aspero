//! Related works: works around a set, by co-citation and bibliographic
//! coupling, read from OpenAlex's citing works (DESIGN "Citation links").
//!
//! - **Citing works.** The works citing the set come from `cites:` filters,
//!   200 a page ($0.0001), most cited first, each only its id and references
//!   (`select=id,referenced_works`): which works of the set it cites is in
//!   its references. Works cited at most a page's worth of times share a
//!   filter, as many as one page holds by their `cited_by_count`, so they
//!   cost a page between them; others are read alone, at most
//!   [`PAGES_PER_WORK`] pages each (their 1,000 most cited citing works: a
//!   sample of a classic's). The set reads at most [`CITING_PAGES`] pages,
//!   taken off the works read most first. Pages are cached per work of the
//!   set (`edges`, `cited_by`, a day), each citing work's references with
//!   them (`references`, 30 days).
//! - **Co-citation:** the citing works read that cite a work and at least
//!   one work of the set. One per citing work, however many works of the
//!   set it cites: a review citing ten of them counts once. In the score
//!   each counts 1/√(its references), so works listing hundreds (a
//!   handbook's chapters) don't make everything they list related: see
//!   [`citing_weight`].
//! - **Coupling:** how many of the set's references (all its works'
//!   references together) a work cites too, counted from [`MIN_SHARED`].
//!   Counted for every work whose references are known: the citing works
//!   read, the works found citing the set's most shared references, and the
//!   works looked up. Those found citing the shared references are a fixed
//!   sample: one filter of the references at least two works of the set
//!   cite (the 100 most shared), at most [`COUPLING_PAGES`] pages of it,
//!   most cited first, cached as searches are (a day).
//! - **Score:** co-citation strength (the weights summed) and the coupling
//!   count, each as a share of the highest among the works found, weighted
//!   0.8 and 0.2 (all of it to one when no work has the other): see
//!   [`CO_CITATION`].
//! - The best are looked up in whole batches (one request for up to 100),
//!   so works that are one of the set (sharing any identifier with any
//!   seed, or one work with a seed) and retracted works drop, and versions
//!   of one work merge (`same_work`, counts joined), at no extra cost; then
//!   the best `limit`, ties by id.

use std::collections::{BTreeMap, BTreeSet};

use aspero_core::matching::same_work;
use aspero_core::{Cursor, FieldSet, Page, ProviderId, SearchQuery, SourceRecord, Usage, WorkId};
use aspero_store::{Direction, Edges};
use futures::StreamExt;

use super::{
    Group, Incomplete, IncompleteKind, MAX_FILTER, MIN_CITING, Section, Seed, Spend, TheSet,
    openalex_id,
};
use crate::{Engine, Entry, Outcome};

/// Works per page of citing works: OpenAlex's most.
const PAGE: u64 = 200;

/// Pages of one work's citing works at most: its 1,000 most cited.
const PAGES_PER_WORK: usize = 5;

/// Pages of citing works for the whole set at most. A set of 25 works cited
/// 300 times each reads 50 ($0.005), all of them.
const CITING_PAGES: usize = 75;

/// Pages of the works citing the set's most shared references at most.
const COUPLING_PAGES: usize = 5;

/// References in common for a work to count as coupled: one is chance.
const MIN_SHARED: usize = 2;

/// The weights of co-citation and coupling in a score. Co-citation leads:
/// what the citing literature reads together. Coupling counts favour long
/// reference lists, so weighted evenly (tried on two sets, 2026-11) they
/// ranked broad reviews, theses and off-topic works (AI in food science)
/// above the field's core works; they still rank works no one cites yet.
const CO_CITATION: f64 = 0.8;
const COUPLING: f64 = 0.2;

/// Pages read at once (different filters; one filter's pages run in turn).
const READ_AT_ONCE: usize = 4;

const SELECT: &str = "id,publication_year,referenced_works";
const SORT: &str = "cited_by_count:desc";

/// A work around the set: co-cited with it, coupled to it, or both.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RelatedWork {
    /// Its OpenAlex id (the best scored, when versions merged).
    pub id: WorkId,
    /// Every OpenAlex id merged into it (preprint and published versions).
    pub ids: Vec<WorkId>,
    /// Its record, when looked up.
    pub record: Option<SourceRecord>,
    /// From 0 to 1: its co-citation strength (each citing work counting
    /// 1/√(its references)) and coupling count, each as a share of the
    /// highest among the works found, weighted 0.8 and 0.2 (all to one when
    /// no work has the other).
    pub score: f64,
    /// The citing works read that cite it and at least one work of the set.
    pub co_cited_with: usize,
    /// The set's references it cites too (counted from two).
    pub shared_references: usize,
    /// The seeds linking it (co-cited with it, or sharing references with
    /// it), one per work of the set (the first seed of merged ones), in
    /// order.
    pub seeds: Vec<usize>,
}

/// A work of the set and how many works cite it (its `cited_by_count`).
type Counted = (WorkId, u64);

/// Citing works, each with its references.
type Citing = Vec<(WorkId, Vec<WorkId>)>;

/// A work of the set's citing works from the cache: those whose references
/// are cached too, how many were read, and whether that's all of them.
type CachedCiting = (Citing, usize, bool);

/// A `cites:` filter of works of the set, and the pages of it to read.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Cites {
    works: Vec<WorkId>,
    pages: usize,
}

impl Cites {
    fn filter(&self) -> String {
        cites(&self.works)
    }
}

fn cites(works: &[WorkId]) -> String {
    let values: Vec<String> = works
        .iter()
        .map(|w| match w {
            WorkId::OpenAlex(id) => id.to_string(),
            other => other.to_string(),
        })
        .collect();
    format!("cites:{}", values.join("|"))
}

/// Works cited at most a page's worth of times (each with its count),
/// packed into filters that one page holds (most cited first, ties by id;
/// each into the first filter it fits).
fn bins(works: &[(WorkId, u64)]) -> Vec<Vec<WorkId>> {
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
fn plan(works: &[(WorkId, u64)]) -> Vec<Cites> {
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
            let pages = (count.div_ceil(PAGE) as usize).min(PAGES_PER_WORK);
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
fn target(plan: &[Cites], work: &WorkId, count: u64) -> u64 {
    let pages = plan
        .iter()
        .find(|c| c.works.contains(work))
        .map_or(0, |c| c.pages);
    count.min(pages as u64 * PAGE)
}

/// The references at least two works of the set cite, most shared first
/// (ties by id), as many as one filter takes.
fn most_shared(groups: &[Group]) -> Vec<WorkId> {
    let mut citing: BTreeMap<&WorkId, usize> = BTreeMap::new();
    for group in groups {
        for cited in group.references.iter().flatten() {
            *citing.entry(cited).or_default() += 1;
        }
    }
    let mut shared: Vec<(&WorkId, usize)> = citing
        .into_iter()
        .filter(|(_, n)| *n >= MIN_CITING)
        .collect();
    shared.sort_by(|(a, x), (b, y)| y.cmp(x).then(a.cmp(b)));
    shared
        .into_iter()
        .take(MAX_FILTER)
        .map(|(w, _)| w.clone())
        .collect()
}

/// A page of citing works, as asked for.
fn citing_query(filter: &str, cursor: Option<Cursor>) -> SearchQuery {
    let mut query = SearchQuery::new("");
    query.page.limit = PAGE as u32;
    query.page.cursor = cursor;
    query.extra.insert("filter".into(), filter.to_owned());
    query.extra.insert("select".into(), SELECT.into());
    query.extra.insert("sort".into(), SORT.into());
    query
}

/// What reading a filter's pages found.
#[derive(Default)]
struct Read {
    /// Each citing work read, with its references; `None` when OpenAlex
    /// lists none, or ones it can't have (see [`Read::take`]).
    works: Vec<(WorkId, Option<Vec<WorkId>>)>,
    /// Pages paid for.
    paid: usize,
    /// The last page was read.
    finished: bool,
    failed: bool,
    usage: Vec<(ProviderId, Usage)>,
}

impl Read {
    /// Keeps a page's works, without the references of those dated more
    /// than a year before a work of the set they cite (`years`): OpenAlex
    /// sometimes gives a work another's references (a 2001 paper on mustard
    /// citing 2014 sepsis trials, seen 2026-11).
    fn take(&mut self, page: Page, years: &BTreeMap<WorkId, i16>) -> Option<Cursor> {
        for hit in page.hits {
            let Some(id) = openalex_id(&hit.record).cloned() else {
                continue;
            };
            let year = hit.record.work.issued.map(|d| d.year());
            let cited = hit.record.references.filter(|cited| {
                year.is_none_or(|year| {
                    !cited
                        .iter()
                        .any(|c| years.get(c).is_some_and(|&y| y > year + 1))
                })
            });
            self.works.push((id, cited));
        }
        if page.next.is_none() {
            self.finished = true;
        }
        page.next
    }
}

/// Counts for one work around the set.
#[derive(Debug, Clone, Default)]
struct Tally {
    /// The citing works (indexes) that cite it and a work of the set.
    citing: Vec<u32>,
    /// The set's references it cites too, when at least [`MIN_SHARED`].
    shared: BTreeSet<WorkId>,
    /// The works of the set (groups) linking it.
    groups: BTreeSet<usize>,
}

impl Tally {
    /// Counts `refs` (a work's references) as coupling, with `cited` the
    /// works of the set citing each of the set's references. It links the
    /// works of the set it shares at least [`MIN_SHARED`] references with.
    fn couple(&mut self, refs: &[WorkId], cited: &BTreeMap<&WorkId, BTreeSet<usize>>) -> bool {
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

    fn join(&mut self, other: Tally) {
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
fn score(t: &Tally, weights: &[f64], (most_cited, most_shared): (f64, usize)) -> f64 {
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
fn citing_weight(references: usize) -> f64 {
    1.0 / (references.max(1) as f64).sqrt()
}

/// A work's co-citation strength: its citing works' weights, summed.
fn co_citation(t: &Tally, weights: &[f64]) -> f64 {
    t.citing.iter().map(|&i| weights[i as usize]).sum()
}

fn highest<'a>(tallies: impl Iterator<Item = &'a Tally>, weights: &[f64]) -> (f64, usize) {
    tallies.fold((0.0, 0), |(c, s), t| {
        (c.max(co_citation(t, weights)), s.max(t.shared.len()))
    })
}

/// Why a step stopped short.
#[derive(Default)]
struct Stops {
    budget: bool,
    failed: bool,
}

impl Engine {
    /// Works around the set by co-citation and coupling, the best `limit`
    /// (see the module's notes). Estimating, it counts the pages it would
    /// read (exactly, from the works' `cited_by_count`s and the cache) and
    /// the coupling pages and lookups (a ceiling).
    pub(super) async fn related(
        &self,
        entry: &Entry,
        seeds: &[Seed],
        groups: &[Group],
        limit: u32,
        spend: &mut Spend,
    ) -> Section<RelatedWork> {
        let provider = entry.provider.id();
        // The set's works, each once, with how often each is cited.
        let mut counts: BTreeMap<WorkId, u64> = BTreeMap::new();
        for group in groups {
            for &m in &group.members {
                if let Some(record) = &seeds[m].record
                    && let Some(work) = openalex_id(record)
                {
                    counts.insert(work.clone(), record.work.cited_by_count.unwrap_or(0));
                }
            }
        }
        let works: Vec<(WorkId, u64)> = counts.into_iter().collect();
        let years: BTreeMap<WorkId, i16> = seeds
            .iter()
            .filter_map(|s| {
                let record = s.record.as_ref()?;
                Some((openalex_id(record)?.clone(), record.work.issued?.year()))
            })
            .collect();
        let full = plan(&works);
        let targets: Vec<u64> = works.iter().map(|(w, c)| target(&full, w, *c)).collect();
        let cached = self.cached_citing(&works, &targets);

        // Works not cached: read alone with the plan's pages, or sharing
        // filters again among themselves.
        let mut filters: Vec<Cites> = Vec::new();
        let small: Vec<(WorkId, u64)> = works
            .iter()
            .zip(&cached)
            .filter(|((_, c), cached)| cached.is_none() && (1..=PAGE).contains(c))
            .map(|(w, _)| w.clone())
            .collect();
        filters.extend(
            bins(&small)
                .into_iter()
                .map(|works| Cites { works, pages: 1 }),
        );
        filters.extend(
            full.iter()
                .filter(|c| {
                    c.pages > 0
                        && c.works.len() == 1
                        && works.iter().zip(&cached).any(|((w, n), cached)| {
                            cached.is_none() && *n > PAGE && *w == c.works[0]
                        })
                })
                .cloned(),
        );

        // Coupling: the works citing the set's most shared references.
        let shared = most_shared(groups);
        let coupling = (!shared.is_empty()).then(|| cites(&shared));
        let cached_pages = coupling
            .as_deref()
            .map(|f| self.cached_pages(&provider, f))
            .unwrap_or_default();
        let coupling_paid = match (&coupling, cached_pages.last()) {
            (None, _) => 0,
            (Some(_), Some(last)) if last.next.is_none() => 0,
            (Some(_), _) => COUPLING_PAGES - cached_pages.len(),
        };

        let citing_paid: usize = filters.iter().map(|c| c.pages).sum();
        let anything = works.iter().any(|(_, c)| *c > 0) || coupling.is_some();
        let lookups = if anything {
            (limit as usize).div_ceil(MAX_FILTER)
        } else {
            0
        };
        if spend.estimating {
            spend.requests += (citing_paid + coupling_paid + lookups) as u64;
            spend.ceiling |= coupling_paid + lookups > 0;
            return Section::new(Vec::new(), None);
        }

        // The budget, less the lookups, is shared out before reading, a
        // page at a time: each filter's first page, then second pages, …;
        // coupling's last.
        let mut left = spend.affordable().saturating_sub(lookups);
        let mut allotted = vec![0usize; filters.len()];
        let rounds = filters.iter().map(|c| c.pages).max().unwrap_or(0);
        for round in 0..rounds {
            for (c, n) in filters.iter().zip(allotted.iter_mut()) {
                if c.pages > round && left > 0 {
                    *n += 1;
                    left -= 1;
                }
            }
        }
        let coupling_allotted = coupling_paid.min(left);

        let mut jobs: Vec<(String, usize, Vec<Page>, bool)> = Vec::new();
        for (c, &n) in filters.iter().zip(&allotted) {
            jobs.push((c.filter(), n, Vec::new(), false));
        }
        if let Some(filter) = &coupling {
            jobs.push((filter.clone(), coupling_allotted, cached_pages, true));
        }
        let reads =
            futures::stream::iter(jobs.into_iter().map(|(filter, paid, cached, remember)| {
                self.read_citing(entry, filter, paid, cached, remember, &years)
            }))
            .buffered(READ_AT_ONCE)
            .collect::<Vec<Read>>()
            .await;
        let mut reads = reads.into_iter();
        let mut citing: BTreeMap<WorkId, Vec<WorkId>> = BTreeMap::new();
        let mut stops = Stops::default();

        // Each work's citing works, and how many of them were read.
        let mut found: BTreeMap<&WorkId, (Vec<WorkId>, bool)> = BTreeMap::new();
        let mut to_cache: Vec<(WorkId, Edges)> = Vec::new();
        let mut links: Vec<(WorkId, Vec<WorkId>)> = Vec::new();
        for (c, n) in filters.iter().zip(&allotted) {
            let mut read = reads.next().expect("a read per filter");
            spend.add(std::mem::take(&mut read.usage), read.paid);
            stops.failed |= read.failed;
            stops.budget |= *n < c.pages && !read.finished && !read.failed;
            for work in &c.works {
                // Alone, every work read cites it; sharing, those whose
                // references say so.
                let ids: Vec<WorkId> = read
                    .works
                    .iter()
                    .filter(|(_, cited)| {
                        c.works.len() == 1 || cited.as_ref().is_some_and(|r| r.contains(work))
                    })
                    .map(|(id, _)| id.clone())
                    .collect();
                if !read.failed && *n > 0 {
                    to_cache.push((work.clone(), Edges::new(ids.clone(), read.finished)));
                }
                found.insert(work, (ids, read.finished));
            }
            for (id, cited) in read.works {
                if let Some(cited) = cited {
                    links.push((id.clone(), cited.clone()));
                    citing.insert(id, cited);
                }
            }
        }
        if coupling.is_some() {
            let mut read = reads.next().expect("a read for coupling");
            spend.add(std::mem::take(&mut read.usage), read.paid);
            stops.failed |= read.failed;
            stops.budget |= coupling_allotted < coupling_paid && !read.finished && !read.failed;
            for (id, cited) in read.works {
                if let Some(cited) = cited {
                    links.push((id.clone(), cited.clone()));
                    citing.entry(id).or_insert(cited);
                }
            }
        }
        self.remember_citing(&to_cache, &links);

        // How many of the set's citing works were read.
        let (mut done, mut total) = (0u64, 0u64);
        for (((work, count), cached), target) in works.iter().zip(cached).zip(&targets) {
            total += count;
            let (read, complete) = match cached {
                Some((cited, n, complete)) => {
                    for (id, refs) in cited {
                        citing.entry(id).or_insert(refs);
                    }
                    (n as u64, complete)
                }
                None => match found.get(work) {
                    Some((ids, finished)) => (ids.len() as u64, *finished),
                    None => (0, *target == 0 && *count == 0),
                },
            };
            done += if complete { *count } else { read.min(*count) };
        }

        // Counts: co-citation from every citing work read that cites the
        // set; coupling for every work read (not of the set).
        let group_of: BTreeMap<&WorkId, usize> = groups
            .iter()
            .enumerate()
            .flat_map(|(g, group)| group.works.iter().map(move |w| (w, g)))
            .collect();
        let mut cited: BTreeMap<&WorkId, BTreeSet<usize>> = BTreeMap::new();
        for (g, group) in groups.iter().enumerate() {
            for r in group.references.iter().flatten() {
                cited.entry(r).or_default().insert(g);
            }
        }
        let mut tallies: BTreeMap<&WorkId, Tally> = BTreeMap::new();
        let weights: Vec<f64> = citing.values().map(|r| citing_weight(r.len())).collect();
        for (i, (work, refs)) in citing.iter().enumerate() {
            let of_set: BTreeSet<usize> = refs
                .iter()
                .filter_map(|r| group_of.get(r).copied())
                .collect();
            if !of_set.is_empty() {
                for r in refs.iter().filter(|r| !group_of.contains_key(r)) {
                    let t = tallies.entry(r).or_default();
                    t.citing.push(i as u32);
                    t.groups.extend(&of_set);
                }
            }
            if !group_of.contains_key(work) {
                let mut t = Tally::default();
                if t.couple(refs, &cited) {
                    tallies.entry(work).or_default().join(t);
                }
            }
        }
        let most = highest(tallies.values(), &weights);
        let mut ranked: Vec<(&WorkId, Tally, f64)> = tallies
            .into_iter()
            .map(|(w, t)| {
                let s = score(&t, &weights, most);
                (w, t, s)
            })
            .collect();
        ranked.sort_by(|(a, _, x), (b, _, y)| y.total_cmp(x).then(a.cmp(b)));

        // The best looked up in whole batches.
        let fetch = ranked.len().min(lookups * MAX_FILTER);
        ranked.truncate(fetch);
        let ids: Vec<WorkId> = ranked.iter().map(|(w, _, _)| (*w).clone()).collect();
        let uncached = self
            .cached_lookups(&provider, &ids, FieldSet::DEFAULT)
            .iter()
            .filter(|c| c.is_none())
            .count();
        let paid = match uncached {
            0 | 1 => 0,
            n => n.div_ceil(MAX_FILTER),
        };
        let mut records: Vec<Option<SourceRecord>> = vec![None; fetch];
        if !ids.is_empty() {
            if spend.allows(paid) {
                let mut usage = Vec::new();
                let answers = self
                    .lookup_at(entry, &ids, FieldSet::DEFAULT, &mut usage)
                    .await;
                spend.add(usage, paid);
                stops.failed |= answers
                    .iter()
                    .any(|(a, _)| matches!(a.outcome, Outcome::Failed(_)));
                records = answers.into_iter().map(|(_, r)| r).collect();
            } else {
                stops.budget = true;
            }
        }

        // Works of the set and retracted works drop; versions merge.
        let set = TheSet::new(seeds);
        let mut merged: Vec<(RelatedWork, Tally)> = Vec::new();
        for ((id, mut tally, _), record) in ranked.into_iter().zip(records) {
            if set.has(id, record.as_ref())
                || record
                    .as_ref()
                    .is_some_and(|r| r.work.retraction().is_some())
            {
                continue;
            }
            if tally.shared.is_empty()
                && let Some(refs) = record.as_ref().and_then(|r| r.references.as_deref())
            {
                tally.couple(refs, &cited);
            }
            let same = record.as_ref().and_then(|r| {
                merged
                    .iter_mut()
                    .find(|(m, _)| m.record.as_ref().is_some_and(|other| same_work(other, r)))
            });
            match same {
                Some((work, joined)) => {
                    work.ids.push(id.clone());
                    joined.join(tally);
                }
                None => merged.push((
                    RelatedWork {
                        id: id.clone(),
                        ids: vec![id.clone()],
                        record,
                        score: 0.0,
                        co_cited_with: 0,
                        shared_references: 0,
                        seeds: Vec::new(),
                    },
                    tally,
                )),
            }
        }
        let most = highest(merged.iter().map(|(_, t)| t), &weights);
        let mut items: Vec<RelatedWork> = merged
            .into_iter()
            .map(|(mut work, tally)| {
                work.score = score(&tally, &weights, most);
                work.co_cited_with = tally.citing.len();
                work.shared_references = tally.shared.len();
                work.seeds = tally.groups.iter().map(|&g| groups[g].members[0]).collect();
                work.seeds.sort_unstable();
                work
            })
            .filter(|w| w.co_cited_with > 0 || w.shared_references > 0)
            .collect();
        items.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
        items.truncate(limit as usize);

        let kind = if stops.failed {
            Some(IncompleteKind::ProviderFailed)
        } else if stops.budget {
            Some(IncompleteKind::Budget)
        } else if done < total {
            Some(IncompleteKind::Capped)
        } else {
            None
        };
        let incomplete = kind.map(|kind| Incomplete { kind, done, total });
        Section::new(items, incomplete)
    }

    /// Reads up to `paid` pages of a `cites:` filter after the `cached` ones
    /// (pages already read, from the first), keeping each page read in the
    /// search cache when `remember`; `years` are the set's works' years.
    async fn read_citing(
        &self,
        entry: &Entry,
        filter: String,
        paid: usize,
        cached: Vec<Page>,
        remember: bool,
        years: &BTreeMap<WorkId, i16>,
    ) -> Read {
        let provider = entry.provider.id();
        let mut read = Read::default();
        let mut cursor = None;
        for page in cached {
            cursor = read.take(page, years);
            if cursor.is_none() {
                return read;
            }
        }
        for _ in 0..paid {
            let query = citing_query(&filter, cursor.take());
            if self.options.offline {
                read.failed = true;
                break;
            }
            match self
                .call_paid(entry, || entry.provider.search(&query))
                .await
            {
                Ok(resp) => {
                    read.paid += 1;
                    if let Some(u) = resp.usage {
                        crate::add_usage(&mut read.usage, &provider, u);
                    }
                    if remember && let Some(cache) = &self.cache {
                        let _ = cache.put_search(&provider, &query, &resp.value, (self.clock)());
                    }
                    let empty = resp.value.hits.is_empty();
                    cursor = read.take(resp.value, years);
                    if cursor.is_none() || empty {
                        break;
                    }
                }
                Err(e) => {
                    tracing::warn!(target: "aspero::graph", error = %e, %filter, "citing works failed");
                    read.failed = true;
                    break;
                }
            }
        }
        read
    }

    /// The pages of a `cites:` filter the search cache holds fresh (or, offline, at all), from the first.
    fn cached_pages(&self, provider: &ProviderId, filter: &str) -> Vec<Page> {
        let mut pages = Vec::new();
        let mut cursor = None;
        while pages.len() < COUPLING_PAGES {
            let query = citing_query(filter, cursor.take());
            match self.cached_search(provider, &query) {
                Some((page, fresh)) if fresh || self.options.offline => {
                    cursor = page.next.clone();
                    pages.push(page);
                    if cursor.is_none() {
                        break;
                    }
                }
                _ => break,
            }
        }
        pages
    }

    /// For each work of the set (with its count and the citing works to
    /// read of it), its citing works from the cache, when cached within a
    /// day (any age, offline) and covering the target (all of them, or a
    /// sample at least that big): those whose references are cached (the
    /// others had none to use), how many were read, and whether that's all.
    fn cached_citing(&self, works: &[Counted], targets: &[u64]) -> Vec<Option<CachedCiting>> {
        let none = || vec![None; works.len()];
        let Some(cache) = &self.cache else {
            return none();
        };
        let fresh = |stored: &aspero_store::Entry<Edges>, ttl| {
            self.options.offline || self.current(stored, ttl)
        };
        let ids: Vec<WorkId> = works.iter().map(|(w, _)| w.clone()).collect();
        let Ok(rows) = cache.get_edges(Direction::CitedBy, &ids) else {
            return none();
        };
        let rows: Vec<Option<(Vec<WorkId>, bool)>> = rows
            .into_iter()
            .zip(works.iter().zip(targets))
            .map(|(row, ((_, count), target))| {
                let row = row.filter(|r| fresh(r, self.cache_policy.cited_by))?;
                let enough = match *target >= *count {
                    true => row.value.complete,
                    false => row.value.ids.len() as u64 >= *target,
                };
                (*target > 0 && enough).then(|| {
                    let mut ids = row.value.ids;
                    if !row.value.complete {
                        ids.truncate(*target as usize);
                    }
                    (ids, row.value.complete)
                })
            })
            .collect();
        let citing: Vec<WorkId> = rows
            .iter()
            .flatten()
            .flat_map(|(ids, _)| ids.iter().cloned())
            .collect();
        let Ok(refs) = cache.get_edges(Direction::References, &citing) else {
            return none();
        };
        let refs: BTreeMap<WorkId, Vec<WorkId>> = citing
            .into_iter()
            .zip(refs)
            .filter_map(|(id, row)| {
                let row = row.filter(|r| fresh(r, self.cache_policy.references))?;
                Some((id, row.value.ids))
            })
            .collect();
        rows.into_iter()
            .map(|row| {
                let (ids, complete) = row?;
                let read = ids.len();
                let cited: Citing = ids
                    .into_iter()
                    .filter_map(|id| {
                        let r = refs.get(&id)?.clone();
                        Some((id, r))
                    })
                    .collect();
                Some((cited, read, complete))
            })
            .collect()
    }

    /// Caches the citing works read of each work of the set, and each
    /// citing work's references.
    fn remember_citing(&self, citing: &[(WorkId, Edges)], links: &[(WorkId, Vec<WorkId>)]) {
        let Some(cache) = &self.cache else {
            return;
        };
        let now = (self.clock)();
        let rows: Vec<(&WorkId, &Edges)> = citing.iter().map(|(w, e)| (w, e)).collect();
        let _ = cache.put_edges(Direction::CitedBy, &rows, now);
        let links: Vec<(&WorkId, &[WorkId])> =
            links.iter().map(|(w, r)| (w, r.as_slice())).collect();
        self.remember_references(&links);
    }
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
        let plan = plan(&works);
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

    #[test]
    fn the_set_reads_at_most_its_cap_taken_off_the_most_read() {
        // 30 works cited 1,000 times: 150 pages wanted, 75 read.
        let works: Vec<(WorkId, u64)> = (1..=30).map(|n| (w(n), 1_000)).collect();
        let plan = plan(&works);
        let pages: Vec<usize> = plan.iter().map(|c| c.pages).collect();
        assert_eq!(pages.iter().sum::<usize>(), CITING_PAGES);
        // Taken evenly, the last (ties by id) first.
        assert_eq!(pages[..15], [3; 15]);
        assert_eq!(pages[15..], [2; 15]);
    }
}
