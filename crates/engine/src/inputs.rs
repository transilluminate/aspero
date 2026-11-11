//! Looking up identifiers as people and documents write them: parsing each,
//! retrying DOIs whose trailing punctuation was part of them, and adding
//! what a full answer needs. Front ends share this so they answer alike.

use aspero_core::{Doi, FieldSet, IdError, ProviderId, SourceRecord, Usage, WorkId};

use crate::enrich::found_records;
use crate::{Abstract, Disagreement, Engine, LookupReport, LookupResult, add_usage};

/// What to add to each record found.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Extras {
    /// Notices in both directions, dated; open access from whichever
    /// provider knows; disagreements with the DOI's registry; and the
    /// latest version of superseded works.
    pub full: bool,
    pub abstracts: bool,
}

impl Extras {
    pub fn full() -> Self {
        Extras {
            full: true,
            abstracts: false,
        }
    }
}

/// One input's lookup.
#[derive(Debug, Clone)]
pub enum InputLookup {
    /// Not an identifier.
    Invalid(IdError),
    Looked(Box<Looked>),
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Looked {
    pub result: LookupResult,
    /// Found only with the trailing punctuation kept.
    pub with_punctuation: bool,
    /// With `full`: where the record contradicts the DOI's registry.
    pub disagreements: Vec<Disagreement>,
    /// With `full`: the latest version, if the work was superseded.
    pub latest: Option<SourceRecord>,
    /// With `abstracts`, for works found: the abstract, if any provider
    /// has one.
    pub abstract_text: Option<Option<Abstract>>,
}

#[derive(Debug, Clone)]
pub struct InputsReport {
    /// One per input, in order.
    pub items: Vec<InputLookup>,
    pub usage: Vec<(ProviderId, Usage)>,
}

impl Engine {
    /// Looks up each input as written: one batched lookup, a second for
    /// DOIs not found unless their trailing punctuation is kept, then
    /// `extras`. Never fails as a whole.
    pub async fn lookup_inputs(&self, inputs: &[String], extras: Extras) -> InputsReport {
        let parsed: Vec<Result<WorkId, IdError>> = inputs.iter().map(|s| s.parse()).collect();
        let (positions, valid): (Vec<usize>, Vec<WorkId>) = parsed
            .iter()
            .enumerate()
            .filter_map(|(i, p)| Some((i, p.as_ref().ok()?.clone())))
            .unzip();
        let mut report = self.lookup(&valid, FieldSet::DEFAULT).await;
        let retried = self
            .retry_with_punctuation(inputs, &positions, &mut report)
            .await;

        let n = report.results.len();
        let mut disagreements = vec![Vec::new(); n];
        let mut latest: Vec<Option<SourceRecord>> = vec![None; n];
        let mut abstracts: Vec<Option<Option<Abstract>>> = vec![None; n];
        if extras.full {
            self.add_notices(&mut report).await;
            self.add_open_access(&mut report).await;
            disagreements = self.cross_check(&mut report).await;
            let found: Vec<(usize, &SourceRecord)> =
                found_records(&report.results, |_| true).collect();
            let records: Vec<&SourceRecord> = found.iter().map(|(_, r)| *r).collect();
            let versions = self.latest_versions(&records).await;
            for ((i, _), record) in found.iter().zip(versions.latest) {
                latest[*i] = record;
            }
            for (p, u) in versions.usage {
                add_usage(&mut report.usage, &p, u);
            }
        }
        if extras.abstracts {
            let found: Vec<(usize, &SourceRecord)> =
                found_records(&report.results, |_| true).collect();
            let records: Vec<&SourceRecord> = found.iter().map(|(_, r)| *r).collect();
            let fetched = self.abstracts(&records).await;
            for ((i, _), a) in found.iter().zip(fetched.found) {
                abstracts[*i] = Some(a);
            }
            for (p, u) in fetched.usage {
                add_usage(&mut report.usage, &p, u);
            }
        }

        let mut looked = report
            .results
            .into_iter()
            .zip(disagreements)
            .zip(latest)
            .zip(abstracts)
            .map(
                |(((result, disagreements), latest), abstract_text)| Looked {
                    with_punctuation: retried.contains(&result.id),
                    result,
                    disagreements,
                    latest,
                    abstract_text,
                },
            );
        let items = parsed
            .into_iter()
            .map(|p| match p {
                Err(e) => InputLookup::Invalid(e),
                Ok(_) => InputLookup::Looked(Box::new(
                    looked.next().expect("one result per valid input"),
                )),
            })
            .collect();
        InputsReport {
            items,
            usage: report.usage,
        }
    }

    /// DOIs may legally end in `.` (or `,;:`), which parsing strips as
    /// sentence punctuation. For DOIs not found that way, asks once more
    /// with the punctuation kept, in one batch; found ones replace their
    /// results. Returns the identifiers found only on retry.
    async fn retry_with_punctuation(
        &self,
        inputs: &[String],
        positions: &[usize],
        report: &mut LookupReport,
    ) -> Vec<WorkId> {
        let retries: Vec<(usize, WorkId)> = positions
            .iter()
            .zip(&report.results)
            .enumerate()
            .filter(|(_, (_, r))| r.is_not_found())
            .filter_map(|(i, (&at, _))| {
                Some((i, WorkId::Doi(Doi::with_trailing_punctuation(&inputs[at])?)))
            })
            .collect();
        if retries.is_empty() {
            return Vec::new();
        }
        let ids: Vec<WorkId> = retries.iter().map(|(_, id)| id.clone()).collect();
        let second = self.lookup(&ids, FieldSet::DEFAULT).await;
        for (p, u) in second.usage {
            add_usage(&mut report.usage, &p, u);
        }
        let mut found = Vec::new();
        for ((i, _), result) in retries.into_iter().zip(second.results) {
            if result.record.is_some() {
                found.push(result.id.clone());
                report.results[i] = result;
            }
        }
        found
    }
}
