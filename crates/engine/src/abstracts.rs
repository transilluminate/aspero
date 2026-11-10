//! Finding abstracts across providers.
//!
//! Abstract coverage differs by provider and publisher: OpenAlex lacks them
//! for some publishers, Crossref has them only when the publisher deposited
//! one. So a work found at one provider without an abstract is looked up at
//! the others (by any identifier they accept) until one has it. Lookups are
//! batched per provider, cached, and free at OpenAlex (single works) and
//! Crossref. The abstract keeps its provenance: it is never blended into
//! another provider's record.

use aspero_core::{Field, ProviderId, SourceRecord, Usage};

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
            if !entry
                .provider
                .capabilities()
                .fields
                .contains(Field::Abstract)
            {
                continue;
            }
            let missing = works
                .iter()
                .copied()
                .enumerate()
                .filter(|(i, _)| found[*i].is_none());
            for (i, record) in self.ask_about(entry, missing, &mut usage).await {
                if let Some(text) = non_blank(record.work.abstract_text) {
                    found[i] = Some(Abstract {
                        text,
                        source: entry.provider.id(),
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
