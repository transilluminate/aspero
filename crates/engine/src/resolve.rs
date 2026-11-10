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

use aspero_core::matching::{self, PLAUSIBLE, VERIFIED, id_agreement, same_work_id};
use aspero_core::{
    Agreement, Assessment, Claim, FieldSet, ProviderId, SearchQuery, SourceRecord, Update,
    UpdateKind, Usage, WorkId,
};

use futures::future::{join, join_all};
use futures::stream::{self, Stream, StreamExt};

use crate::{Engine, EngineError, LookupResult, SearchReport, add_usage};

/// Title searches per claim: the claim's own title or the matchers' top
/// distinct titles.
const MAX_TITLE_SEARCHES: usize = 2;

/// Claims verified at once in batches. Providers pace themselves, so this
/// overlaps different providers' waits rather than adding load to any one.
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
fn merge_updates(known: &mut Vec<Update>, new: &[Update]) {
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
        let mut report = self.gather(claim, limit, true).await;
        self.detail_notices(&mut report).await;
        judge(claim, report)
    }

    /// Verifies many claims, several at a time (each provider's pacing
    /// still applies), yielding results in claim order as they complete.
    pub fn verify_many<'a>(
        &'a self,
        claims: &'a [Claim],
        limit: u32,
    ) -> impl Stream<Item = Verification> + 'a {
        stream::iter(claims)
            .map(move |claim| self.verify(claim, limit))
            .buffered(CLAIM_CONCURRENCY)
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
        let (lookup, matched) = join(lookup, matched).await;

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
                merge_updates(&mut c.updates, &record.work.updates);
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

/// Same title and authors, but something else (usually the year) is wrong:
/// a real work cited with an error, not a missing one. Authors must match
/// (first author present): short titles like "Protein Folding" occur inside
/// many longer ones, so the title alone proves little.
fn near_miss(c: &Candidate) -> bool {
    let a = &c.assessment;
    a.title.agreement == Agreement::Match && a.authors.agreement == Agreement::Match
}

/// The fields that don't fully agree, as a clause: "its year (2017)
/// differs", "its authors and year (2017) differ".
fn differences(c: &Candidate) -> String {
    let a = &c.assessment;
    let year = c
        .record
        .work
        .issued
        .map(|d| format!(" ({})", d.year()))
        .unwrap_or_default();
    let fields: Vec<String> = [
        (a.title.agreement != Agreement::Match).then(|| "title".to_owned()),
        (a.authors.agreement != Agreement::Match).then(|| "authors".to_owned()),
        matches!(a.year.agreement, Agreement::Mismatch | Agreement::Partial)
            .then(|| format!("year{year}")),
    ]
    .into_iter()
    .flatten()
    .collect();
    match fields.len() {
        0 => "its details differ".into(),
        1 => format!("its {} differs", fields[0]),
        _ => format!("its {} differ", fields.join(" and ")),
    }
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
            Some(c) if near_miss(c) => (
                Verdict::Mismatch,
                format!("closest work is {}, but {}", describe(c), differences(c)),
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
    // Existing is not the same as citable: say so whatever the verdict.
    let reason = match best_index.and_then(|i| report.candidates[i].retraction()) {
        Some(r) => format!("{reason}; note: {}", describe_update(r)),
        None => reason,
    };
    Verification {
        verdict,
        reason,
        best: best_index,
        report,
    }
}

/// "RETRACTED (2010-02-06, notice doi:…, via retraction-watch)".
pub fn describe_update(u: &Update) -> String {
    let what = match &u.kind {
        UpdateKind::Retraction => "RETRACTED".to_owned(),
        UpdateKind::Withdrawal => "WITHDRAWN".to_owned(),
        UpdateKind::ExpressionOfConcern => "expression of concern".to_owned(),
        UpdateKind::Correction => "corrected".to_owned(),
        other => other.as_str().replace('_', " "),
    };
    let details: Vec<String> = [
        u.date.map(|d| d.to_string()),
        u.notice.as_ref().map(|n| format!("notice {n}")),
        u.source.as_ref().map(|s| format!("via {s}")),
    ]
    .into_iter()
    .flatten()
    .collect();
    if details.is_empty() {
        what
    } else {
        format!("{what} ({})", details.join(", "))
    }
}

#[cfg(test)]
mod tests {
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
