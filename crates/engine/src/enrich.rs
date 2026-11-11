//! Filling in what a record's provider didn't say, from the providers
//! that know: notices (both directions) and open access.

use aspero_core::{Field, FieldSet, IdKind, ProviderId, SourceRecord, Update, Usage, WorkId};

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
                resolve::merge_updates(&mut work.updates, &found.work.updates);
                for target in found.work.update_to {
                    if !work.update_to.contains(&target) {
                        work.update_to.push(target);
                    }
                }
                work.type_as_notice();
                // OpenAlex flags a retraction or withdrawal notice itself as
                // retracted, without details: name the record as its own
                // notice, so every row about the one event shares `notice`.
                let own_doi = record.ids.iter().find(|id| id.kind() == IdKind::Doi);
                if work.update_to.iter().any(|t| t.kind.is_retraction())
                    && let Some(own) = own_doi
                {
                    let bare = |u: &&mut Update| u.kind.is_retraction() && u.notice.is_none();
                    for update in work.updates.iter_mut().filter(bare) {
                        update.notice = Some(own.clone());
                    }
                }
            }
        }
        self.date_notices(report).await;
    }

    /// Fills in where works can be read free, for records whose provider
    /// didn't say, from the first provider (by priority) that does.
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
                if let (Some(oa), Some(record)) =
                    (found.work.open_access, report.results[i].record.as_mut())
                {
                    record.work.open_access = Some(oa);
                }
            }
        }
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
