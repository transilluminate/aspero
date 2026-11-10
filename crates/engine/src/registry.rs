//! Checking records against the registry that issued their DOI (Crossref):
//! aggregators occasionally merge two works into one record, so another
//! provider's answer can carry one work's DOI with another's date and page.

use aspero_core::matching::title_similarity;
use aspero_core::publication::first_page;
use aspero_core::{IdKind, PartialDate, ProviderId, SourceRecord, UpdateKind, WorkId};

use crate::{Engine, Entry, LookupReport};

/// One field on which a record and the registry's record of the same DOI
/// disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Disagreement {
    /// `issued`, `volume`, `page` or `title`.
    pub field: &'static str,
    /// The record's value.
    pub value: String,
    /// The registry's value.
    pub registry_value: String,
    pub registry: ProviderId,
}

/// Below this title similarity, two records are of different works.
const SAME_TITLE: f64 = 0.5;

/// Where `record` contradicts `registry`'s record of the same DOI. Dates
/// compare by year, any of either record's years counting (online-first
/// and print years differ legitimately); pages by first page.
pub fn disagreements(record: &SourceRecord, registry: &SourceRecord) -> Vec<Disagreement> {
    let (w, r) = (&record.work, &registry.work);
    let mut out = Vec::new();
    let mut differ = |field, value: String, registry_value: String| {
        out.push(Disagreement {
            field,
            value,
            registry_value,
            registry: registry.source.clone(),
        });
    };
    if let (Some(a), Some(b)) = (w.issued, r.issued) {
        let years = |d: PartialDate, others: &[i16]| {
            std::iter::once(d.year())
                .chain(others.iter().copied())
                .collect::<Vec<_>>()
        };
        let ours = years(a, &w.other_years);
        if !years(b, &r.other_years).iter().any(|y| ours.contains(y)) {
            differ("issued", a.to_string(), b.to_string());
        }
    }
    if let (Some(a), Some(b)) = (&w.volume, &r.volume)
        && !a.trim().eq_ignore_ascii_case(b.trim())
    {
        differ("volume", a.clone(), b.clone());
    }
    if let (Some(a), Some(b)) = (&w.page, &r.page)
        && !first_page(a).eq_ignore_ascii_case(first_page(b))
    {
        differ("page", a.clone(), b.clone());
    }
    if let (Some(a), Some(b)) = (&w.title, &r.title)
        && title_similarity(a, b) < SAME_TITLE
    {
        differ("title", a.clone(), b.clone());
    }
    out
}

impl Engine {
    /// The provider that issues DOIs' metadata and notices (Crossref).
    pub(crate) fn registry(&self) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|e| e.provider.capabilities().notices)
    }

    /// For each result, where its record contradicts the registry's record
    /// of the same DOI; empty when they agree, or there's nothing to
    /// compare (no DOI, or the registry answered itself). Cached, like
    /// lookups.
    pub async fn cross_check(&self, report: &mut LookupReport) -> Vec<Vec<Disagreement>> {
        let mut found = vec![Vec::new(); report.results.len()];
        let Some(registry) = self.registry() else {
            return found;
        };
        let works = crate::found_records(&report.results, |_| true);
        for (i, theirs) in self.ask_about(registry, works, &mut report.usage).await {
            if let Some(ours) = &report.results[i].record {
                found[i] = disagreements(ours, &theirs);
            }
        }
        found
    }

    /// Dates each notice by when it appeared: the earliest of the date the
    /// source gives (often the issue's) and the notice's own publication
    /// dates, from the registry and from the first provider to answer when
    /// that agrees with the registry. The source's date is kept as
    /// `deposited` when earlier dates win.
    pub(crate) async fn date_notices(&self, report: &mut LookupReport) {
        let Some(registry) = self.registry() else {
            return;
        };
        let mut notices: Vec<WorkId> = Vec::new();
        for record in report.results.iter().filter_map(|r| r.record.as_ref()) {
            for u in &record.work.updates {
                if let Some(n) = &u.notice
                    && u.kind != UpdateKind::NewVersion
                    && n.kind() == IdKind::Doi
                    && !notices.contains(n)
                {
                    notices.push(n.clone());
                }
            }
        }
        if notices.is_empty() {
            return;
        }
        let registered = self
            .lookup_at(
                registry,
                &notices,
                aspero_core::FieldSet::ALL,
                &mut report.usage,
            )
            .await;
        let first = self.lookup(&notices, aspero_core::FieldSet::DEFAULT).await;
        for (p, u) in first.usage {
            crate::add_usage(&mut report.usage, &p, u);
        }
        let appeared: Vec<Option<PartialDate>> = registered
            .into_iter()
            .zip(first.results)
            .map(|((_, theirs), ours)| {
                let consistent = ours.record.filter(|r| {
                    theirs
                        .as_ref()
                        .is_none_or(|t| disagreements(r, t).is_empty())
                });
                [theirs, consistent]
                    .into_iter()
                    .flatten()
                    .filter_map(|r| r.work.issued)
                    .min_by_key(|d| starts(*d))
            })
            .collect();
        for record in report.results.iter_mut().filter_map(|r| r.record.as_mut()) {
            for u in &mut record.work.updates {
                let Some(at) = u
                    .notice
                    .as_ref()
                    .and_then(|n| notices.iter().position(|x| x == n))
                else {
                    continue;
                };
                let earlier = |a: &PartialDate| u.date.is_none_or(|d| starts(*a) < starts(d));
                if let Some(earlier) = appeared[at].filter(earlier) {
                    u.deposited = u.date;
                    u.date = Some(earlier);
                }
            }
        }
    }
}

/// When a date's period starts: 2026-12 is no earlier than 2026-12-01.
fn starts(d: PartialDate) -> (i16, u8, u8) {
    (d.year(), d.month().unwrap_or(1), d.day().unwrap_or(1))
}
