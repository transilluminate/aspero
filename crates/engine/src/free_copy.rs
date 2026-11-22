//! A work's free copy that a program can fetch (#204).
//!
//! A free copy is often only a PubMed Central article page, whose PDF
//! links turn scripts away. PMC's open data (on AWS) has the PDF of its
//! open-access articles at an address that doesn't. It is asked only when
//! a caller wants the copy ([`Engine::free_copy`], or a lookup with
//! [`Extras::free_copies`](crate::Extras::free_copies)), never in a
//! lookup, verify or search: one request about one work's PMCID, and a
//! second for its metadata when PMC has it.

use aspero_core::matching::same_work_id;
use aspero_core::{
    Field, FieldSet, FreeCopyKind, IdKind, OpenAccess, ProviderId, SourceRecord, Usage, WorkId,
};

use crate::enrich::found_records;
use crate::observe::{self, op};
use crate::{Attempt, Engine, LookupReport};

/// What the free-copy source's records hold.
const FIELDS: FieldSet = FieldSet::of(&[Field::OpenAccess]);

/// A work's free copy, as far as is known.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct FreeCopy {
    /// The work's open access, with the fetchable PDF where one was found
    /// (its `source` then the free-copy source's, `pmc`). `None` when
    /// nothing is known.
    pub open_access: Option<OpenAccess>,
    /// The free-copy source's answer, when it was asked (live or from the
    /// cache); `None` when it wasn't: the record already had a fetchable
    /// PDF, has no PMCID, or no source is set up. A failure leaves
    /// `open_access` as the record had it.
    pub attempt: Option<Attempt>,
    pub usage: Vec<(ProviderId, Usage)>,
}

impl FreeCopy {
    /// What kind of copy is known: [`OpenAccess::free_copy`], or
    /// [`NoneKnown`](FreeCopyKind::NoneKnown) without open access.
    pub fn kind(&self) -> FreeCopyKind {
        self.open_access
            .as_ref()
            .map_or(FreeCopyKind::NoneKnown, OpenAccess::free_copy)
    }
}

impl Engine {
    /// The free copy of the work `record` is: its open access, with a PDF
    /// a program can fetch from PMC's open data where the record has a
    /// PMCID and no such PDF yet. PMC offers the PDF only for articles in
    /// its open-access subset (not its author manuscripts), and keeps a
    /// retracted article's: its notices tell the retraction. Cached like
    /// lookups; a `call` event of `op=lookup`.
    pub async fn free_copy(&self, record: &SourceRecord) -> FreeCopy {
        let input = || {
            record
                .ids
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        };
        let work = Box::pin(async {
            let mut usage = Vec::new();
            let (open_access, attempt) = self
                .free_copies_of(&[record], &mut usage)
                .await
                .pop()
                .unwrap_or_default();
            FreeCopy {
                open_access,
                attempt,
                usage,
            }
        });
        self.observed(op::LOOKUP, 1, input, work, observe::never_fails)
            .await
    }

    /// [`free_copy`](Self::free_copy) for each record found, in place.
    pub(crate) async fn add_free_copies(&self, report: &mut LookupReport) {
        let found: Vec<(usize, &SourceRecord)> = found_records(&report.results, |_| true).collect();
        let records: Vec<&SourceRecord> = found.iter().map(|(_, r)| *r).collect();
        let indices: Vec<usize> = found.iter().map(|(i, _)| *i).collect();
        let copies = self.free_copies_of(&records, &mut report.usage).await;
        for (i, (open_access, _)) in indices.into_iter().zip(copies) {
            if let Some(record) = report.results[i].record.as_mut() {
                record.work.open_access = open_access;
            }
        }
    }

    /// Each record's open access, with the free-copy source's PDF where it
    /// has one, and that source's answer where it was asked.
    async fn free_copies_of(
        &self,
        records: &[&SourceRecord],
        usage: &mut Vec<(ProviderId, Usage)>,
    ) -> Vec<(Option<OpenAccess>, Option<Attempt>)> {
        let mut out: Vec<(Option<OpenAccess>, Option<Attempt>)> = records
            .iter()
            .map(|r| (r.work.open_access.clone(), None))
            .collect();
        let Some(entry) = &self.free_copies else {
            return out;
        };
        let wanted: Vec<(usize, WorkId)> = records
            .iter()
            .enumerate()
            .filter_map(|(i, r)| Some((i, wanted(r)?)))
            .collect();
        if wanted.is_empty() {
            return out;
        }
        let ids: Vec<WorkId> = wanted.iter().map(|(_, id)| id.clone()).collect();
        let answers = self.lookup_at(entry, &ids, FIELDS, usage).await;
        for ((i, _), (attempt, found)) in wanted.into_iter().zip(answers) {
            if let Some(found) = found
                && !another_work(records[i], &found)
            {
                out[i].0 = with_copy(out[i].0.take(), found.work.open_access);
            }
            out[i].1 = Some(attempt);
        }
        out
    }
}

/// The PMCID to ask about: a record's without a fetchable PDF.
fn wanted(record: &SourceRecord) -> Option<WorkId> {
    let has_pdf = record
        .work
        .open_access
        .as_ref()
        .is_some_and(|oa| oa.free_copy() == FreeCopyKind::Pdf);
    if has_pdf {
        return None;
    }
    record
        .ids
        .iter()
        .find(|id| id.kind() == IdKind::Pmcid)
        .cloned()
}

/// Whether `found` (the free-copy source's record of `record`'s PMCID)
/// names a DOI and `record` others only: the PMCID is then another work's.
fn another_work(record: &SourceRecord, found: &SourceRecord) -> bool {
    let dois = |r: &SourceRecord| -> Vec<WorkId> {
        r.ids
            .iter()
            .filter(|id| id.kind() == IdKind::Doi)
            .cloned()
            .collect()
    };
    let (mine, theirs) = (dois(record), dois(found));
    !mine.is_empty()
        && !theirs.is_empty()
        && !mine
            .iter()
            .any(|a| theirs.iter().any(|b| same_work_id(a, b)))
}

/// `known` open access with the copy the free-copy source has: its PDF
/// (and that source named), its page where none was known, and the work
/// free to read, as every PMC article is. Status and licence stay as they
/// were, but for a `closed` status, which the copy disproves, and a
/// licence or version not yet known.
fn with_copy(known: Option<OpenAccess>, copy: Option<OpenAccess>) -> Option<OpenAccess> {
    let Some(copy) = copy else {
        return known;
    };
    let Some(mut oa) = known else {
        return Some(copy);
    };
    if copy.is_oa && !oa.is_oa {
        oa.is_oa = true;
        if oa.status.as_deref() == Some("closed") {
            oa.status = None;
        }
        oa.source.clone_from(&copy.source);
    }
    if oa.url.is_none() {
        oa.url = copy.url;
    }
    if copy.pdf_url.is_some() {
        oa.pdf_url = copy.pdf_url;
        oa.source = copy.source;
        if oa.license.is_none() {
            oa.license = copy.license;
        }
        if oa.version.is_none() {
            oa.version = copy.version;
        }
    }
    Some(oa)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oa(is_oa: bool, pdf: Option<&str>, source: &str) -> OpenAccess {
        let mut oa = OpenAccess::default();
        oa.is_oa = is_oa;
        oa.pdf_url = pdf.map(str::to_owned);
        oa.source = Some(source.into());
        oa
    }

    #[test]
    fn a_pmc_pdf_replaces_a_browser_only_link_keeping_status_and_licence() {
        let mut known = oa(
            true,
            Some("https://europepmc.org/articles/PMC1?pdf=render"),
            "x",
        );
        known.status = Some("green".into());
        known.license = Some("cc-by-nc".into());
        let mut copy = oa(
            true,
            Some("https://pmc-oa-opendata.s3.amazonaws.com/a.pdf"),
            "pmc",
        );
        copy.license = Some("cc-by".into());
        copy.url = Some("https://pmc.ncbi.nlm.nih.gov/articles/PMC1/".into());
        let got = with_copy(Some(known), Some(copy)).unwrap();
        assert_eq!(got.free_copy(), FreeCopyKind::Pdf);
        assert_eq!(got.source.as_deref(), Some("pmc"));
        assert_eq!(got.status.as_deref(), Some("green"));
        assert_eq!(got.license.as_deref(), Some("cc-by-nc"));
        assert!(got.url.is_some());
    }

    #[test]
    fn a_closed_work_on_pmc_is_free_to_read() {
        let mut known = oa(false, None, "openalex");
        known.status = Some("closed".into());
        let got = with_copy(Some(known), Some(oa(true, None, "pmc"))).unwrap();
        assert!(got.is_oa);
        assert_eq!(got.status, None);
        assert_eq!(got.free_copy(), FreeCopyKind::BrowserOnly);
        assert_eq!(with_copy(None, None), None);
    }
}
