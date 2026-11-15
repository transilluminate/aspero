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
//!   sample of a classic's). The set reads at most
//!   [`CITING_PAGES`](plan::CITING_PAGES) pages, taken off the works read
//!   most first. Pages are cached per work of the
//!   set (`edges`, `cited_by`, a day), each citing work's references with
//!   them (`references`, 30 days).
//! - **Co-citation:** the citing works read that cite a work and at least
//!   one work of the set. One per citing work, however many works of the
//!   set it cites: a review citing ten of them counts once. In the score
//!   each counts 1/√(its references), so works listing hundreds (a
//!   handbook's chapters) don't make everything they list related: see
//!   [`citing_weight`].
//! - **Coupling:** how many of the set's references (all its works'
//!   references together) a work cites too, counted from
//!   [`MIN_SHARED`](score::MIN_SHARED). Counted for every work whose references are known: the citing works
//!   read, the works found citing the set's most shared references, and the
//!   works looked up. Those found citing the shared references are a fixed
//!   sample: one filter of the references at least two works of the set
//!   cite (the 100 most shared), at most [`COUPLING_PAGES`] pages of it,
//!   most cited first, cached as searches are (a day).
//! - **Score:** co-citation strength (the weights summed) and the coupling
//!   count, each as a share of the highest among the works found, weighted
//!   0.8 and 0.2 (all of it to one when no work has the other): see
//!   [`CO_CITATION`](score::CO_CITATION).
//! - The best are looked up in whole batches (one request for up to 100),
//!   so works that are one of the set (sharing any identifier with any
//!   seed, or one work with a seed) and retracted works drop, and versions
//!   of one work merge (`same_work`, counts joined), at no extra cost; then
//!   the best `limit`, ties by id.

use std::collections::{BTreeMap, BTreeSet};

use aspero_core::{Cursor, ErrorKind, Page, ProviderId, SearchQuery, SourceRecord, Usage, WorkId};
use aspero_store::{Direction, Edges};
use futures::StreamExt;

use super::seeds::Group;
use super::spend::{Purse, Spend, TheSet, merge_versions};
use super::{
    Incomplete, IncompleteKind, MAX_FILTER, MIN_CITING, Section, Seed, Stage, openalex_id,
};
use crate::{Engine, Entry};

mod plan;
mod score;

use plan::{Counted, PAGE, PAGES_PER_WORK, cites, plan, target, to_read};
use score::{Tally, citing_weight, highest, score};

/// Pages of the works citing the set's most shared references at most.
const COUPLING_PAGES: usize = 5;

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

/// Citing works, each with its references.
type Citing = Vec<(WorkId, Vec<WorkId>)>;

/// A work of the set's citing works from the cache: those whose references
/// are cached too, how many were read, and whether that's all of them.
type CachedCiting = (Citing, usize, bool);

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

/// A `cites:` filter to read: up to `paid` pages after the `cached` ones
/// (pages already read, from the first), each page read kept in the
/// search cache when `remember`.
struct Job {
    filter: String,
    paid: usize,
    cached: Vec<Page>,
    remember: bool,
}

/// What reading a filter's pages found.
#[derive(Default)]
struct Read {
    /// Each citing work read, with its references; `None` when OpenAlex
    /// lists none, or ones it can't have (see [`Read::take`]).
    works: Vec<(WorkId, Option<Vec<WorkId>>)>,
    /// Of `works`, how many came from pages cached.
    cached: usize,
    /// Pages paid for.
    paid: usize,
    /// The last page was read.
    finished: bool,
    failed: bool,
    /// Stopped by the budget (the caller's daily cap on OpenAlex).
    budget: bool,
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

/// Why each stage stopped short, if it did.
#[derive(Default)]
struct Stops {
    /// Reading the set's citing works.
    citing: Stop,
    /// Reading the works citing the set's most shared references: pages
    /// read of those meant to be.
    coupling: Stop,
    coupling_pages: (u64, u64),
    /// Looking up the best: works answered of those asked about.
    lookup: Stop,
    lookups: (u64, u64),
}

#[derive(Default, Clone, Copy)]
struct Stop {
    budget: bool,
    failed: bool,
}

impl Stops {
    /// What stopped related works: the stages that failed, then those
    /// stopped by the budget, each in stage order, then a sample (`sampled`,
    /// or `done` of `total` citing works read with nothing else stopping
    /// them), each with its stage and how far that got.
    fn incomplete(
        &self,
        (done, total): (u64, u64),
        sampled: bool,
        provider: &ProviderId,
    ) -> Vec<Incomplete> {
        let stages = [
            (Stage::Citing, self.citing, (done, total)),
            (Stage::Coupling, self.coupling, self.coupling_pages),
            (Stage::Lookup, self.lookup, self.lookups),
        ];
        let at = |kind, (stage, _, (done, total)): (Stage, Stop, (u64, u64))| {
            let mut i = Incomplete::new(kind, done as usize, total as usize).at(stage);
            if kind == IncompleteKind::ProviderFailed {
                i = i.from(provider.clone());
            }
            i
        };
        let failed = stages.iter().filter(|(_, s, _)| s.failed);
        let budget = stages.iter().filter(|(_, s, _)| s.budget);
        let short = done < total && !self.citing.failed && !self.citing.budget;
        failed
            .map(|&s| at(IncompleteKind::ProviderFailed, s))
            .chain(budget.map(|&s| at(IncompleteKind::Budget, s)))
            .chain((sampled || short).then(|| at(IncompleteKind::Capped, stages[0])))
            .collect()
    }
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
        // Without a key, OpenAlex gives only first pages: of a work cited
        // more than 200 times, its 200 most cited citing works (`capped`).
        let max_pages = match entry.provider.capabilities().max_pages {
            0 => usize::MAX,
            n => n,
        };
        // The set's works, each once, with how often each is cited.
        // A work whose count OpenAlex doesn't give is read as if cited a
        // page's worth (a page of its own), and counted by what's read.
        let mut counts: BTreeMap<WorkId, u64> = BTreeMap::new();
        let mut unknown: BTreeSet<WorkId> = BTreeSet::new();
        for group in groups {
            for &m in &group.members {
                if let Some(record) = &seeds[m].record
                    && let Some(work) = openalex_id(record)
                {
                    let count = record.work.cited_by_count.unwrap_or_else(|| {
                        unknown.insert(work.clone());
                        PAGE
                    });
                    counts.insert(work.clone(), count);
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
        let full = plan(&works, PAGES_PER_WORK.min(max_pages));
        let targets: Vec<u64> = works.iter().map(|(w, c)| target(&full, w, *c)).collect();
        let cached = self.cached_citing(&works, &targets);

        // Works the plan reads not cached: alone with the plan's pages, or
        // sharing filters again among themselves.
        let is_cached: Vec<bool> = cached.iter().map(Option::is_some).collect();
        let filters = to_read(&full, &works, &is_cached);

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
            (Some(_), _) => COUPLING_PAGES
                .min(max_pages)
                .saturating_sub(cached_pages.len()),
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
            return Section::new(Vec::new(), Vec::new());
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

        let mut jobs: Vec<Job> = Vec::new();
        for (c, &n) in filters.iter().zip(&allotted) {
            jobs.push(Job {
                filter: c.filter(),
                paid: n,
                cached: Vec::new(),
                remember: false,
            });
        }
        let coupling_cached = cached_pages.len();
        if let Some(filter) = &coupling {
            jobs.push(Job {
                filter: filter.clone(),
                paid: coupling_allotted,
                cached: cached_pages,
                remember: true,
            });
        }
        // Read four at a time, each page within what's left by what pages
        // cost (OpenAlex may charge more than list price).
        let purse = Purse::new(spend, lookups);
        let reads = futures::stream::iter(
            jobs.into_iter()
                .map(|job| self.read_citing(entry, job, &years, &purse)),
        )
        .buffered(READ_AT_ONCE)
        .collect::<Vec<Read>>()
        .await;
        let mut reads = reads.into_iter();
        let mut citing: BTreeMap<WorkId, Vec<WorkId>> = BTreeMap::new();
        let mut stops = Stops::default();

        // Each work's citing works, and how many of them were read.
        let mut found: BTreeMap<&WorkId, (Vec<WorkId>, bool)> = BTreeMap::new();
        let mut to_cache: Vec<(WorkId, Edges)> = Vec::new();
        // Citing works read live (their references to remember).
        let mut fresh: BTreeSet<WorkId> = BTreeSet::new();
        for (c, n) in filters.iter().zip(&allotted) {
            let mut read = reads.next().expect("a read per filter");
            spend.add(std::mem::take(&mut read.usage), read.paid);
            stops.citing.failed |= read.failed;
            stops.citing.budget |= read.budget || (*n < c.pages && !read.finished && !read.failed);
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
                    fresh.insert(id.clone());
                    citing.insert(id, cited);
                }
            }
        }
        if coupling.is_some() {
            let mut read = reads.next().expect("a read for coupling");
            spend.add(std::mem::take(&mut read.usage), read.paid);
            stops.coupling.failed |= read.failed;
            stops.coupling.budget |= read.budget
                || (coupling_allotted < coupling_paid && !read.finished && !read.failed);
            stops.coupling_pages = (
                (coupling_cached + read.paid) as u64,
                (coupling_cached + coupling_paid) as u64,
            );
            for (k, (id, cited)) in read.works.into_iter().enumerate() {
                if let Some(cited) = cited {
                    if k >= read.cached {
                        fresh.insert(id.clone());
                    }
                    citing.entry(id).or_insert(cited);
                }
            }
        }
        // The references read live, not those of pages cached (which got
        // their lease when read).
        let links: Vec<(&WorkId, &[WorkId])> = citing
            .iter()
            .filter(|(id, _)| fresh.contains(*id))
            .map(|(id, refs)| (id, refs.as_slice()))
            .collect();
        self.remember_citing(&to_cache, &links);

        // How many of the set's citing works were read.
        let (mut done, mut total) = (0u64, 0u64);
        // A much cited work's citing works sampled by the plan, whatever
        // else stopped the reading; or a work of unknown count not read to
        // the end: a sample, of how many no one knows.
        let mut sampled = works.iter().zip(&targets).any(|((_, count), t)| t < count);
        for (((work, count), cached), target) in works.iter().zip(cached).zip(&targets) {
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
            if unknown.contains(work) {
                (done, total) = (done + read, total + read);
                sampled |= !complete;
                continue;
            }
            total += count;
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
        let looked = self
            .look_up(entry, &ids, spend, "related works' lookup failed")
            .await;
        for stop in &looked.stops {
            stops.lookup.failed |= stop.kind == IncompleteKind::ProviderFailed;
            stops.lookup.budget |= stop.kind == IncompleteKind::Budget;
            stops.lookups = (stop.done, stop.total);
        }

        // Works of the set and retracted works drop; versions merge.
        let set = TheSet::new(seeds);
        let kept = ranked
            .into_iter()
            .zip(looked.records)
            .filter(|((id, _, _), record)| {
                !set.has(id, record.as_ref())
                    && record
                        .as_ref()
                        .is_none_or(|r| r.work.retraction().is_none())
            })
            .map(|((id, mut tally, _), record)| {
                if tally.shared.is_empty()
                    && let Some(refs) = record.as_ref().and_then(|r| r.references.as_deref())
                {
                    tally.couple(refs, &cited);
                }
                (id.clone(), record, tally)
            })
            .collect();
        let merged = merge_versions(kept, Tally::join);
        let most = highest(merged.iter().map(|m| &m.value), &weights);
        let mut items: Vec<RelatedWork> = merged
            .into_iter()
            .map(|m| {
                let tally = m.value;
                let mut seeds: Vec<usize> =
                    tally.groups.iter().map(|&g| groups[g].members[0]).collect();
                seeds.sort_unstable();
                RelatedWork {
                    id: m.id,
                    ids: m.ids,
                    record: m.record,
                    score: score(&tally, &weights, most),
                    co_cited_with: tally.citing.len(),
                    shared_references: tally.shared.len(),
                    seeds,
                }
            })
            .filter(|w| w.co_cited_with > 0 || w.shared_references > 0)
            .collect();
        items.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
        items.truncate(limit as usize);

        Section::new(items, stops.incomplete((done, total), sampled, &provider))
    }

    /// Reads a job's pages, each if the `purse` has it; `years` are the
    /// set's works' years.
    async fn read_citing(
        &self,
        entry: &Entry,
        job: Job,
        years: &BTreeMap<WorkId, i16>,
        purse: &Purse,
    ) -> Read {
        let Job {
            filter,
            paid,
            cached,
            remember,
        } = job;
        let provider = entry.provider.id();
        let mut read = Read::default();
        let mut cursor = None;
        for page in cached {
            cursor = read.take(page, years);
            read.cached = read.works.len();
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
            if !purse.take() {
                read.budget = true;
                break;
            }
            let answer = self
                .call_paid(entry, || entry.provider.search(&query))
                .await;
            let cost = match &answer {
                Ok(resp) => resp.usage.as_ref().and_then(|u| u.cost),
                Err(_) => Some(0.0),
            };
            purse.settle(cost);
            match answer {
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
                Err(e) if e.kind() == ErrorKind::BudgetCapped => {
                    tracing::info!(target: "aspero::graph", error = %e, %filter, "citing works stopped at the daily cap");
                    read.budget = true;
                    break;
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
                    cursor.clone_from(&page.next);
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
    fn remember_citing(&self, citing: &[(WorkId, Edges)], links: &[(&WorkId, &[WorkId])]) {
        let Some(cache) = &self.cache else {
            return;
        };
        let now = (self.clock)();
        let rows: Vec<(&WorkId, &Edges)> = citing.iter().map(|(w, e)| (w, e)).collect();
        let _ = cache.put_edges(Direction::CitedBy, &rows, now);
        self.remember_references(links);
    }
}
