//! Finding abstracts across providers.
//!
//! Abstract coverage differs by provider and publisher: OpenAlex lacks them
//! for some publishers, Crossref has them only when the publisher deposited
//! one. So a work found at one provider without an abstract is looked up at
//! the others (by any identifier they accept) until one has it. Lookups are
//! batched per provider, cached, and free at OpenAlex (single works) and
//! Crossref. The abstract keeps its provenance: it is never blended into
//! another provider's record.

use aspero_core::{FieldSet, ProviderId, SourceRecord, Usage, WorkId};

use crate::Engine;

/// An abstract and the provider it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Abstract {
    pub text: String,
    pub source: ProviderId,
}

#[derive(Debug, Clone, Default)]
pub struct AbstractsReport {
    /// One per work, in order; `None` where no provider has an abstract.
    pub found: Vec<Option<Abstract>>,
    pub usage: Vec<(ProviderId, Usage)>,
}

impl Engine {
    /// The abstract for each work: its own record's if present, otherwise
    /// the first other provider's (by priority) that has one. Works are
    /// identified by their records' identifiers; pass a record with merged
    /// identifiers to widen the search.
    pub async fn abstracts(&self, works: &[&SourceRecord]) -> AbstractsReport {
        let mut found: Vec<Option<Abstract>> = works
            .iter()
            .map(|r| {
                non_blank(r.work.abstract_text.clone()).map(|text| Abstract {
                    text,
                    source: r.source.clone(),
                })
            })
            .collect();
        let mut usage = Vec::new();

        for entry in &self.entries {
            let provider = entry.provider.id();
            let kinds = &entry.provider.capabilities().lookup_kinds;
            // One identifier per still-missing work that this provider
            // accepts (and that it didn't already answer without one).
            let wanted: Vec<(usize, WorkId)> = works
                .iter()
                .enumerate()
                .filter(|(i, r)| found[*i].is_none() && r.source != provider)
                .filter_map(|(i, r)| {
                    let id = r.ids.iter().find(|id| kinds.contains(&id.kind()))?;
                    Some((i, id.clone()))
                })
                .collect();
            if wanted.is_empty() {
                continue;
            }
            let ids: Vec<WorkId> = wanted.iter().map(|(_, id)| id.clone()).collect();
            let answers = self.lookup_at(entry, &ids, FieldSet::ALL, &mut usage).await;
            for ((i, _), (_, record)) in wanted.into_iter().zip(answers) {
                if let Some(text) = record.and_then(|r| non_blank(r.work.abstract_text)) {
                    found[i] = Some(Abstract {
                        text,
                        source: provider.clone(),
                    });
                }
            }
        }
        AbstractsReport { found, usage }
    }
}

/// Some deposits carry an empty abstract element; treat it as absent.
fn non_blank(text: Option<String>) -> Option<String> {
    text.filter(|t| !t.trim().is_empty())
}
