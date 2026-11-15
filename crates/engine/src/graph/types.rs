//! What the analyses say: their kinds, the seeds as resolved, each
//! analysis' findings and what stopped it.

use aspero_core::{Cursor, ProviderId, SourceRecord, Usage, WorkId};

use super::{RelatedWork, openalex_id};
use crate::LookupResult;

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
    pub(super) fn unresolved(
        input: &str,
        problem: SeedProblem,
        message: impl Into<String>,
    ) -> Self {
        Seed {
            input: input.to_owned(),
            record: None,
            problem: Some(problem),
            message: Some(message.into()),
            same_as: None,
        }
    }

    /// Resolved, or settled unresolved (not stopped by a failure or the
    /// budget: asking again won't change the answer).
    pub(super) fn settled(&self) -> bool {
        !matches!(
            self.problem,
            Some(SeedProblem::ProviderFailed | SeedProblem::OverBudget)
        )
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
/// references checked, works looked up, citing works read; for related
/// works, of the `stage` that stopped).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Incomplete {
    pub kind: IncompleteKind,
    pub done: u64,
    pub total: u64,
    /// Which stage stopped, for an analysis of several (related works).
    pub stage: Option<Stage>,
    /// The provider that failed (`ProviderFailed`).
    pub provider: Option<ProviderId>,
}

impl Incomplete {
    pub fn new(kind: IncompleteKind, done: usize, total: usize) -> Self {
        Incomplete {
            kind,
            done: done as u64,
            total: total as u64,
            stage: None,
            provider: None,
        }
    }

    /// The same, stopped at `stage`.
    pub fn at(mut self, stage: Stage) -> Self {
        self.stage = Some(stage);
        self
    }

    /// The same, `provider` having failed.
    pub fn from(mut self, provider: ProviderId) -> Self {
        self.provider = Some(provider);
        self
    }
}

/// A stage of related works, and what its `done` and `total` count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Stage {
    /// Reading the works citing the set: citing works read, of those the
    /// set's `cited_by_count`s promise.
    Citing,
    /// Reading the works citing the set's most shared references (the
    /// coupling sample): pages read, of those meant to be.
    Coupling,
    /// Looking the best up: works answered, of those asked about.
    Lookup,
}

impl Stage {
    every_variant!(Stage: Citing, Coupling, Lookup);

    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Citing => "citing",
            Stage::Coupling => "coupling",
            Stage::Lookup => "lookup",
        }
    }
}

/// One analysis' findings, and what stopped it, if anything.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Section<T> {
    pub items: Vec<T>,
    /// Every stop, once each: failures first, then the budget, then a
    /// cap; for related works, each in stage order. Empty when complete.
    pub incomplete: Vec<Incomplete>,
}

impl<T> Section<T> {
    /// `items`, stopped by each of `incomplete` (repeats dropped).
    pub(super) fn new(items: Vec<T>, incomplete: Vec<Incomplete>) -> Self {
        let mut once: Vec<Incomplete> = Vec::with_capacity(incomplete.len());
        for i in incomplete {
            if !once.contains(&i) {
                once.push(i);
            }
        }
        Section {
            items,
            incomplete: once,
        }
    }

    pub fn complete(&self) -> bool {
        self.incomplete.is_empty()
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
    /// Its references are unknown and no other seed cites it, or another
    /// seed whose references are unknown may cite it: whether it belongs
    /// can't be told.
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
    /// In US dollars, at [`LIST_PRICE`](super::LIST_PRICE) each.
    pub cost_usd: f64,
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
    /// What stopped resolving the seeds early (the budget running out);
    /// empty when nothing did.
    pub incomplete: Vec<Incomplete>,
    /// Works of the set (seeds merged) with known references: what "cited
    /// by k of n" counts out of.
    pub with_references: usize,
    pub retracted_references: Option<Section<RetractedReference>>,
    pub odd_one_out: Option<Section<OddOne>>,
    pub probably_missing: Option<Section<Missing>>,
    pub related: Option<Section<RelatedWork>>,
    /// With `estimate_only`: the cost of running it, in place of the
    /// analyses.
    pub estimate: Option<Estimate>,
    pub usage: Vec<(ProviderId, Usage)>,
}

impl Analysis {
    /// Every seed resolved or settled (found or not), and every analysis
    /// run to the end.
    pub fn complete(&self) -> bool {
        self.seeds.iter().all(Seed::settled)
            && self.incomplete.is_empty()
            && self
                .retracted_references
                .as_ref()
                .is_none_or(Section::complete)
            && self.odd_one_out.as_ref().is_none_or(Section::complete)
            && self.probably_missing.as_ref().is_none_or(Section::complete)
            && self.related.as_ref().is_none_or(Section::complete)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
