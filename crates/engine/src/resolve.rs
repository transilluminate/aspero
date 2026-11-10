//! `resolve` and `verify`: from a citation claim to scored candidate works.
//!
//! Candidates are gathered in cost order, cheapest first:
//!
//! 1. **Lookup** of identifiers the claim asserts (free at OpenAlex and
//!    Crossref).
//! 2. **Citation matchers** (Crossref) with the whole reference. Good at
//!    picking the title out of a messy string, even when the work they
//!    return is a lookalike.
//! 3. **Title searches** on keyword providers (OpenAlex, $0.001 each): the
//!    claim's title, or the distinct titles of the matchers' top hits. Bare
//!    titles rank far better there than whole references (DESIGN §9).
//!
//! Candidates sharing an identifier are merged; each is scored against the
//! claim with [`assess`]. `verify` stops as soon as a candidate verifies, so
//! paid searches only run when the free sources weren't conclusive.

use aspero_core::matching::{self, PLAUSIBLE, VERIFIED, id_agreement, same_work_id};
use aspero_core::{
    Agreement, Assessment, Claim, FieldSet, ProviderId, SearchQuery, SourceRecord, Usage, WorkId,
};

use crate::{Engine, EngineError, LookupResult, add_usage};

/// Title searches per claim: the claim's own title or the matchers' top
/// distinct titles.
const MAX_TITLE_SEARCHES: usize = 2;

/// How a candidate was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Via {
    Lookup,
    CitationMatch,
    TitleSearch,
    KeywordSearch,
}

impl Via {
    pub fn as_str(self) -> &'static str {
        match self {
            Via::Lookup => "lookup",
            Via::CitationMatch => "citation_match",
            Via::TitleSearch => "title_search",
            Via::KeywordSearch => "keyword_search",
        }
    }
}

/// One provider surfacing a candidate.
#[derive(Debug, Clone)]
pub struct Sighting {
    pub provider: ProviderId,
    pub via: Via,
    /// 1-based rank in that provider's results.
    pub rank: usize,
    pub score: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Candidate {
    /// The best-matching record among those merged into this candidate.
    pub record: SourceRecord,
    /// Every identifier any merged record asserted.
    pub ids: Vec<WorkId>,
    pub sightings: Vec<Sighting>,
    pub assessment: Assessment,
}

#[derive(Debug, Clone, Default)]
pub struct ResolveReport {
    /// Most confident first.
    pub candidates: Vec<Candidate>,
    /// Lookups of the claim's identifiers, if it had any.
    pub lookups: Vec<LookupResult>,
    /// Providers that failed; their absence from candidates is not evidence.
    pub errors: Vec<EngineError>,
    pub usage: Vec<(ProviderId, Usage)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Verdict {
    /// A work matching the claim exists (and so do its claimed identifiers).
    Verified,
    /// A work partly matching the claim exists; check the details.
    Plausible,
    /// The claimed identifier is wrong: it doesn't exist, or it belongs to
    /// a work that doesn't match the rest of the claim.
    Mismatch,
    /// No matching work was found.
    NotFound,
    /// Providers failed, so nothing conclusive can be said.
    Unverifiable,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Verified => "verified",
            Verdict::Plausible => "plausible",
            Verdict::Mismatch => "mismatch",
            Verdict::NotFound => "not_found",
            Verdict::Unverifiable => "unverifiable",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Verification {
    pub verdict: Verdict,
    /// One sentence, for people and agents.
    pub reason: String,
    /// The candidate the verdict is about, as an index into
    /// `report.candidates`: the matching work, or the best guess at what was
    /// meant.
    pub best: Option<usize>,
    pub report: ResolveReport,
}

impl Engine {
    /// Gathers and scores every candidate for `claim`. `limit` caps results
    /// per provider search.
    pub async fn resolve(&self, claim: &Claim, limit: u32) -> ResolveReport {
        self.gather(claim, limit, false).await
    }

    /// Decides whether `claim` refers to a real work, stopping early once it
    /// does.
    pub async fn verify(&self, claim: &Claim, limit: u32) -> Verification {
        let report = self.gather(claim, limit, true).await;
        judge(claim, report)
    }

    async fn gather(&self, claim: &Claim, limit: u32, stop_when_verified: bool) -> ResolveReport {
        let mut acc = Accumulator {
            claim,
            report: ResolveReport::default(),
        };
        let done = |acc: &Accumulator| stop_when_verified && acc.verified();

        if !claim.ids.is_empty() {
            let lookup = self.lookup(&claim.ids, FieldSet::DEFAULT).await;
            for result in &lookup.results {
                if let Some(record) = &result.record {
                    let sighting = Sighting {
                        provider: record.source.clone(),
                        via: Via::Lookup,
                        rank: 1,
                        score: None,
                    };
                    acc.add(record.clone(), sighting);
                }
            }
            acc.usage(lookup.usage);
            acc.report.lookups = lookup.results;
            // An identifier-only claim has nothing else to compare.
            if done(&acc) || !claim.is_descriptive() {
                return acc.finish();
            }
        }

        let mut titles: Vec<String> = claim.title.iter().cloned().collect();
        if let Some(reference) = claim.reference() {
            for provider in self.searchers(true) {
                let mut query = SearchQuery::citation(&reference);
                query.page.limit = limit;
                let hits = acc
                    .search(self, &provider, &query, Via::CitationMatch)
                    .await;
                if claim.title.is_none() {
                    titles.extend(hits.into_iter().take(MAX_TITLE_SEARCHES));
                }
            }
        }
        if done(&acc) {
            return acc.finish();
        }

        titles = distinct(titles);
        titles.truncate(MAX_TITLE_SEARCHES);
        for provider in self.searchers(false) {
            if titles.is_empty() {
                // No title to go on: fall back to cleaned-up keywords.
                let Some(text) = &claim.text else { continue };
                let mut query = SearchQuery::new(matching::search_terms(text));
                query.page.limit = limit;
                acc.search(self, &provider, &query, Via::KeywordSearch)
                    .await;
            }
            for title in &titles {
                if done(&acc) {
                    break;
                }
                let mut query = SearchQuery::new(title);
                query.page.limit = limit;
                acc.search(self, &provider, &query, Via::TitleSearch).await;
            }
        }
        acc.finish()
    }

    /// Providers that search, either citation matchers or keyword engines.
    fn searchers(&self, matchers: bool) -> Vec<ProviderId> {
        self.providers()
            .filter(|p| p.capabilities().search && p.capabilities().citation_search == matchers)
            .map(|p| p.id())
            .collect()
    }
}

trait ClaimExt {
    fn is_descriptive(&self) -> bool;
}

impl ClaimExt for Claim {
    /// Whether the claim says anything besides identifiers.
    fn is_descriptive(&self) -> bool {
        self.text.is_some() || self.title.is_some() || !self.authors.is_empty()
    }
}

struct Accumulator<'a> {
    claim: &'a Claim,
    report: ResolveReport,
}

impl Accumulator<'_> {
    /// Adds a record, merging it into an existing candidate that shares an
    /// identifier.
    fn add(&mut self, record: SourceRecord, sighting: Sighting) {
        let assessment = matching::assess(self.claim, &record);
        let candidates = &mut self.report.candidates;
        let existing = candidates.iter_mut().find(|c| {
            c.ids
                .iter()
                .any(|a| record.ids.iter().any(|b| same_work_id(a, b)))
        });
        let candidate = match existing {
            Some(c) => {
                for id in &record.ids {
                    if !c.ids.iter().any(|known| same_work_id(known, id)) {
                        c.ids.push(id.clone());
                    }
                }
                c.sightings.push(sighting);
                if assessment.confidence > c.assessment.confidence {
                    c.record = record;
                    c.assessment = assessment;
                }
                c
            }
            None => {
                candidates.push(Candidate {
                    ids: record.ids.clone(),
                    record,
                    sightings: vec![sighting],
                    assessment,
                });
                candidates.last_mut().expect("just pushed")
            }
        };
        // Judge claimed identifiers against everything known about the work.
        candidate.assessment.ids = id_agreement(&self.claim.ids, &candidate.ids);
    }

    /// Runs one search and adds its hits; returns the hits' titles.
    async fn search(
        &mut self,
        engine: &Engine,
        provider: &ProviderId,
        query: &SearchQuery,
        via: Via,
    ) -> Vec<String> {
        match engine.search(Some(provider.as_str()), query).await {
            Ok(report) => {
                if let Some(u) = report.usage {
                    add_usage(&mut self.report.usage, provider, u);
                }
                let mut titles = Vec::new();
                for (i, hit) in report.page.hits.into_iter().enumerate() {
                    titles.extend(hit.record.work.title.clone());
                    let sighting = Sighting {
                        provider: provider.clone(),
                        via,
                        rank: i + 1,
                        score: hit.score,
                    };
                    self.add(hit.record, sighting);
                }
                titles
            }
            Err(e) => {
                self.report.errors.push(e);
                Vec::new()
            }
        }
    }

    fn usage(&mut self, usage: Vec<(ProviderId, Usage)>) {
        for (provider, u) in usage {
            add_usage(&mut self.report.usage, &provider, u);
        }
    }

    /// A candidate already verifies the claim (including its identifiers,
    /// if it asserted any).
    fn verified(&self) -> bool {
        self.report.candidates.iter().any(|c| {
            c.assessment.confidence >= VERIFIED
                && (self.claim.ids.is_empty() || c.assessment.ids == Agreement::Match)
        })
    }

    fn finish(mut self) -> ResolveReport {
        self.report.candidates.sort_by(|a, b| {
            b.assessment
                .confidence
                .total_cmp(&a.assessment.confidence)
                .then(b.sightings.len().cmp(&a.sightings.len()))
        });
        self.report
    }
}

/// Titles with duplicates (by normalised form) removed, order kept.
fn distinct(titles: Vec<String>) -> Vec<String> {
    let mut seen = Vec::new();
    titles
        .into_iter()
        .filter(|t| {
            let norm = matching::normalise(t);
            let new = !norm.is_empty() && !seen.contains(&norm);
            seen.push(norm);
            new
        })
        .collect()
}

fn describe(c: &Candidate) -> String {
    let w = &c.record.work;
    let title = w.title.as_deref().unwrap_or("(untitled)");
    let year = w
        .issued
        .map(|d| format!(" ({})", d.year()))
        .unwrap_or_default();
    let id = c.ids.first().map(|i| format!(", {i}")).unwrap_or_default();
    format!("\"{title}\"{year}{id}")
}

/// Turns gathered candidates into a verdict.
fn judge(claim: &Claim, report: ResolveReport) -> Verification {
    let best = report.candidates.first();
    let best_conf = best.map_or(0.0, |c| c.assessment.confidence);
    let failed = !report.errors.is_empty()
        || report
            .lookups
            .iter()
            .any(|l| l.record.is_none() && !l.is_not_found());

    let (verdict, reason, best_index) = if claim.ids.is_empty() {
        match best {
            Some(c) if best_conf >= VERIFIED => (
                Verdict::Verified,
                format!("matches {}", describe(c)),
                Some(0),
            ),
            Some(c) if best_conf >= PLAUSIBLE => (
                Verdict::Plausible,
                format!("partly matches {}; check the details", describe(c)),
                Some(0),
            ),
            _ if failed => (
                Verdict::Unverifiable,
                "providers failed; nothing conclusive".into(),
                None,
            ),
            _ => (Verdict::NotFound, "no matching work found".into(), None),
        }
    } else {
        let claimed = claim
            .ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let by_id = report
            .candidates
            .iter()
            .position(|c| c.assessment.ids == Agreement::Match);
        let elsewhere = |skip: Option<usize>| {
            report
                .candidates
                .iter()
                .enumerate()
                .find(|(i, c)| Some(*i) != skip && c.assessment.confidence >= PLAUSIBLE)
        };
        match by_id.map(|i| (i, &report.candidates[i])) {
            Some((i, c)) if !claim.is_descriptive() => (
                Verdict::Verified,
                format!("{claimed} exists: {}", describe(c)),
                Some(i),
            ),
            Some((i, c)) if c.assessment.confidence >= VERIFIED => (
                Verdict::Verified,
                format!("{claimed} and the citation agree: {}", describe(c)),
                Some(i),
            ),
            Some((i, c)) if c.assessment.confidence >= PLAUSIBLE => (
                Verdict::Plausible,
                format!(
                    "{claimed} exists and partly matches the citation: {}",
                    describe(c)
                ),
                Some(i),
            ),
            Some((i, c)) => match elsewhere(Some(i)) {
                Some((j, other)) => (
                    Verdict::Mismatch,
                    format!(
                        "{claimed} is {}, but the citation matches {}",
                        describe(c),
                        describe(other)
                    ),
                    Some(j),
                ),
                None => (
                    Verdict::Mismatch,
                    format!(
                        "{claimed} is {}, which does not match the citation",
                        describe(c)
                    ),
                    Some(i),
                ),
            },
            None if failed => (
                Verdict::Unverifiable,
                format!("could not look up {claimed}: providers failed"),
                None,
            ),
            None => match elsewhere(None) {
                Some((j, other)) => (
                    Verdict::Mismatch,
                    format!(
                        "{claimed} does not exist; the citation matches {}",
                        describe(other)
                    ),
                    Some(j),
                ),
                None => (
                    Verdict::NotFound,
                    format!("{claimed} does not exist and no matching work was found"),
                    None,
                ),
            },
        }
    };
    Verification {
        verdict,
        reason,
        best: best_index,
        report,
    }
}
