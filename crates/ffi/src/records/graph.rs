//! Citation links: a work's references, and analyses of a set of works
//! (retracted references, odd ones out, probably missing, related).

use aspero_engine as engine;

use super::{Failure, Lookup, ProviderUsage, Work, count, usage};

/// Whether a work's references are known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Coverage {
    /// OpenAlex lists the work's references.
    Known,
    /// OpenAlex has no reference data for it (books, older work): it may
    /// cite anything.
    Unknown,
}

impl From<engine::Coverage> for Coverage {
    // The wildcard is for variants the non-exhaustive enum adds later,
    // not the named one it happens to map the same as.
    #[allow(clippy::match_same_arms)]
    fn from(c: engine::Coverage) -> Self {
        match c {
            engine::Coverage::Known => Coverage::Known,
            engine::Coverage::Unknown => Coverage::Unknown,
            _ => Coverage::Unknown,
        }
    }
}

/// A page of a work's references.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ReferencesPage {
    /// As given.
    pub input: String,
    /// The work's OpenAlex id (`openalex:W…`).
    pub id: Option<String>,
    /// The work, when it was looked up (not when its references were
    /// cached).
    pub work: Option<Work>,
    pub coverage: Coverage,
    /// How many works it cites; none when unknown.
    pub total: Option<u64>,
    /// This page, in OpenAlex's order (by id), each looked up.
    pub references: Vec<Lookup>,
    /// The cursor for the next page; none on the last.
    pub next_cursor: Option<String>,
    /// Why there's no page: the work couldn't be resolved.
    pub failure: Option<Failure>,
    pub usage: Vec<ProviderUsage>,
}

impl ReferencesPage {
    pub(crate) fn new(
        input: &str,
        result: Result<engine::ReferencesPage, engine::EngineError>,
    ) -> Self {
        match result {
            Err(e) => ReferencesPage {
                input: input.to_owned(),
                id: None,
                work: None,
                coverage: Coverage::Unknown,
                total: None,
                references: Vec::new(),
                next_cursor: None,
                failure: Some(Failure::from(&e)),
                usage: Vec::new(),
            },
            Ok(page) => ReferencesPage {
                input: input.to_owned(),
                id: Some(page.work.to_string()),
                work: page.record.as_ref().map(Work::from_record),
                coverage: page.coverage.into(),
                total: page.total.map(|t| t as u64),
                references: page
                    .references
                    .iter()
                    .map(|r| Lookup::from_result(&r.id.to_string(), r))
                    .collect(),
                next_cursor: page.next_cursor.map(|c| c.0),
                failure: None,
                usage: usage(&page.usage),
            },
        }
    }
}

/// An analysis of a set of works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AnalysisKind {
    /// Which seed cites which retracted or withdrawn work.
    RetractedReferences,
    /// Seeds linked to no other seed.
    OddOneOut,
    /// The best `limit` works several seeds cite that the set lacks.
    ProbablyMissing { limit: u32 },
    /// The best works around the set, by co-citation and bibliographic
    /// coupling (`related`): as many as `analyse`'s `relatedLimit`.
    Related,
}

impl AnalysisKind {
    /// The engine's kind, `related_limit` the number of related works.
    pub(crate) fn to_engine(self, related_limit: u32) -> engine::AnalysisKind {
        match self {
            AnalysisKind::RetractedReferences => engine::AnalysisKind::RetractedReferences,
            AnalysisKind::OddOneOut => engine::AnalysisKind::OddOneOut,
            AnalysisKind::ProbablyMissing { limit } => {
                engine::AnalysisKind::ProbablyMissing { limit }
            }
            AnalysisKind::Related => engine::AnalysisKind::Related {
                limit: related_limit,
            },
        }
    }
}

impl From<engine::AnalysisKind> for AnalysisKind {
    // The wildcard is for variants the non-exhaustive enum adds later,
    // not the named one it happens to map the same as.
    #[allow(clippy::match_same_arms)]
    fn from(k: engine::AnalysisKind) -> Self {
        match k {
            engine::AnalysisKind::RetractedReferences => AnalysisKind::RetractedReferences,
            engine::AnalysisKind::OddOneOut => AnalysisKind::OddOneOut,
            engine::AnalysisKind::ProbablyMissing { limit } => {
                AnalysisKind::ProbablyMissing { limit }
            }
            engine::AnalysisKind::Related { .. } => AnalysisKind::Related,
            _ => AnalysisKind::Related,
        }
    }
}

/// Why a seed has no OpenAlex work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SeedProblem {
    /// Not an identifier.
    InvalidId,
    /// OpenAlex can't look up this kind of identifier (an ISBN), and no
    /// record found by it has a DOI or PMID it can.
    UnsupportedId,
    NotFound,
    /// The lookup failed (asked once; seeds aren't retried).
    ProviderFailed,
    /// Not looked up: the budget ran out first.
    OverBudget,
}

impl From<engine::SeedProblem> for SeedProblem {
    // The wildcard is for variants the non-exhaustive enum adds later,
    // not the named one it happens to map the same as.
    #[allow(clippy::match_same_arms)]
    fn from(p: engine::SeedProblem) -> Self {
        match p {
            engine::SeedProblem::InvalidId => SeedProblem::InvalidId,
            engine::SeedProblem::UnsupportedId => SeedProblem::UnsupportedId,
            engine::SeedProblem::NotFound => SeedProblem::NotFound,
            engine::SeedProblem::ProviderFailed => SeedProblem::ProviderFailed,
            engine::SeedProblem::OverBudget => SeedProblem::OverBudget,
            _ => SeedProblem::ProviderFailed,
        }
    }
}

/// One work of the set.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Seed {
    /// Its place among the seeds given; findings name seeds by it.
    pub index: u32,
    /// As given.
    pub input: String,
    /// Its OpenAlex id, when resolved.
    pub id: Option<String>,
    pub work: Option<Work>,
    /// For resolved seeds: whether its references are known.
    pub coverage: Option<Coverage>,
    /// How many works it cites, when known.
    pub references: Option<u64>,
    /// Why it isn't resolved.
    pub problem: Option<SeedProblem>,
    /// The problem in English.
    pub message: Option<String>,
    /// The earlier seed that is the same work (a preprint and its
    /// published version): the two count as one.
    pub same_as: Option<u32>,
}

impl Seed {
    fn new(index: usize, s: &engine::Seed) -> Self {
        Seed {
            index: count(index),
            input: s.input.clone(),
            id: s.work().map(ToString::to_string),
            work: s.record.as_ref().map(Work::from_record),
            coverage: s.coverage().map(Into::into),
            references: s.references().map(|r| r.len() as u64),
            problem: s.problem.map(Into::into),
            message: s.message.clone(),
            same_as: s.same_as.map(count),
        }
    }
}

/// What stopped part of an analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum IncompleteKind {
    /// A fixed cap: related works read a sample of a much cited work's
    /// citing works.
    Capped,
    /// The budget ran out.
    Budget,
    ProviderFailed,
}

impl From<engine::IncompleteKind> for IncompleteKind {
    // The wildcard is for variants the non-exhaustive enum adds later,
    // not the named one it happens to map the same as.
    #[allow(clippy::match_same_arms)]
    fn from(k: engine::IncompleteKind) -> Self {
        match k {
            engine::IncompleteKind::Capped => IncompleteKind::Capped,
            engine::IncompleteKind::Budget => IncompleteKind::Budget,
            engine::IncompleteKind::ProviderFailed => IncompleteKind::ProviderFailed,
            _ => IncompleteKind::ProviderFailed,
        }
    }
}

/// A stage of related works, and what its `Incomplete`'s `done` and
/// `total` count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AnalysisStage {
    /// Reading the works citing the set: citing works read, of those the
    /// seeds' citation counts promise.
    Citing,
    /// Reading the works citing the set's most shared references (the
    /// coupling sample): pages read, of those meant to be.
    Coupling,
    /// Looking the best up: works answered, of those asked about.
    Lookup,
}

impl From<engine::Stage> for AnalysisStage {
    // The wildcard is for variants the non-exhaustive enum adds later,
    // not the named one it happens to map the same as.
    #[allow(clippy::match_same_arms)]
    fn from(s: engine::Stage) -> Self {
        match s {
            engine::Stage::Citing => AnalysisStage::Citing,
            engine::Stage::Coupling => AnalysisStage::Coupling,
            engine::Stage::Lookup => AnalysisStage::Lookup,
            _ => AnalysisStage::Lookup,
        }
    }
}

/// How far a stopped step got: `done` of `total` (seeds looked up,
/// references checked, works looked up; for related works, as its `stage`
/// counts), to word as "checked 1,200 of 4,300".
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Incomplete {
    pub kind: IncompleteKind,
    pub done: u64,
    pub total: u64,
    /// Which stage of related works stopped; `nil` for the other
    /// analyses.
    #[uniffi(default = None)]
    pub stage: Option<AnalysisStage>,
    /// The provider that failed, for `providerFailed`.
    #[uniffi(default = None)]
    pub provider: Option<String>,
}

impl From<&engine::Incomplete> for Incomplete {
    fn from(i: &engine::Incomplete) -> Self {
        Incomplete {
            kind: i.kind.into(),
            done: i.done,
            total: i.total,
            stage: i.stage.map(Into::into),
            provider: i.provider.as_ref().map(ToString::to_string),
        }
    }
}

/// A seed citing a retracted or withdrawn work.
#[derive(Debug, Clone, uniffi::Record)]
pub struct RetractedReference {
    pub seed: u32,
    pub cites: Work,
    /// Who says so: `openalex`, or a cached notice's source (`crossref`,
    /// `retraction-watch`, `publisher`).
    pub source: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct RetractedReferences {
    pub items: Vec<RetractedReference>,
    /// What stopped the check: a failure, then the budget; empty when
    /// nothing did.
    pub incomplete: Vec<Incomplete>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum OddReason {
    /// It cites no other seed, none cites it, and it shares no
    /// references with any.
    NoLinks,
    /// Its references are unknown and no other seed cites it, or a seed
    /// whose references are unknown may cite it: whether it belongs can't
    /// be told.
    NoReferenceData,
}

impl From<engine::OddReason> for OddReason {
    // The wildcard is for variants the non-exhaustive enum adds later,
    // not the named one it happens to map the same as.
    #[allow(clippy::match_same_arms)]
    fn from(r: engine::OddReason) -> Self {
        match r {
            engine::OddReason::NoLinks => OddReason::NoLinks,
            engine::OddReason::NoReferenceData => OddReason::NoReferenceData,
            _ => OddReason::NoReferenceData,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct OddOne {
    pub seed: u32,
    pub reason: OddReason,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct OddOnesOut {
    pub items: Vec<OddOne>,
    /// What stopped it; empty when nothing did.
    pub incomplete: Vec<Incomplete>,
}

/// A work several seeds cite that the set lacks.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MissingWork {
    /// Its OpenAlex id.
    pub id: String,
    /// Every OpenAlex id merged into it (preprint and published versions).
    pub ids: Vec<String>,
    /// When looked up.
    pub work: Option<Work>,
    /// The seeds citing it, one per work of the set.
    pub cited_by: Vec<u32>,
    pub count: u32,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ProbablyMissing {
    pub items: Vec<MissingWork>,
    /// What stopped it; empty when nothing did.
    pub incomplete: Vec<Incomplete>,
}

/// A work around the set, and why: co-cited with it, sharing its
/// references, or both.
#[derive(Debug, Clone, uniffi::Record)]
pub struct RelatedWork {
    /// Its OpenAlex id.
    pub id: String,
    /// Every OpenAlex id merged into it (preprint and published versions).
    pub ids: Vec<String>,
    /// When looked up.
    pub work: Option<Work>,
    /// From 0 to 1: its co-citation strength (each citing work counting
    /// 1/√ of its references) and coupling count, each as a share of the
    /// highest among the works found, weighted 0.8 and 0.2.
    pub score: f64,
    /// Citing works read that cite it and at least one seed.
    pub co_cited_with: u32,
    /// The seeds' references it cites too (counted from two).
    pub shared_references: u32,
    /// The seeds linking it, one per work of the set.
    pub seeds: Vec<u32>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct RelatedWorks {
    pub items: Vec<RelatedWork>,
    /// Every stop, in order: stages that failed, then those the budget
    /// stopped, then `capped` when a much cited seed's citing works were
    /// sampled; `done` of `total` as each one's `stage` counts them. Empty
    /// when nothing stopped it.
    pub incomplete: Vec<Incomplete>,
}

/// What an analysis would cost.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct Estimate {
    /// Paid requests.
    pub requests: u64,
    /// What they'd cost, in US dollars.
    pub cost_usd: f64,
    /// Whether the counts are known: otherwise a floor (seeds not
    /// resolved), or with related works a ceiling (their citing works'
    /// pages are exact; coupling's pages and the lookups at most).
    pub exact: bool,
}

/// Analyses of a set of works.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Analysis {
    /// One per seed given, in order.
    pub seeds: Vec<Seed>,
    /// Works of the set (seeds merged) with known references: what "cited
    /// by k of n" counts out of.
    pub with_references: u32,
    pub retracted_references: Option<RetractedReferences>,
    pub odd_one_out: Option<OddOnesOut>,
    pub probably_missing: Option<ProbablyMissing>,
    /// With `estimateOnly`: the cost of running it, in place of findings.
    pub estimate: Option<Estimate>,
    /// Every seed settled and every analysis run to the end.
    pub complete: bool,
    /// What stopped resolving the seeds early; empty when nothing did.
    pub incomplete: Vec<Incomplete>,
    /// Why there's no analysis at all (no OpenAlex).
    pub failure: Option<Failure>,
    pub usage: Vec<ProviderUsage>,
    /// Works around the set, when asked for (`related`).
    #[uniffi(default = None)]
    pub related: Option<RelatedWorks>,
}

impl Analysis {
    pub(crate) fn new(result: Result<engine::Analysis, engine::EngineError>) -> Self {
        let a = match result {
            Ok(a) => a,
            Err(e) => {
                return Analysis {
                    seeds: Vec::new(),
                    with_references: 0,
                    retracted_references: None,
                    odd_one_out: None,
                    probably_missing: None,
                    estimate: None,
                    complete: false,
                    incomplete: Vec::new(),
                    failure: Some(Failure::from(&e)),
                    usage: Vec::new(),
                    related: None,
                };
            }
        };
        let incomplete = |i: &[engine::Incomplete]| i.iter().map(Incomplete::from).collect();
        Analysis {
            seeds: a
                .seeds
                .iter()
                .enumerate()
                .map(|(i, s)| Seed::new(i, s))
                .collect(),
            with_references: count(a.with_references),
            retracted_references: a
                .retracted_references
                .as_ref()
                .map(|s| RetractedReferences {
                    items: s
                        .items
                        .iter()
                        .map(|r| RetractedReference {
                            seed: count(r.seed),
                            cites: Work::from_record(&r.cites),
                            source: r.source.clone(),
                        })
                        .collect(),
                    incomplete: incomplete(&s.incomplete),
                }),
            odd_one_out: a.odd_one_out.as_ref().map(|s| OddOnesOut {
                items: s
                    .items
                    .iter()
                    .map(|o| OddOne {
                        seed: count(o.seed),
                        reason: o.reason.into(),
                    })
                    .collect(),
                incomplete: incomplete(&s.incomplete),
            }),
            probably_missing: a.probably_missing.as_ref().map(|s| ProbablyMissing {
                items: s
                    .items
                    .iter()
                    .map(|m| MissingWork {
                        id: m.id.to_string(),
                        ids: m.ids.iter().map(ToString::to_string).collect(),
                        work: m.record.as_ref().map(Work::from_record),
                        cited_by: m.cited_by.iter().copied().map(count).collect(),
                        count: count(m.count()),
                    })
                    .collect(),
                incomplete: incomplete(&s.incomplete),
            }),
            estimate: a.estimate.map(|e| Estimate {
                requests: e.requests,
                cost_usd: e.cost_usd,
                exact: e.exact,
            }),
            complete: a.complete(),
            incomplete: incomplete(&a.incomplete),
            failure: None,
            usage: usage(&a.usage),
            related: a.related.as_ref().map(|s| RelatedWorks {
                items: s
                    .items
                    .iter()
                    .map(|r| RelatedWork {
                        id: r.id.to_string(),
                        ids: r.ids.iter().map(ToString::to_string).collect(),
                        work: r.record.as_ref().map(Work::from_record),
                        score: r.score,
                        co_cited_with: count(r.co_cited_with),
                        shared_references: count(r.shared_references),
                        seeds: r.seeds.iter().copied().map(count).collect(),
                    })
                    .collect(),
                incomplete: incomplete(&s.incomplete),
            }),
        }
    }
}
