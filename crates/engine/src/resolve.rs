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
//!    claim's title, or the title the reference cites as located via the
//!    matchers' hits ([`matching::shared_title`]). Bare titles rank far
//!    better there than whole references (DESIGN §9), and locating the
//!    title in the reference keeps derivative hits ("Faculty Opinions
//!    recommendation of …") from steering the search.
//!
//! Candidates sharing an identifier are merged; each is scored against the
//! claim with [`assess`]. `verify` stops as soon as a candidate verifies, so
//! paid searches only run when the free sources weren't conclusive.

use aspero_core::matching::{self, id_agreement, same_work_id};
use aspero_core::{
    Agreement, Assessment, Claim, FieldSet, ProviderId, SearchQuery, SourceRecord, Update, Usage,
    WorkId,
};

use futures::future::{Either, join_all, select};
use futures::stream::{self, Stream, StreamExt};

use crate::verdict::{Verification, judge};
use crate::{Engine, EngineError, LookupResult, SearchReport, add_usage};

/// Title searches per claim: the claim's own title or the matchers' top
/// distinct titles.
const MAX_TITLE_SEARCHES: usize = 2;

/// Confidence at which `verify` stops gathering candidates early.
const DECISIVE: f64 = 0.95;

/// Claims verified at once in batches. Providers pace themselves, so this
/// overlaps different providers' waits rather than adding load to any one;
/// and it keeps a batch from queueing so many requests that a caller
/// sharing the engine (someone waiting) is stuck behind them.
const CLAIM_CONCURRENCY: usize = 4;

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
    /// Editorial notices from every merged record, most detailed kept.
    pub updates: Vec<Update>,
}

impl Candidate {
    /// The first retraction or withdrawal notice, if any.
    pub fn retraction(&self) -> Option<&Update> {
        self.updates.iter().find(|u| u.kind.is_retraction())
    }
}

/// Adds notices not already known. A detailed notice (with the notice's
/// identifier) replaces a bare one of the same kind, such as OpenAlex's
/// `is_retracted` flag.
pub(crate) fn merge_updates(known: &mut Vec<Update>, new: &[Update]) {
    for update in new {
        let same_kind = |k: &Update| k.kind == update.kind;
        if known
            .iter()
            .any(|k| same_kind(k) && k.notice == update.notice)
        {
            continue; // already known
        }
        if update.notice.is_none() {
            if !known.iter().any(same_kind) {
                known.push(update.clone());
            }
            continue; // a bare flag adds nothing to a known notice
        }
        match known
            .iter_mut()
            .find(|k| same_kind(k) && k.notice.is_none())
        {
            Some(bare) => *bare = update.clone(),
            None => known.push(update.clone()),
        }
    }
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

impl ResolveReport {
    /// Every failure: providers' searches, and lookups of the claimed
    /// identifiers that failed rather than finding nothing.
    pub fn failures(&self) -> Vec<EngineError> {
        let lookups = self
            .lookups
            .iter()
            .filter(|l| l.record.is_none())
            .flat_map(|l| &l.attempts)
            .filter_map(|a| match &a.outcome {
                crate::Outcome::Failed(error) => Some(EngineError::Provider {
                    provider: a.provider.clone(),
                    error: error.clone(),
                }),
                _ => None,
            });
        self.errors.iter().cloned().chain(lookups).collect()
    }
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
        // Alternative readings of claimed identifiers are looked up with
        // them, alongside the citation matchers; which was meant is settled
        // after, so a broken DOI costs no extra round of lookups.
        let mut wide = claim.clone();
        for a in &claim.alternatives {
            if claim.ids.contains(&a.claimed) && !wide.ids.contains(&a.alternative) {
                wide.ids.push(a.alternative.clone());
            }
        }
        let mut report = self.gather(&wide, limit, true).await;
        let claim = settle_alternatives(claim, &mut report);
        self.detail_notices(&mut report).await;
        judge(&claim, report)
    }

    /// Verifies many claims, several at a time (each provider's pacing
    /// still applies), yielding each with its claim's index as soon as it
    /// completes: a slow claim doesn't hold up the rest.
    pub fn verify_many<'a>(
        &'a self,
        claims: &'a [Claim],
        limit: u32,
    ) -> impl Stream<Item = (usize, Verification)> + 'a {
        stream::iter(claims.iter().enumerate())
            .map(move |(i, claim)| async move { (i, self.verify(claim, limit).await) })
            .buffer_unordered(CLAIM_CONCURRENCY)
    }

    /// For candidates known to be retracted only by a bare flag (OpenAlex's
    /// `is_retracted`), asks the other providers by identifier for the
    /// notice itself: date, notice DOI, source. Free, cached, and only for
    /// flagged works.
    async fn detail_notices(&self, report: &mut ResolveReport) {
        let bare = |c: &Candidate| {
            c.updates
                .iter()
                .any(|u| u.kind.is_retraction() && u.notice.is_none())
        };
        for entry in &self.entries {
            let provider = entry.provider.id();
            let kinds = &entry.provider.capabilities().lookup_kinds;
            let wanted: Vec<(usize, WorkId)> = report
                .candidates
                .iter()
                .enumerate()
                .filter(|(_, c)| bare(c) && !c.sightings.iter().any(|s| s.provider == provider))
                .filter_map(|(i, c)| {
                    Some((
                        i,
                        c.ids.iter().find(|id| kinds.contains(&id.kind()))?.clone(),
                    ))
                })
                .collect();
            if wanted.is_empty() {
                continue;
            }
            let ids: Vec<WorkId> = wanted.iter().map(|(_, id)| id.clone()).collect();
            let answers = self
                .lookup_at(entry, &ids, FieldSet::ALL, &mut report.usage)
                .await;
            for ((i, _), (_, record)) in wanted.into_iter().zip(answers) {
                if let Some(record) = record {
                    merge_updates(&mut report.candidates[i].updates, &record.work.updates);
                }
            }
        }
    }

    /// Looks up every identifier the claims assert, in one batched call per
    /// provider. With a cache attached, this warms it so verifying the
    /// claims one by one (and streaming results) doesn't look them up one
    /// by one.
    pub async fn prefetch(&self, claims: &[Claim]) -> crate::LookupReport {
        let mut ids: Vec<WorkId> = Vec::new();
        for id in claims.iter().flat_map(|c| &c.ids) {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        self.lookup(&ids, FieldSet::DEFAULT).await
    }

    async fn gather(&self, claim: &Claim, limit: u32, stop_when_verified: bool) -> ResolveReport {
        let mut acc = Accumulator {
            claim,
            report: ResolveReport::default(),
        };
        let done = |acc: &Accumulator| stop_when_verified && acc.verified();
        let query = |text: &str, citation: bool| {
            let mut q = if citation {
                SearchQuery::citation(text)
            } else {
                SearchQuery::new(text)
            };
            q.page.limit = limit;
            q
        };

        // Stage 1, concurrently (both free): lookups of the claimed
        // identifiers, and citation matchers on the whole reference.
        let reference = claim.reference();
        let matchers: Vec<ProviderId> = match &reference {
            Some(_) => self.searchers(true),
            None => Vec::new(),
        };
        let lookup = async {
            match claim.ids.is_empty() {
                true => None,
                false => Some(self.lookup(&claim.ids, FieldSet::DEFAULT).await),
            }
        };
        let matched = join_all(matchers.iter().map(|p| {
            let q = query(reference.as_deref().unwrap_or_default(), true);
            async move { self.search(Some(p.as_str()), &q).await }
        }));
        let (lookup, matched) = (std::pin::pin!(lookup), std::pin::pin!(matched));
        let (lookup, matched) = match select(lookup, matched).await {
            Either::Left((lookup, matched)) => (lookup, Either::Left(matched)),
            Either::Right((matched, lookup)) => (lookup.await, Either::Right(matched)),
        };

        if let Some(lookup) = lookup {
            for record in lookup.results.iter().filter_map(|r| r.record.as_ref()) {
                let sighting = Sighting {
                    provider: record.source.clone(),
                    via: Via::Lookup,
                    rank: 1,
                    score: None,
                };
                acc.add(record.clone(), sighting);
            }
            acc.usage(lookup.usage);
            acc.report.lookups = lookup.results;
        }
        // When the identifiers' lookup alone verifies the claim, the
        // matchers can't change the verdict: stop waiting for them (Crossref's
        // takes seconds).
        let matched = match matched {
            Either::Right(matched) => matched,
            Either::Left(_) if done(&acc) => Vec::new(),
            Either::Left(pending) => pending.await,
        };
        let mut titles: Vec<String> = claim.title.iter().cloned().collect();
        for (provider, result) in matchers.iter().zip(matched) {
            let hits = acc.add_search(provider, Via::CitationMatch, result);
            if let (None, Some(reference)) = (&claim.title, &reference) {
                // Query the title the reference cites, as found via each
                // hit: a commentary titled "Recommendation of X" yields X.
                titles.extend(
                    hits.iter()
                        .filter_map(|t| matching::shared_title(t, reference)),
                );
            }
        }
        // An identifier-only claim has nothing else to compare.
        if done(&acc) || !claim.is_descriptive() {
            return acc.finish();
        }

        // Stage 2, concurrently: title searches (or, with no title to go on,
        // cleaned-up keywords) on every keyword provider. Results are added
        // in provider-then-title order, so merging stays deterministic.
        titles = distinct(titles);
        titles.truncate(MAX_TITLE_SEARCHES);
        let mut jobs: Vec<(ProviderId, SearchQuery, Via)> = Vec::new();
        for provider in self.searchers(false) {
            if titles.is_empty()
                && let Some(text) = &claim.text
            {
                let q = query(&matching::search_terms(text), false);
                jobs.push((provider.clone(), q, Via::KeywordSearch));
            }
            for title in &titles {
                jobs.push((provider.clone(), query(title, false), Via::TitleSearch));
            }
        }
        let results = join_all(
            jobs.iter()
                .map(|(p, q, _)| self.search(Some(p.as_str()), q)),
        )
        .await;
        for ((provider, _, via), result) in jobs.iter().zip(results) {
            acc.add_search(provider, *via, result);
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

pub(crate) trait ClaimExt {
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
    fn add(&mut self, mut record: SourceRecord, sighting: Sighting) {
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
                merge_updates(&mut c.updates, &record.work.updates);
                // Pool the years every source reports (online-first vs
                // print, editions), then keep the better-matching record.
                let pooled: Vec<i16> = years(&c.record).chain(years(&record)).collect();
                add_years(&mut c.record, &pooled);
                add_years(&mut record, &pooled);
                // And keep a free copy's location, whichever record wins.
                let oa = |r: &SourceRecord| r.work.open_access.clone();
                let known = oa(&c.record).or_else(|| oa(&record));
                c.record.work.open_access.clone_from(&known);
                record.work.open_access = record.work.open_access.take().or(known);
                c.assessment = matching::assess(self.claim, &c.record);
                let assessment = matching::assess(self.claim, &record);
                if assessment.confidence > c.assessment.confidence {
                    c.record = record;
                    c.assessment = assessment;
                }
                c
            }
            None => {
                let mut updates = Vec::new();
                merge_updates(&mut updates, &record.work.updates);
                candidates.push(Candidate {
                    ids: record.ids.clone(),
                    updates,
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

    /// Adds one search's hits; returns the hits' titles.
    fn add_search(
        &mut self,
        provider: &ProviderId,
        via: Via,
        result: Result<SearchReport, EngineError>,
    ) -> Vec<String> {
        match result {
            Ok(report) => {
                if let Some(u) = report.usage {
                    add_usage(&mut self.report.usage, provider, u);
                }
                let mut titles = Vec::new();
                for (i, hit) in report.page.hits.into_iter().enumerate() {
                    // An untitled hit (a book chapter registered under the
                    // book) points to the cited title via its container.
                    let w = &hit.record.work;
                    titles.extend(w.title.clone().or_else(|| w.container_title.clone()));
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

    /// A candidate so clearly right that searching further can't improve
    /// on it: decisive, and either the identifiers the claim asserted or two
    /// providers vouch for it. A digest or reprint of a paper can match the
    /// claim exactly while only a later search finds the original.
    fn verified(&self) -> bool {
        self.report.candidates.iter().any(|c| {
            let corroborated = match self.claim.ids.is_empty() {
                true => c
                    .sightings
                    .iter()
                    .any(|s| s.provider != c.sightings[0].provider),
                false => c.assessment.ids == Agreement::Match,
            };
            c.assessment.confidence >= DECISIVE && corroborated
        })
    }

    fn finish(mut self) -> ResolveReport {
        rank(&mut self.report.candidates);
        self.report
    }
}

/// The claim with each claimed identifier that doesn't exist replaced by
/// an alternative reading that does, and the report made about that
/// claim: lookups of readings not taken dropped, candidates rescored.
fn settle_alternatives(claim: &Claim, report: &mut ResolveReport) -> Claim {
    let mut claim = claim.clone();
    if claim.alternatives.is_empty() {
        return claim;
    }
    let lookup = |id: &WorkId| report.lookups.iter().find(|l| l.id == *id);
    for a in claim.alternatives.clone() {
        let missing = lookup(&a.claimed).is_some_and(LookupResult::is_not_found);
        let exists = lookup(&a.alternative).is_some_and(|l| l.record.is_some());
        if missing && exists {
            for id in claim.ids.iter_mut().filter(|id| **id == a.claimed) {
                *id = a.alternative.clone();
            }
        }
    }
    report.lookups.retain(|l| claim.ids.contains(&l.id));
    for c in &mut report.candidates {
        c.assessment = matching::assess(&claim, &c.record);
        c.assessment.ids = id_agreement(&claim.ids, &c.ids);
    }
    rank(&mut report.candidates);
    claim
}

/// Most confident first; on a tie the earliest-published (an original
/// precedes its reprints, abstracts and corrections), then the one more
/// providers agree on.
fn rank(candidates: &mut [Candidate]) {
    let issued = |c: &Candidate| {
        c.record.work.issued.map_or(i64::MAX, |d| {
            i64::from(d.year()) * 10_000
                + i64::from(d.month().unwrap_or(12)) * 100
                + i64::from(d.day().unwrap_or(31))
        })
    };
    candidates.sort_by(|a, b| {
        b.assessment
            .confidence
            .total_cmp(&a.assessment.confidence)
            .then(issued(a).cmp(&issued(b)))
            .then(b.sightings.len().cmp(&a.sightings.len()))
    });
}

/// Every year a record reports: issued, then other editions or versions.
fn years(record: &SourceRecord) -> impl Iterator<Item = i16> + '_ {
    let w = &record.work;
    w.issued
        .map(|d| d.year())
        .into_iter()
        .chain(w.other_years.iter().copied())
}

/// Adds `pooled` years the record doesn't already report to its
/// `other_years`.
fn add_years(record: &mut SourceRecord, pooled: &[i16]) {
    let issued = record.work.issued.map(|d| d.year());
    for &y in pooled {
        if Some(y) != issued && !record.work.other_years.contains(&y) {
            record.work.other_years.push(y);
        }
    }
    record.work.other_years.sort_unstable();
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

#[cfg(test)]
mod tests {
    use aspero_core::UpdateKind;

    use super::*;

    fn update(kind: UpdateKind, notice: Option<&str>) -> Update {
        let mut u = Update::new(kind);
        u.notice = notice.map(|n| n.parse().unwrap());
        u
    }

    #[test]
    fn detailed_notices_replace_bare_flags_without_duplicates() {
        let mut known = vec![update(UpdateKind::Retraction, None)];
        let crossref = [
            update(UpdateKind::Correction, Some("10.1000/c1")),
            update(UpdateKind::Retraction, Some("10.1000/r")),
            update(UpdateKind::Correction, Some("10.1000/c2")),
        ];
        merge_updates(&mut known, &crossref);
        merge_updates(&mut known, &crossref); // again: nothing new
        merge_updates(&mut known, &[update(UpdateKind::Retraction, None)]);
        let summary: Vec<(String, Option<String>)> = known
            .iter()
            .map(|u| {
                (
                    u.kind.as_str().to_owned(),
                    u.notice.as_ref().map(ToString::to_string),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("retraction".into(), Some("doi:10.1000/r".into())),
                ("correction".into(), Some("doi:10.1000/c1".into())),
                ("correction".into(), Some("doi:10.1000/c2".into())),
            ]
        );
    }
}
