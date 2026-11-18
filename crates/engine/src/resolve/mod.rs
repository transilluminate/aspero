//! `resolve` and `verify`: from a citation claim to scored candidate works.
//!
//! Candidates are gathered in cost order, cheapest first ([`gather`]):
//!
//! 1. **Lookup** of identifiers the claim asserts (free at OpenAlex and
//!    Crossref).
//! 2. **Citation matchers** (Crossref) with the whole reference. Good at
//!    picking the title out of a messy string, even when the work they
//!    return is a lookalike.
//! 3. **Title searches** on keyword providers (OpenAlex, $0.001 each): the
//!    claim's title, or the title the reference cites as located via the
//!    matchers' hits ([`matching::shared_title`]). Bare titles rank far
//!    better there than whole references (DESIGN "Provider notes and
//!    gotchas"), and locating the title in the reference keeps derivative
//!    hits ("Faculty Opinions recommendation of …") from steering the
//!    search.
//!
//! A claim from an article page without identifiers names the page's site
//! ([`Claim::site`]): Crossref is also asked for the title among the works
//! of the site's publisher (its DOI prefixes, `filter=prefix:…`, free), and
//! between candidates that match about as well the publisher's is
//! preferred (`rank::prefer_site`).
//!
//! Candidates sharing an identifier are merged ([`merge`]); each is scored
//! against the claim with [`assess`] and ranked
//! ([`rank`](rank::rank)). `verify` stops as soon as a candidate verifies,
//! so paid searches only run when the free sources weren't conclusive.
//!
//! [`matching::shared_title`]: aspero_core::matching::shared_title

use aspero_core::matching::id_agreement;
use aspero_core::{
    Assessment, Claim, FieldSet, IdKind, ProviderId, SourceRecord, Update, Usage, WorkId,
};

use futures::stream::{self, Stream, StreamExt};

use crate::verdict::{Verification, judge};
use crate::{Engine, EngineError, LookupResult};

mod gather;
mod merge;
mod rank;

use merge::assess;
pub(crate) use merge::merge_updates;
use rank::rank;

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
    /// A search by volume and first page.
    PlaceSearch,
}

impl Via {
    pub fn as_str(self) -> &'static str {
        match self {
            Via::Lookup => "lookup",
            Via::CitationMatch => "citation_match",
            Via::TitleSearch => "title_search",
            Via::KeywordSearch => "keyword_search",
            Via::PlaceSearch => "place_search",
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

#[derive(Debug, Clone, Default)]
pub struct ResolveReport {
    /// Most confident first, but for the first: the claim's site's
    /// publisher's, when it matches about as well (`chosen_by_site`).
    pub candidates: Vec<Candidate>,
    /// Whether the first candidate is first for being the claim's site's
    /// publisher's ([`Claim::site`]): another would be first without it.
    pub chosen_by_site: bool,
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
        let work = Box::pin(self.gather(claim, limit, false));
        let input = || format!("{claim:?}");
        self.observed("resolve", 1, input, work, crate::observe::never_fails)
            .await
    }

    /// Decides whether `claim` refers to a real work, stopping early once it
    /// does.
    pub async fn verify(&self, claim: &Claim, limit: u32) -> Verification {
        let work = Box::pin(self.verify_claim(claim, limit));
        let input = || format!("{claim:?}");
        self.observed("verify", 1, input, work, crate::observe::never_fails)
            .await
    }

    async fn verify_claim(&self, claim: &Claim, limit: u32) -> Verification {
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
        let settled = settle_alternatives(claim, &mut report);
        self.detail_notices(&mut report).await;
        let mut v = judge(&settled, report);
        // As written, not as settled: a DOI cut short at a line break shows
        // beside the one found.
        v.read = claim.as_read();
        v
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

/// The claim with each claimed identifier that doesn't exist replaced by
/// an alternative reading that does, and the report made about that
/// claim: lookups of readings not taken dropped, candidates rescored.
fn settle_alternatives(claim: &Claim, report: &mut ResolveReport) -> Claim {
    let mut claim = claim.clone();
    let lookup = |id: &WorkId| report.lookups.iter().find(|l| l.id == *id);
    let missing = |id: &WorkId| lookup(id).is_some_and(LookupResult::is_not_found);
    let mut settled: Vec<(WorkId, WorkId)> = Vec::new();
    for a in &claim.alternatives {
        if missing(&a.claimed) && lookup(&a.alternative).is_some_and(|l| l.record.is_some()) {
            settled.push((a.claimed.clone(), a.alternative.clone()));
        }
    }
    // A DOI that doesn't exist, and a work found by the citation's text
    // whose DOI starts with it: the same DOI, cut short at a line break
    // (#39).
    for id in claim
        .ids
        .iter()
        .filter(|id| id.kind() == IdKind::Doi && missing(id))
    {
        let cited = id.to_string();
        let longer = report
            .candidates
            .iter()
            .flat_map(|c| &c.ids)
            .find(|c| c.kind() == IdKind::Doi && c.to_string().starts_with(&cited) && *c != id);
        if let Some(longer) = longer
            && !settled.iter().any(|(from, _)| from == id)
        {
            settled.push((id.clone(), longer.clone()));
        }
    }
    if claim.alternatives.is_empty() && settled.is_empty() {
        return claim;
    }
    for (from, to) in settled {
        for id in claim.ids.iter_mut().filter(|id| **id == from) {
            *id = to.clone();
        }
    }
    report.lookups.retain(|l| claim.ids.contains(&l.id));
    for c in &mut report.candidates {
        c.assessment = assess(&claim, &c.record, &c.ids);
        c.assessment.ids = id_agreement(&claim.ids, &c.ids);
    }
    report.chosen_by_site = rank(&mut report.candidates, &claim);
    claim
}

#[cfg(test)]
mod tests {
    use aspero_core::{FieldSet, PartialDate, ProviderId, Raw, SourceRecord, Work, WorkType};
    use jiff::Timestamp;

    /// A record of a work titled `title`, by `authors`, in `year`.
    pub(super) fn titled(doi: &str, title: &str, authors: &[&str], year: i16) -> SourceRecord {
        let mut work = Work::default();
        work.title = Some(title.into());
        work.authors = authors
            .iter()
            .map(|a| aspero_core::Person::literal(*a))
            .collect();
        work.issued = Some(PartialDate::from_year(year));
        work.work_type = Some(WorkType::JournalArticle);
        SourceRecord::new(
            ProviderId::from_static("openalex"),
            vec![doi.parse().unwrap()],
            work,
            FieldSet::ALL,
            Raw {
                media_type: "application/json".into(),
                body: "{}".into(),
            },
            Timestamp::UNIX_EPOCH,
        )
    }
}
