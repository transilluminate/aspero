//! Filling in what a record's provider didn't say, from the providers
//! that know: notices (both directions) and open access.

use aspero_core::{Field, FieldSet, IdKind, Notice, ProviderId, SourceRecord, Usage, WorkId};

use crate::{Engine, Entry, LookupReport, LookupResult, resolve};

impl Engine {
    /// Adds editorial notices, in both directions, to records that another
    /// provider supplied, from the providers that report them in detail
    /// (Crossref): an original then lists its corrections and retractions,
    /// a correction the work it corrects. Notices are typed as such and
    /// dated by when they appeared. Batched and cached, like lookups.
    pub async fn add_notices(&self, report: &mut LookupReport) {
        if let Some(entry) = self.registry() {
            let works = found_records(&report.results, |_| true);
            for (i, found) in self.ask_about(entry, works, &mut report.usage).await {
                let Some(record) = report.results[i].record.as_mut() else {
                    continue;
                };
                let work = &mut record.work;
                resolve::merge_notices(&mut work.notices, &found.work.notices);
                for target in found.work.notice_for {
                    if !work.notice_for.contains(&target) {
                        work.notice_for.push(target);
                    }
                }
                work.type_as_notice();
                // OpenAlex flags a retraction or withdrawal notice itself as
                // retracted, without details: name the record as its own
                // notice, so every row about the one event shares `notice`.
                let own_doi = record.ids.iter().find(|id| id.kind() == IdKind::Doi);
                if work.notice_for.iter().any(|t| t.kind.is_retraction())
                    && let Some(own) = own_doi
                {
                    let bare = |u: &&mut Notice| u.kind.is_retraction() && u.notice.is_none();
                    for update in work.notices.iter_mut().filter(bare) {
                        update.notice = Some(own.clone());
                    }
                }
            }
        }
        self.date_notices(report).await;
    }

    /// Fills in where works can be read free, for records whose provider
    /// didn't say, from the first provider (by priority) that does; not,
    /// for a record of the DOI's registry, from a record it overrules
    /// (another work's link, #183).
    pub async fn add_open_access(&self, report: &mut LookupReport) {
        for entry in &self.entries {
            if !entry
                .provider
                .capabilities()
                .fields
                .contains(Field::OpenAccess)
            {
                continue;
            }
            let works = found_records(&report.results, |r| r.work.open_access.is_none());
            for (i, found) in self.ask_about(entry, works, &mut report.usage).await {
                let Some(record) = report.results[i].record.as_mut() else {
                    continue;
                };
                if self.overruled(&found, record) {
                    continue;
                }
                if let Some(oa) = found.work.open_access {
                    record.work.open_access = Some(oa);
                }
            }
        }
    }

    /// Whether `record` is the DOI registry's and it [overrules] `other`:
    /// what `other` says is then of another work.
    ///
    /// [overrules]: crate::registry::overrules
    pub(crate) fn overruled(&self, other: &SourceRecord, record: &SourceRecord) -> bool {
        self.registry()
            .is_some_and(|r| r.provider.id() == record.source)
            && crate::registry::overrules(record, other)
    }

    /// One provider's records of works it didn't supply itself, each asked
    /// for by an identifier it accepts: `(work index, record)` per work it
    /// knows.
    pub(crate) async fn ask_about<'a>(
        &self,
        entry: &Entry,
        works: impl IntoIterator<Item = (usize, &'a SourceRecord)>,
        usage: &mut Vec<(ProviderId, Usage)>,
    ) -> Vec<(usize, SourceRecord)> {
        let provider = entry.provider.id();
        let kinds = &entry.provider.capabilities().lookup_kinds;
        let wanted: Vec<(usize, WorkId)> = works
            .into_iter()
            .filter(|(_, r)| r.source != provider)
            .filter_map(|(i, r)| {
                let id = r.ids.iter().find(|id| kinds.contains(&id.kind()))?;
                Some((i, id.clone()))
            })
            .collect();
        if wanted.is_empty() {
            return Vec::new();
        }
        let ids: Vec<WorkId> = wanted.iter().map(|(_, id)| id.clone()).collect();
        let answers = self.lookup_at(entry, &ids, FieldSet::ALL, usage).await;
        wanted
            .into_iter()
            .zip(answers)
            .filter_map(|((i, _), (_, record))| Some((i, record?)))
            .collect()
    }
}

/// The records found among `results`, by index, that `keep` selects.
pub(crate) fn found_records(
    results: &[LookupResult],
    keep: impl Fn(&SourceRecord) -> bool,
) -> impl Iterator<Item = (usize, &SourceRecord)> {
    results
        .iter()
        .enumerate()
        .filter_map(move |(i, r)| Some((i, r.record.as_ref().filter(|r| keep(r))?)))
}
