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
//!
//! The analyses' parts: [`types`] (what they say), [`seeds`] (resolving
//! and merging the set), [`spend`] (the budget, and what counts as the
//! set), [`missing`] (probably missing, odd one out), [`retracted`],
//! [`references`] (a work's references, and their cache) and [`related`].

use aspero_core::{IdKind, SourceRecord, WorkId};

use crate::{Engine, EngineError, Entry};

mod missing;
mod references;
mod related;
mod retracted;
mod seeds;
mod spend;
mod types;

pub use related::RelatedWork;
pub use types::{
    Analysis, AnalysisKind, Coverage, Estimate, Incomplete, IncompleteKind, Missing, OddOne,
    OddReason, ReferencesPage, RetractedReference, Section, Seed, SeedProblem, Stage,
};

use missing::odd_one_out;
use seeds::merge_seeds;
use spend::Spend;

/// The provider citation links come from.
const OPENALEX: &str = "openalex";

/// What OpenAlex charges for one list or filter call, in US dollars
/// (help.openalex.org, 2026-11). Lookups of one id are free.
pub const LIST_PRICE: f64 = 0.0001;

/// Most values OpenAlex ORs in one filter, and most works per page.
const MAX_FILTER: usize = 100;

/// Seeds that must cite a work for it to be probably missing.
const MIN_CITING: usize = 2;

/// Works ORed in an OpenAlex filter: `W1|W2|…` (OpenAlex ids bare).
fn any_of<'a>(works: impl IntoIterator<Item = &'a WorkId>) -> String {
    let values: Vec<String> = works
        .into_iter()
        .map(|w| match w {
            WorkId::OpenAlex(id) => id.to_string(),
            other => other.to_string(),
        })
        .collect();
    values.join("|")
}

/// The OpenAlex work id among a record's identifiers.
fn openalex_id(record: &SourceRecord) -> Option<&WorkId> {
    record.ids.iter().find(|id| id.kind() == IdKind::OpenAlex)
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
}
