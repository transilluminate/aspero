//! Checking records against the registry that issued their DOI (Crossref):
//! aggregators occasionally merge two works into one record, so another
//! provider's answer can carry one work's DOI with another's date and page.
//! Where the aggregator's record contradicts the registry's record of a DOI
//! it carries, the registry's wins for that DOI (#183): a lookup serves it
//! instead, and the record loses that DOI ([`overrules`],
//! [`drop_foreign`]).

use aspero_core::matching::{same_work_id, title_similarity};
use aspero_core::publication::first_page;
use aspero_core::{FieldSet, IdKind, NoticeKind, PartialDate, ProviderId, SourceRecord, WorkId};

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

/// Whether `record` contradicts `registry`'s record of a DOI it carries on
/// at least two of date, volume and page ([`disagreements`]): it is then,
/// at least in part, another work's record (two corrections one aggregator
/// merged into one work, #183). One alone differs routinely between
/// records of one work (an online-first year beside the print year, an
/// article number for a page, "Volume 15", a Cochrane issue as volume);
/// two works differ in where they appeared, and when. A title differing
/// isn't counted: translations and subtitles do.
fn contradicts(record: &SourceRecord, registry: &SourceRecord) -> bool {
    let differ = disagreements(record, registry)
        .iter()
        .filter(|d| d.field != "title")
        .count();
    differ >= CONTRADICTION
}

/// How many of date, volume and page must differ for a contradiction.
const CONTRADICTION: usize = 2;

/// The aggregator whose records the registry's overrule: it builds its
/// works from many sources, and occasionally merges two into one (#183).
/// Other providers' records are each a source's own (Europe PMC's are
/// PubMed's, one per article), and where one contradicts the registry
/// either may be wrong: a reprint's metadata deposited under the
/// original's DOI (wild-4 #131), an advance-access deposit with no volume
/// and an article number for a page (wild-3 #291). Those merge as before.
const AGGREGATOR: &str = "openalex";

/// Whether the registry's record (`registry`) wins over `record` for its
/// DOI: `record` is the aggregator's and [`contradicts`] it.
pub(crate) fn overrules(registry: &SourceRecord, record: &SourceRecord) -> bool {
    record.source.as_str() == AGGREGATOR && contradicts(record, registry)
}

/// Whether `ids` carry a DOI of `registry`'s record.
pub(crate) fn carries_doi(ids: &[WorkId], registry: &SourceRecord) -> bool {
    registry
        .ids
        .iter()
        .filter(|r| r.kind() == IdKind::Doi)
        .any(|r| ids.iter().any(|id| same_work_id(id, r)))
}

/// The identifiers of a record that `registry`'s [`overrules`], as far as
/// they may be its own: not the registry record's DOI, whose work it isn't,
/// nor the PMIDs and PMCIDs the registry's record doesn't confirm, which
/// may be either merged work's.
pub(crate) fn drop_foreign(ids: &mut Vec<WorkId>, registry: &SourceRecord) {
    let confirmed = |id: &WorkId| registry.ids.iter().any(|r| same_work_id(r, id));
    ids.retain(|id| match id.kind() {
        IdKind::Doi => !confirmed(id),
        IdKind::Pmid | IdKind::Pmcid => confirmed(id),
        _ => true,
    });
}

impl Engine {
    /// The provider that issues DOIs' metadata and notices (Crossref).
    pub(crate) fn registry(&self) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|e| e.provider.capabilities().notices)
    }

    /// For each result, where its record contradicts the registry's record
    /// of the same DOI (as asked for, else the record's); empty when they
    /// agree, or there's nothing to compare (no DOI, or the registry
    /// answered itself). Cached, like lookups.
    ///
    /// The registry's record wins for its DOI (#183): the aggregator's
    /// record, where it [`contradicts`] the registry's on two of date,
    /// volume and page ([`overrules`]), is replaced by it, with none of the
    /// record's other identifiers or links; and where it carries another
    /// DOI whose registry record it contradicts, it loses that DOI and the
    /// PMIDs and PMCIDs that record doesn't confirm ([`drop_foreign`]).
    pub async fn cross_check(&self, report: &mut LookupReport) -> Vec<Vec<Disagreement>> {
        let mut found = vec![Vec::new(); report.results.len()];
        let Some(registry) = self.registry() else {
            return found;
        };
        let provider = registry.provider.id();
        let kinds = &registry.provider.capabilities().lookup_kinds;
        // Per result, its own DOI (the one asked for, if a DOI) first, then
        // the record's other DOIs.
        let mut wanted: Vec<(usize, WorkId)> = Vec::new();
        for (i, result) in report.results.iter().enumerate() {
            let Some(record) = result.record.as_ref().filter(|r| r.source != provider) else {
                continue;
            };
            let asked = Some(&result.id).filter(|id| id.kind() == IdKind::Doi);
            let Some(own) =
                asked.or_else(|| record.ids.iter().find(|id| kinds.contains(&id.kind())))
            else {
                continue;
            };
            wanted.push((i, own.clone()));
            for other in &record.ids {
                if other.kind() == IdKind::Doi && !same_work_id(other, own) {
                    wanted.push((i, other.clone()));
                }
            }
        }
        if wanted.is_empty() {
            return found;
        }
        let ids: Vec<WorkId> = wanted.iter().map(|(_, id)| id.clone()).collect();
        let answers = self
            .lookup_at(registry, &ids, FieldSet::ALL, &mut report.usage)
            .await;
        let mut own_seen = vec![false; report.results.len()];
        for ((i, _), (_, theirs)) in wanted.into_iter().zip(answers) {
            let own = !std::mem::replace(&mut own_seen[i], true);
            let result = &mut report.results[i];
            let (Some(theirs), Some(ours)) = (theirs, result.record.as_mut()) else {
                continue;
            };
            if own {
                found[i] = disagreements(ours, &theirs);
            }
            // Once replaced, the record is the registry's own.
            if !overrules(&theirs, ours) {
                continue;
            }
            if own {
                let mut record = theirs;
                if !record.ids.iter().any(|id| same_work_id(id, &result.id)) {
                    record.ids.push(result.id.clone());
                }
                *ours = record;
            } else {
                drop_foreign(&mut ours.ids, &theirs);
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
            for u in &record.work.notices {
                if let Some(n) = &u.notice
                    && u.kind != NoticeKind::NewVersion
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
            .lookup_at(registry, &notices, FieldSet::ALL, &mut report.usage)
            .await;
        let first = self.lookup(&notices, FieldSet::DEFAULT).await;
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
            for u in &mut record.work.notices {
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
