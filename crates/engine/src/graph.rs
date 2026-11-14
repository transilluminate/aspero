//! Citation links: a work's references, and analyses of a set of works
//! over them (DESIGN "Citation links").
//!
//! Links come from OpenAlex's `referenced_works`, so the nodes are OpenAlex
//! work ids and every call needs the OpenAlex provider. A work whose
//! references OpenAlex doesn't list has unknown references, never none
//! (see [`SourceRecord::references`]).
//!
//! Costs are OpenAlex's: a lookup of one id is free, a batch of up to 100
//! ids (an `openalex:`/`doi:`/`pmid:` filter) costs [`LIST_PRICE`], as does
//! each check of 100 references for retractions (`openalex:…,is_retracted:true`)
//! and each page of 200 citing works (`cites:…`, for related works).
//! An analysis spends within its budget: before each paid call, it stops if
//! the call could take the spend past the budget.

use std::collections::{BTreeMap, BTreeSet};

use aspero_core::matching::{same_work, same_work_id};
use aspero_core::{
    Cursor, FieldSet, IdKind, ProviderId, SearchQuery, SourceRecord, Update, Usage, WorkId,
};
use aspero_store::{Direction, Edges};
use futures::StreamExt;

use crate::{CallOptions, Engine, EngineError, Entry, LookupResult, Outcome, Served, add_usage};

mod related;

pub use related::RelatedWork;

/// The provider citation links come from.
const OPENALEX: &str = "openalex";

/// What OpenAlex charges for one list or filter call, in US dollars
/// (help.openalex.org, 2026-11). Lookups of one id are free.
pub const LIST_PRICE: f64 = 0.0001;

/// Most values OpenAlex ORs in one filter, and most works per page.
const MAX_FILTER: usize = 100;

/// Seeds that must cite a work for it to be probably missing.
const MIN_CITING: usize = 2;

/// Lookups of seeds at once when estimating (each free, one id each).
const ESTIMATE_LOOKUPS: usize = 4;

/// Whether a work's references are known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Coverage {
    /// The source lists the work's references.
    Known,
    /// The source has no reference data for the work (books, older work,
    /// publishers that deposit no references): it may cite anything.
    Unknown,
}

impl Coverage {
    every_variant!(Coverage: Known, Unknown);

    pub fn of(references: Option<&[WorkId]>) -> Self {
        match references {
            Some(_) => Coverage::Known,
            None => Coverage::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Coverage::Known => "known",
            Coverage::Unknown => "unknown",
        }
    }
}

/// A page of a work's references.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ReferencesPage {
    /// The work's OpenAlex id.
    pub work: WorkId,
    /// The work's own record, when it was looked up (not when its
    /// references were cached).
    pub record: Option<SourceRecord>,
    pub coverage: Coverage,
    /// How many works it cites; `None` when unknown.
    pub total: Option<usize>,
    /// This page of them, in the source's order (OpenAlex lists them by
    /// id, not as the work does), each looked up.
    pub references: Vec<LookupResult>,
    /// Where the next page starts; `None` on the last.
    pub next_cursor: Option<Cursor>,
    pub usage: Vec<(ProviderId, Usage)>,
}

/// An analysis of a set of works.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AnalysisKind {
    /// Which seed cites which retracted work.
    RetractedReferences,
    /// Seeds linked to no other seed.
    OddOneOut,
    /// Works that several seeds cite but the set lacks: the best `limit`.
    ProbablyMissing { limit: u32 },
    /// Works around the set by co-citation and bibliographic coupling:
    /// the best `limit`.
    Related { limit: u32 },
}

impl AnalysisKind {
    every_variant!(AnalysisKind:
        RetractedReferences,
        OddOneOut,
        ProbablyMissing { limit: 20 },
        Related { limit: 20 },
    );

    pub fn as_str(self) -> &'static str {
        match self {
            AnalysisKind::RetractedReferences => "retracted_references",
            AnalysisKind::OddOneOut => "odd_one_out",
            AnalysisKind::ProbablyMissing { .. } => "probably_missing",
            AnalysisKind::Related { .. } => "related",
        }
    }
}

/// Why a seed has no OpenAlex work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SeedProblem {
    /// Not an identifier.
    InvalidId,
    /// OpenAlex can't look up this kind of identifier (an ISBN), and no
    /// record found by it carries a DOI or PMID it can.
    UnsupportedId,
    /// OpenAlex has no such work.
    NotFound,
    /// The lookup failed (asked once: seeds aren't retried).
    ProviderFailed,
    /// Not looked up: the budget ran out first.
    OverBudget,
}

impl SeedProblem {
    every_variant!(SeedProblem: InvalidId, UnsupportedId, NotFound, ProviderFailed, OverBudget);

    pub fn as_str(self) -> &'static str {
        match self {
            SeedProblem::InvalidId => "invalid_id",
            SeedProblem::UnsupportedId => "unsupported_id",
            SeedProblem::NotFound => "not_found",
            SeedProblem::ProviderFailed => "provider_failed",
            SeedProblem::OverBudget => "over_budget",
        }
    }
}

/// One work of the set, as given and as resolved.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Seed {
    /// As given.
    pub input: String,
    /// Its OpenAlex record, when resolved.
    pub record: Option<SourceRecord>,
    /// Why it isn't resolved.
    pub problem: Option<SeedProblem>,
    /// The problem in English.
    pub message: Option<String>,
    /// The earlier seed that is the same work (a preprint and its
    /// published version, or the same work given twice): the two count
    /// as one.
    pub same_as: Option<usize>,
}

impl Seed {
    fn unresolved(input: &str, problem: SeedProblem, message: impl Into<String>) -> Self {
        Seed {
            input: input.to_owned(),
            record: None,
            problem: Some(problem),
            message: Some(message.into()),
            same_as: None,
        }
    }

    /// Its OpenAlex work id, when resolved.
    pub fn work(&self) -> Option<&WorkId> {
        self.record.as_ref().and_then(openalex_id)
    }

    /// The works it cites; `None` when unresolved or unknown.
    pub fn references(&self) -> Option<&[WorkId]> {
        self.record.as_ref()?.references.as_deref()
    }

    pub fn coverage(&self) -> Option<Coverage> {
        self.record
            .as_ref()
            .map(|r| Coverage::of(r.references.as_deref()))
    }
}

/// Why part of an analysis is missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum IncompleteKind {
    /// Stopped at a fixed cap: related works read a sample of a much
    /// cited work's citing works.
    Capped,
    /// The budget ran out.
    Budget,
    /// A provider failed.
    ProviderFailed,
}

impl IncompleteKind {
    every_variant!(IncompleteKind: Capped, Budget, ProviderFailed);

    pub fn as_str(self) -> &'static str {
        match self {
            IncompleteKind::Capped => "capped",
            IncompleteKind::Budget => "budget",
            IncompleteKind::ProviderFailed => "provider_failed",
        }
    }
}

/// How much of a step was done: `done` of `total` (seeds resolved,
/// references checked, works looked up, citing works read).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Incomplete {
    pub kind: IncompleteKind,
    pub done: u64,
    pub total: u64,
}

impl Incomplete {
    pub fn new(kind: IncompleteKind, done: usize, total: usize) -> Self {
        Incomplete {
            kind,
            done: done as u64,
            total: total as u64,
        }
    }
}

/// One analysis' findings, and what stopped it, if anything.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Section<T> {
    pub items: Vec<T>,
    pub incomplete: Option<Incomplete>,
}

impl<T> Section<T> {
    fn new(items: Vec<T>, incomplete: Option<Incomplete>) -> Self {
        Section { items, incomplete }
    }

    pub fn complete(&self) -> bool {
        self.incomplete.is_none()
    }
}

/// A seed citing a retracted (or withdrawn) work.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RetractedReference {
    /// The seed's index.
    pub seed: usize,
    /// The retracted work.
    pub cites: SourceRecord,
    /// Who says it is retracted: `openalex` (from Retraction Watch), or
    /// the source of a notice already cached (`crossref`,
    /// `retraction-watch`, `publisher`).
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OddReason {
    /// It cites no other seed, none cites it, and it shares no
    /// references with any.
    NoLinks,
    /// Its references are unknown and no other seed cites it, so whether
    /// it belongs can't be told.
    NoReferenceData,
}

impl OddReason {
    every_variant!(OddReason: NoLinks, NoReferenceData);

    pub fn as_str(self) -> &'static str {
        match self {
            OddReason::NoLinks => "no_links",
            OddReason::NoReferenceData => "no_reference_data",
        }
    }
}

/// A seed linked to no other seed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct OddOne {
    pub seed: usize,
    pub reason: OddReason,
}

impl OddOne {
    pub fn new(seed: usize, reason: OddReason) -> Self {
        OddOne { seed, reason }
    }
}

/// A work several seeds cite that the set lacks.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Missing {
    /// Its OpenAlex id (the most cited, when versions merged).
    pub id: WorkId,
    /// Every OpenAlex id merged into it (preprint and published versions).
    pub ids: Vec<WorkId>,
    /// Its record, when looked up.
    pub record: Option<SourceRecord>,
    /// The seeds citing it, one per work of the set (the first seed of
    /// merged ones), in order: as many as cite it.
    pub cited_by: Vec<usize>,
}

impl Missing {
    pub fn count(&self) -> usize {
        self.cited_by.len()
    }
}

/// What an analysis would cost, from what is known before running it.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Estimate {
    /// Paid requests.
    pub requests: u64,
    /// At [`LIST_PRICE`] each.
    pub usd: f64,
    /// Whether the counts are known: every seed resolved or settled (else
    /// a floor), and no related works asked for (whose coupling pages and
    /// lookups are a ceiling; their citing works' pages are exact).
    pub exact: bool,
}

/// Analyses of a set of works.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Analysis {
    /// One per input, in order.
    pub seeds: Vec<Seed>,
    /// Resolving the seeds stopped early (the budget ran out).
    pub incomplete: Option<Incomplete>,
    /// Works of the set (seeds merged) with known references: what "cited
    /// by k of n" counts out of.
    pub with_references: usize,
    pub retracted_references: Option<Section<RetractedReference>>,
    pub odd_one_out: Option<Section<OddOne>>,
    pub probably_missing: Option<Section<Missing>>,
    pub related: Option<Section<RelatedWork>>,
    /// Kinds asked for that aren't built yet (none, now).
    pub not_yet: Vec<AnalysisKind>,
    /// With `estimate_only`: the cost of running it, in place of the
    /// analyses.
    pub estimate: Option<Estimate>,
    pub usage: Vec<(ProviderId, Usage)>,
}

impl Analysis {
    /// Every seed resolved or settled (found or not), and every analysis
    /// run to the end.
    pub fn complete(&self) -> bool {
        let seeds = self.seeds.iter().all(|s| {
            !matches!(
                s.problem,
                Some(SeedProblem::ProviderFailed | SeedProblem::OverBudget)
            )
        });
        seeds
            && self.incomplete.is_none()
            && self
                .retracted_references
                .as_ref()
                .is_none_or(Section::complete)
            && self.odd_one_out.as_ref().is_none_or(Section::complete)
            && self.probably_missing.as_ref().is_none_or(Section::complete)
            && self.related.as_ref().is_none_or(Section::complete)
    }
}

/// The OpenAlex work id among a record's identifiers.
fn openalex_id(record: &SourceRecord) -> Option<&WorkId> {
    record.ids.iter().find(|id| id.kind() == IdKind::OpenAlex)
}

/// What an analysis has spent and may spend.
struct Spend {
    budget: Option<f64>,
    spent: f64,
    /// Paid requests counted, when estimating.
    requests: u64,
    estimating: bool,
    /// Some of `requests` are a ceiling, not an exact count.
    ceiling: bool,
    usage: Vec<(ProviderId, Usage)>,
}

impl Spend {
    /// Whether `calls` more paid calls fit the budget.
    fn allows(&self, calls: usize) -> bool {
        // A hair over, so rounding doesn't refuse the last call that fits.
        self.budget
            .is_none_or(|b| self.spent + calls as f64 * LIST_PRICE <= b + LIST_PRICE * 1e-6)
    }

    /// How many more paid calls fit the budget.
    fn affordable(&self) -> usize {
        match self.budget {
            None => usize::MAX,
            Some(b) => ((b - self.spent) / LIST_PRICE + 1e-6).max(0.0).floor() as usize,
        }
    }

    /// Adds a step's usage; `calls` paid calls were made, and count at
    /// list price if the provider reported no cost.
    fn add(&mut self, usage: Vec<(ProviderId, Usage)>, calls: usize) {
        let reported: Option<f64> = usage
            .iter()
            .filter_map(|(_, u)| u.cost)
            .reduce(|a, b| a + b);
        self.spent += reported.unwrap_or(calls as f64 * LIST_PRICE);
        for (provider, u) in usage {
            add_usage(&mut self.usage, &provider, u);
        }
    }
}

/// Seeds counted as one work: merged seeds, their OpenAlex ids and their
/// references (the union of the known ones).
struct Group {
    members: Vec<usize>,
    works: BTreeSet<WorkId>,
    references: Option<BTreeSet<WorkId>>,
}

impl Group {
    fn cites(&self, other: &Group) -> bool {
        self.references
            .as_ref()
            .is_some_and(|r| other.works.iter().any(|w| r.contains(w)))
    }

    fn shares_references(&self, other: &Group) -> bool {
        match (&self.references, &other.references) {
            (Some(a), Some(b)) => !a.is_disjoint(b),
            _ => false,
        }
    }
}

impl Engine {
    fn openalex_entry(&self) -> Result<&Entry, EngineError> {
        let found = self
            .entries
            .iter()
            .find(|e| e.provider.id().as_str() == OPENALEX);
        found.ok_or_else(
            || match self.disabled.iter().find(|p| p.as_str() == OPENALEX) {
                Some(off) => EngineError::Disabled(off.clone()),
                None => EngineError::NoCapableProvider("citation links"),
            },
        )
    }

    /// A page of the works `id` cites: `limit` from `cursor` (from a
    /// previous page's `next_cursor`), each looked up, in batches of up to
    /// 100. `id` is any identifier aspero looks up; it's resolved to an
    /// OpenAlex work as a search's relations are.
    pub async fn references(
        &self,
        id: &str,
        cursor: Option<&Cursor>,
        limit: u32,
    ) -> Result<ReferencesPage, EngineError> {
        let entry = self.openalex_entry()?;
        let mut usage = Vec::new();
        let start = match cursor {
            None => 0,
            Some(c) => c.0.parse::<usize>().map_err(|_| {
                EngineError::InvalidQuery(format!("not a references cursor: {:?}", c.0))
            })?,
        };

        let given: Option<WorkId> = id.parse().ok();
        let cached = given
            .as_ref()
            .filter(|w| w.kind() == IdKind::OpenAlex)
            .and_then(|w| Some((w.clone(), self.cached_references(w)?)));
        let (work, record, references) = match cached {
            Some((work, references)) => (work, None, Some(references)),
            None => {
                let w = self.openalex_work(id, &mut usage).await?;
                let work: WorkId =
                    WorkId::OpenAlex(w.parse().map_err(|e: aspero_core::IdError| {
                        EngineError::Unresolved {
                            id: id.to_owned(),
                            reason: e.to_string(),
                            kind: aspero_core::ErrorKind::Decode,
                        }
                    })?);
                let (attempt, record) = self
                    .lookup_at(
                        entry,
                        std::slice::from_ref(&work),
                        FieldSet::DEFAULT,
                        &mut usage,
                    )
                    .await
                    .pop()
                    .expect("one answer per id");
                let Some(record) = record else {
                    let (reason, kind) = match attempt.outcome {
                        Outcome::Failed(e) => (format!("the lookup failed: {e}"), e.kind()),
                        _ => (
                            "OpenAlex has no record of it".into(),
                            aspero_core::ErrorKind::NotFound,
                        ),
                    };
                    return Err(EngineError::Unresolved {
                        id: id.to_owned(),
                        reason,
                        kind,
                    });
                };
                let references = record.references.clone();
                if let Some(cited) = &references {
                    self.remember_references(&[(&work, cited)]);
                }
                (work, Some(record), references)
            }
        };

        let total = references.as_ref().map(Vec::len);
        let coverage = Coverage::of(references.as_deref());
        let page: Vec<WorkId> = references
            .iter()
            .flatten()
            .skip(start)
            .take(limit.max(1) as usize)
            .cloned()
            .collect();
        let answers = match page.is_empty() {
            true => Vec::new(),
            false => {
                self.lookup_at(entry, &page, FieldSet::DEFAULT, &mut usage)
                    .await
            }
        };
        let end = start + page.len();
        let next_cursor = total
            .filter(|t| end < *t && !page.is_empty())
            .map(|_| Cursor(end.to_string()));
        let references = page
            .into_iter()
            .zip(answers)
            .map(|(id, (attempt, record))| LookupResult {
                id,
                record,
                attempts: vec![attempt],
            })
            .collect();
        Ok(ReferencesPage {
            work,
            record,
            coverage,
            total,
            references,
            next_cursor,
            usage,
        })
    }

    /// Analyses of the set of works `seeds` names (identifiers as written).
    ///
    /// Seeds resolve to OpenAlex works cheaply: through the cache, then in
    /// batches, each asked once (no retries); those that don't resolve say
    /// why. Seeds that are one work (shared DOI, PMID or arXiv ID; else
    /// the same title and first author, years at most one apart) count
    /// once. The analyses then run cheapest first (odd one out, probably
    /// missing, retracted references, related works) within `budget` (US
    /// dollars), each saying what stopped it, if anything.
    ///
    /// With `estimate_only`, nothing paid is asked: seeds resolve from the
    /// cache and free lookups of one id each (which the run then finds
    /// cached), and the result says what the analyses would cost.
    pub async fn analyse(
        &self,
        seeds: &[String],
        kinds: &[AnalysisKind],
        budget: Option<f64>,
        estimate_only: bool,
    ) -> Result<Analysis, EngineError> {
        let entry = self.openalex_entry()?;
        let mut spend = Spend {
            budget,
            spent: 0.0,
            requests: 0,
            estimating: estimate_only,
            ceiling: false,
            usage: Vec::new(),
        };
        let (mut seeds, incomplete) = self.resolve_seeds(entry, seeds, &mut spend).await;
        let groups = merge_seeds(&mut seeds);
        let links: Vec<(&WorkId, &[WorkId])> = seeds
            .iter()
            .filter_map(|s| Some((s.work()?, s.references()?)))
            .collect();
        self.remember_references(&links);
        let with_references = groups.iter().filter(|g| g.references.is_some()).count();

        let wants = |f: fn(&AnalysisKind) -> bool| kinds.iter().any(f);
        let mut analysis = Analysis {
            seeds,
            incomplete,
            with_references,
            retracted_references: None,
            odd_one_out: None,
            probably_missing: None,
            related: None,
            not_yet: Vec::new(),
            estimate: None,
            usage: Vec::new(),
        };
        if wants(|k| matches!(k, AnalysisKind::OddOneOut)) && !estimate_only {
            analysis.odd_one_out = Some(odd_one_out(&groups));
        }
        let limit = kinds.iter().find_map(|k| match k {
            AnalysisKind::ProbablyMissing { limit } => Some(*limit),
            _ => None,
        });
        if let Some(limit) = limit {
            let found = self
                .probably_missing(entry, &analysis.seeds, &groups, limit, &mut spend)
                .await;
            analysis.probably_missing = (!estimate_only).then_some(found);
        }
        if wants(|k| matches!(k, AnalysisKind::RetractedReferences)) {
            let found = self.retracted_references(&analysis.seeds, &mut spend).await;
            analysis.retracted_references = (!estimate_only).then_some(found);
        }
        let limit = kinds.iter().find_map(|k| match k {
            AnalysisKind::Related { limit } => Some(*limit),
            _ => None,
        });
        if let Some(limit) = limit {
            let found = self
                .related(entry, &analysis.seeds, &groups, limit, &mut spend)
                .await;
            analysis.related = (!estimate_only).then_some(found);
        }
        if estimate_only {
            let settled = analysis.seeds.iter().all(|s| {
                !matches!(
                    s.problem,
                    Some(SeedProblem::ProviderFailed | SeedProblem::OverBudget)
                )
            });
            analysis.estimate = Some(Estimate {
                requests: spend.requests,
                usd: spend.requests as f64 * LIST_PRICE,
                exact: settled && !spend.ceiling,
            });
        }
        analysis.usage = spend.usage;
        Ok(analysis)
    }

    /// Each seed's OpenAlex record, or why there's none; and whether
    /// resolving stopped early.
    async fn resolve_seeds(
        &self,
        entry: &Entry,
        inputs: &[String],
        spend: &mut Spend,
    ) -> (Vec<Seed>, Option<Incomplete>) {
        // Many seeds are expected not to resolve: ask once, never retry.
        let quick = self.with_options(CallOptions {
            max_retries: Some(0),
            ..self.options
        });
        let accepts = &entry.provider.capabilities().lookup_kinds;
        let mut seeds: Vec<Option<Seed>> = vec![None; inputs.len()];
        // (seed, identifier OpenAlex looks up)
        let mut asks: Vec<(usize, WorkId)> = Vec::new();
        let mut elsewhere: Vec<(usize, WorkId)> = Vec::new();
        for (i, input) in inputs.iter().enumerate() {
            match input.parse::<WorkId>() {
                Err(e) => {
                    seeds[i] = Some(Seed::unresolved(
                        input,
                        SeedProblem::InvalidId,
                        e.to_string(),
                    ))
                }
                Ok(id) if accepts.contains(&id.kind()) => asks.push((i, id)),
                Ok(id) => elsewhere.push((i, id)),
            }
        }

        // Identifiers OpenAlex doesn't look up (PMCIDs, ISBNs): found
        // elsewhere (free), then asked about by the DOI or PMID found.
        if !elsewhere.is_empty() {
            let ids: Vec<WorkId> = elsewhere.iter().map(|(_, id)| id.clone()).collect();
            let report = quick.lookup(&ids, FieldSet::DEFAULT).await;
            spend.add(report.usage, 0);
            for ((i, id), result) in elsewhere.into_iter().zip(report.results) {
                let input = &inputs[i];
                let found = result.record.as_ref().and_then(|r| {
                    r.ids
                        .iter()
                        .find(|x| x.kind() == IdKind::OpenAlex)
                        .or_else(|| r.ids.iter().find(|x| accepts.contains(&x.kind())))
                });
                match (found, seed_failure(&result)) {
                    (Some(other), _) => asks.push((i, other.clone())),
                    (None, Some((problem, message))) => {
                        seeds[i] = Some(Seed::unresolved(input, problem, message))
                    }
                    (None, None) => {
                        seeds[i] = Some(Seed::unresolved(
                            input,
                            SeedProblem::UnsupportedId,
                            format!(
                                "OpenAlex doesn't look up {} identifiers, and the record found \
                                 has no DOI or PMID it does",
                                id.kind()
                            ),
                        ))
                    }
                }
            }
        }

        let mut stopped = None;
        let mut answers = self
            .ask_openalex(&quick, entry, &asks, spend, &mut stopped)
            .await;

        // Seeds OpenAlex has no record of by the identifier given are
        // looked up as `lookup` looks them up (Crossref, DataCite, doi.org,
        // …, through the cache; free, asked once), then asked about again
        // by an identifier the record found carries: an OpenAlex id, or
        // another DOI or PMID.
        let missed: Vec<usize> = (0..asks.len())
            .filter(|&k| matches!(answers[k], Some((Outcome::NotFound, None))))
            .collect();
        if !missed.is_empty() {
            let ids: Vec<WorkId> = missed.iter().map(|&k| asks[k].1.clone()).collect();
            let provider = entry.provider.id();
            let report = quick
                .lookup_skipping(&ids, FieldSet::DEFAULT, Some(&provider))
                .await;
            spend.add(report.usage, 0);
            let mut again: Vec<(usize, WorkId)> = Vec::new();
            for (&k, result) in missed.iter().zip(&report.results) {
                let Some(record) = &result.record else {
                    continue;
                };
                let asked = &asks[k].1;
                let other = record
                    .ids
                    .iter()
                    .find(|x| x.kind() == IdKind::OpenAlex)
                    .or_else(|| {
                        record
                            .ids
                            .iter()
                            .find(|x| accepts.contains(&x.kind()) && !same_work_id(x, asked))
                    });
                if let Some(other) = other {
                    again.push((k, other.clone()));
                }
            }
            if !again.is_empty() && stopped.is_none() {
                let found = self
                    .ask_openalex(&quick, entry, &again, spend, &mut stopped)
                    .await;
                for ((k, _), answer) in again.iter().zip(found) {
                    if let Some((outcome, Some(record))) = answer {
                        answers[*k] = Some((outcome, Some(record)));
                    }
                }
            }
        }

        for ((i, id), answer) in asks.iter().zip(answers) {
            let input = &inputs[*i];
            seeds[*i] = Some(match answer {
                Some((_, Some(record))) => Seed {
                    input: input.clone(),
                    record: Some(record),
                    problem: None,
                    message: None,
                    same_as: None,
                },
                Some((Outcome::Failed(e), None)) => Seed::unresolved(
                    input,
                    SeedProblem::ProviderFailed,
                    format!("the lookup failed: {e}"),
                ),
                Some(_) => Seed::unresolved(
                    input,
                    SeedProblem::NotFound,
                    format!("OpenAlex has no record of {id}"),
                ),
                None => Seed::unresolved(
                    input,
                    SeedProblem::OverBudget,
                    "not looked up: the budget ran out",
                ),
            });
        }
        let seeds = seeds
            .into_iter()
            .map(|s| s.expect("every seed resolved or settled"))
            .collect();
        (seeds, stopped)
    }

    /// OpenAlex's answer for each of `asks` (seed, identifier); `None`
    /// where the budget stopped it, saying so in `stopped`. Fresh cached
    /// answers first, then the rest in batches of one kind each (a batch of
    /// one is a free lookup), asked once. Estimating, the rest are asked
    /// one at a time (free), and the batches a run would pay for are
    /// counted when there's no cache for the run to find them in.
    async fn ask_openalex(
        &self,
        quick: &Engine,
        entry: &Entry,
        asks: &[(usize, WorkId)],
        spend: &mut Spend,
        stopped: &mut Option<Incomplete>,
    ) -> Vec<Option<(Outcome, Option<SourceRecord>)>> {
        let provider = entry.provider.id();
        let ids: Vec<WorkId> = asks.iter().map(|(_, id)| id.clone()).collect();
        let cached = self.cached_lookups(&provider, &ids, FieldSet::DEFAULT);
        let mut answers: Vec<Option<(Outcome, Option<SourceRecord>)>> = cached
            .into_iter()
            .map(|c| {
                c.map(|record| match record {
                    Some(r) => (Outcome::Found, Some(r)),
                    None => (Outcome::NotFound, None),
                })
            })
            .collect();
        let pending: Vec<usize> = (0..asks.len()).filter(|&k| answers[k].is_none()).collect();
        let pending_ids: Vec<WorkId> = pending.iter().map(|&k| asks[k].1.clone()).collect();
        let chunks = batches(&pending_ids, &pending, entry);
        if spend.estimating {
            // Without a cache, or past it, the run won't find them there.
            if self.cache.is_none() || !self.options.reads_cache() {
                spend.requests += chunks.iter().filter(|c| c.len() > 1).count() as u64;
            }
            let lookups = futures::stream::iter(pending_ids.into_iter().zip(pending).map(
                |(id, k)| async move {
                    let mut usage = Vec::new();
                    let answer = quick
                        .lookup_at(
                            entry,
                            std::slice::from_ref(&id),
                            FieldSet::DEFAULT,
                            &mut usage,
                        )
                        .await
                        .pop()
                        .expect("one answer per id");
                    (k, answer, usage)
                },
            ))
            .buffered(ESTIMATE_LOOKUPS)
            .collect::<Vec<_>>()
            .await;
            for (k, (attempt, record), usage) in lookups {
                spend.add(usage, 0);
                answers[k] = Some((attempt.outcome, record));
            }
            return answers;
        }
        let mut done = asks.len() - pending.len();
        for chunk in chunks {
            let paid = usize::from(chunk.len() > 1);
            if !spend.allows(paid) {
                *stopped = Some(Incomplete::new(IncompleteKind::Budget, done, asks.len()));
                break;
            }
            let batch: Vec<WorkId> = chunk.iter().map(|&k| asks[k].1.clone()).collect();
            let mut usage = Vec::new();
            let found = quick
                .lookup_at(entry, &batch, FieldSet::DEFAULT, &mut usage)
                .await;
            spend.add(usage, paid);
            for (&k, (attempt, record)) in chunk.iter().zip(found) {
                answers[k] = Some((attempt.outcome, record));
            }
            done += chunk.len();
        }
        answers
    }

    /// The works cited by at least two works of the set and not in it,
    /// most cited first (ties by id), the best `limit` looked up and
    /// merged where they are one work (or a seed).
    async fn probably_missing(
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
                let failed = answers
                    .iter()
                    .filter(|(a, _)| matches!(a.outcome, Outcome::Failed(_)))
                    .count();
                if failed > 0 {
                    incomplete = Some(Incomplete::new(
                        IncompleteKind::ProviderFailed,
                        fetch - failed,
                        fetch,
                    ));
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

    /// Which seed cites which retracted work: the seeds' references checked
    /// at OpenAlex 100 at a time, and against notices already cached.
    async fn retracted_references(
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
        let (mut done, mut failed) = (0, false);
        let mut incomplete = None;
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
                incomplete = Some(Incomplete::new(IncompleteKind::Budget, done, cited.len()));
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
                Err(e) => {
                    tracing::warn!(target: "aspero::graph", error = %e, "retraction check failed");
                    failed = true;
                }
            }
        }
        if failed && incomplete.is_none() {
            incomplete = Some(Incomplete::new(
                IncompleteKind::ProviderFailed,
                done,
                cited.len(),
            ));
        }
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
        let registry = self.registry().map(|e| e.provider.id());
        for (id, record) in unflagged
            .iter()
            .zip(self.stale_lookups(&provider, &unflagged))
        {
            let Some(record) = record else { continue };
            let notice = match record.work.retraction() {
                Some(u) => Some(notice_source(u, OPENALEX)),
                None => registry.as_ref().and_then(|registry| {
                    let dois: Vec<WorkId> = record
                        .ids
                        .iter()
                        .filter(|i| i.kind() == IdKind::Doi)
                        .cloned()
                        .collect();
                    self.stale_lookups(registry, &dois)
                        .into_iter()
                        .flatten()
                        .find_map(|r| {
                            r.work
                                .retraction()
                                .map(|u| notice_source(u, registry.as_str()))
                        })
                }),
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

    /// A work's references cached within their time to live (none, past
    /// the cache).
    fn cached_references(&self, work: &WorkId) -> Option<Vec<WorkId>> {
        let entry = self
            .cache
            .as_ref()?
            .get_edges(Direction::References, std::slice::from_ref(work))
            .ok()?
            .pop()??;
        self.current(&entry, self.cache_policy.references)
            .then_some(entry.value.ids)
    }

    fn remember_references(&self, links: &[(&WorkId, &[WorkId])]) {
        if let Some(cache) = &self.cache {
            let edges: Vec<Edges> = links
                .iter()
                .map(|(_, ids)| Edges::new(ids.to_vec(), true))
                .collect();
            let rows: Vec<(&WorkId, &Edges)> = links.iter().map(|(w, _)| *w).zip(&edges).collect();
            let _ = cache.put_edges(Direction::References, &rows, (self.clock)());
        }
    }
}

/// What makes a work one of the set: sharing any identifier with any seed,
/// resolved or not (its input, or any id its record carries), or being one
/// work with a seed's record (a preprint and its published version).
struct TheSet<'a> {
    ids: Vec<WorkId>,
    records: Vec<&'a SourceRecord>,
}

impl<'a> TheSet<'a> {
    fn new(seeds: &'a [Seed]) -> Self {
        let ids = seeds
            .iter()
            .flat_map(|s| {
                let given = s.input.parse::<WorkId>().ok();
                let carried = s.record.iter().flat_map(|r| r.ids.iter().cloned());
                given.into_iter().chain(carried)
            })
            .collect();
        let records = seeds.iter().filter_map(|s| s.record.as_ref()).collect();
        TheSet { ids, records }
    }

    /// Whether the work `id` (with its record, when looked up) is one of
    /// the set.
    fn has(&self, id: &WorkId, record: Option<&SourceRecord>) -> bool {
        let is_seed = |id: &WorkId| self.ids.iter().any(|s| same_work_id(s, id));
        is_seed(id)
            || record.is_some_and(|r| {
                r.ids.iter().any(is_seed) || self.records.iter().any(|s| same_work(s, r))
            })
    }
}

/// A lookup's failure as a seed's problem; `None` when it found a record.
fn seed_failure(result: &LookupResult) -> Option<(SeedProblem, String)> {
    if result.record.is_some() {
        return None;
    }
    if let Some(reason) = result.unsupported() {
        return Some((SeedProblem::UnsupportedId, reason));
    }
    if result.is_not_found() {
        return Some((SeedProblem::NotFound, "no such work found".into()));
    }
    let failed = result.attempts.iter().find_map(|a| match &a.outcome {
        Outcome::Failed(e) => Some(e),
        _ => None,
    });
    Some(match failed {
        Some(e) => (
            SeedProblem::ProviderFailed,
            format!("the lookup failed: {e}"),
        ),
        None => (SeedProblem::NotFound, "no such work found".into()),
    })
}

/// Who reported a retraction: the notice's source, or `fallback`.
fn notice_source(update: &Update, fallback: &str) -> String {
    update.source.clone().unwrap_or_else(|| fallback.to_owned())
}

/// The search finding which of `ids` OpenAlex flags as retracted.
fn retracted_query(ids: &[&WorkId]) -> SearchQuery {
    let values: Vec<String> = ids
        .iter()
        .map(|id| match id {
            WorkId::OpenAlex(w) => w.to_string(),
            other => other.to_string(),
        })
        .collect();
    let mut query = SearchQuery::new("");
    query.page.limit = MAX_FILTER as u32;
    query.extra.insert(
        "filter".into(),
        format!("openalex:{},is_retracted:true", values.join("|")),
    );
    query
}

/// `pending` (indices into the asks, with `ids` their identifiers) in
/// batches as OpenAlex looks them up: one kind per batch (a filter per
/// kind; arXiv IDs are DOIs there), at most a batch's worth each.
fn batches(ids: &[WorkId], pending: &[usize], entry: &Entry) -> Vec<Vec<usize>> {
    let max = entry.provider.capabilities().max_batch.clamp(1, MAX_FILTER);
    let kind = |id: &WorkId| match id.kind() {
        IdKind::Arxiv => IdKind::Doi,
        k => k,
    };
    let mut by_kind: Vec<(IdKind, Vec<usize>)> = Vec::new();
    for (id, &k) in ids.iter().zip(pending) {
        match by_kind.iter_mut().find(|(kd, _)| *kd == kind(id)) {
            Some((_, group)) => group.push(k),
            None => by_kind.push((kind(id), vec![k])),
        }
    }
    by_kind
        .into_iter()
        .flat_map(|(_, group)| group.chunks(max).map(<[usize]>::to_vec).collect::<Vec<_>>())
        .collect()
}

/// Marks seeds that are one work (`same_as` the first of them) and groups
/// the resolved ones, in order of their first seed.
fn merge_seeds(seeds: &mut [Seed]) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for i in 0..seeds.len() {
        let Some(record) = &seeds[i].record else {
            continue;
        };
        let same = groups.iter().position(|g| {
            g.members.iter().any(|&m| {
                let other = seeds[m]
                    .record
                    .as_ref()
                    .expect("grouped seeds are resolved");
                openalex_id(other) == openalex_id(record) || same_work(other, record)
            })
        });
        let work = openalex_id(record).cloned();
        let cited: Option<BTreeSet<WorkId>> = record
            .references
            .as_ref()
            .map(|r| r.iter().cloned().collect());
        match same {
            Some(g) => {
                let group = &mut groups[g];
                seeds[i].same_as = Some(group.members[0]);
                group.members.push(i);
                group.works.extend(work);
                if let Some(cited) = cited {
                    group.references.get_or_insert_default().extend(cited);
                }
            }
            None => groups.push(Group {
                members: vec![i],
                works: work.into_iter().collect(),
                references: cited,
            }),
        }
    }
    groups
}

/// Seeds linked to no other: coverage-aware, so unknown references are
/// `no_reference_data` unless another seed cites the work. Nothing is odd
/// in a set of fewer than two works.
fn odd_one_out(groups: &[Group]) -> Section<OddOne> {
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

    #[test]
    fn batches_keep_to_one_kind_and_a_hundred() {
        let mut ids: Vec<WorkId> = (1..=150)
            .map(|n| format!("10.1234/{n}").parse().unwrap())
            .collect();
        ids.push("pmid:1".parse().unwrap());
        ids.push("arXiv:1706.03762".parse().unwrap());
        let pending: Vec<usize> = (0..ids.len()).collect();
        let entry = crate::Entry {
            provider: std::sync::Arc::new(Stub(caps_with(100))),
            policy: aspero_core::ResiliencePolicy::default(),
        };
        let sizes: Vec<usize> = batches(&ids, &pending, &entry)
            .iter()
            .map(Vec::len)
            .collect();
        // DOIs and the arXiv ID (a DOI there) together, 100 at a time;
        // then the PMID.
        assert_eq!(sizes, [100, 51, 1]);
    }

    fn caps_with(max_batch: usize) -> aspero_core::Capabilities {
        let mut caps = aspero_core::Capabilities::default();
        caps.max_batch = max_batch;
        caps
    }

    struct Stub(aspero_core::Capabilities);

    #[async_trait::async_trait]
    impl aspero_core::Provider for Stub {
        fn id(&self) -> ProviderId {
            ProviderId::from_static(OPENALEX)
        }
        fn capabilities(&self) -> &aspero_core::Capabilities {
            &self.0
        }
    }

    #[test]
    fn analysis_kinds_and_reasons_have_names() {
        let names: Vec<&str> = AnalysisKind::ALL.iter().map(|k| k.as_str()).collect();
        assert_eq!(
            names,
            [
                "retracted_references",
                "odd_one_out",
                "probably_missing",
                "related"
            ]
        );
    }
}
