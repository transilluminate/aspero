//! A work's references, a page at a time, and their cache (`edges`,
//! `references`).

use aspero_core::{Cursor, FieldSet, IdKind, WorkId};
use aspero_store::{Direction, Edges};

use super::{Coverage, ReferencesPage};
use crate::{Engine, EngineError, LookupResult, Outcome};

impl Engine {
    /// A page of the works `id` cites: `limit` from `cursor` (from a
    /// previous page's `next_cursor`), each looked up, in batches of up to
    /// 100. `id` is any identifier aspero looks up; it's resolved to an
    /// OpenAlex work as a search's relations are.
    pub async fn references(
        &self,
        id: &str,
        cursor: Option<&Cursor>,
        limit: u32,
    ) -> Result<ReferencesPage, EngineError> {
        let entry = self.openalex_entry()?;
        let mut usage = Vec::new();
        let start = match cursor {
            None => 0,
            Some(c) => c.0.parse::<usize>().map_err(|_| {
                EngineError::InvalidQuery(format!("not a references cursor: {:?}", c.0))
            })?,
        };

        let given: Option<WorkId> = id.parse().ok();
        let cached = given
            .as_ref()
            .filter(|w| w.kind() == IdKind::OpenAlex)
            .and_then(|w| Some((w.clone(), self.cached_references(w)?)));
        let (work, record, references) = match cached {
            Some((work, references)) => (work, None, Some(references)),
            None => {
                let w = self.openalex_work(id, &mut usage).await?;
                let work: WorkId =
                    WorkId::OpenAlex(w.parse().map_err(|e: aspero_core::IdError| {
                        EngineError::Unresolved {
                            id: id.to_owned(),
                            reason: e.to_string(),
                            kind: aspero_core::ErrorKind::Decode,
                        }
                    })?);
                let (attempt, record) = self
                    .lookup_at(
                        entry,
                        std::slice::from_ref(&work),
                        FieldSet::DEFAULT,
                        &mut usage,
                    )
                    .await
                    .pop()
                    .expect("one answer per id");
                let Some(record) = record else {
                    let (reason, kind) = match attempt.outcome {
                        Outcome::Failed(e) => (format!("the lookup failed: {e}"), e.kind()),
                        _ => (
                            "OpenAlex has no record of it".into(),
                            aspero_core::ErrorKind::NotFound,
                        ),
                    };
                    return Err(EngineError::Unresolved {
                        id: id.to_owned(),
                        reason,
                        kind,
                    });
                };
                let references = record.references.clone();
                if let Some(cited) = &references {
                    self.remember_references(&[(&work, cited)]);
                }
                (work, Some(record), references)
            }
        };

        let total = references.as_ref().map(Vec::len);
        let coverage = Coverage::of(references.as_deref());
        let page: Vec<WorkId> = references
            .iter()
            .flatten()
            .skip(start)
            .take(limit.max(1) as usize)
            .cloned()
            .collect();
        let answers = match page.is_empty() {
            true => Vec::new(),
            false => {
                self.lookup_at(entry, &page, FieldSet::DEFAULT, &mut usage)
                    .await
            }
        };
        let end = start + page.len();
        let next_cursor = total
            .filter(|t| end < *t && !page.is_empty())
            .map(|_| Cursor(end.to_string()));
        let references = page
            .into_iter()
            .zip(answers)
            .map(|(id, (attempt, record))| LookupResult {
                id,
                record,
                attempts: vec![attempt],
            })
            .collect();
        Ok(ReferencesPage {
            work,
            record,
            coverage,
            total,
            references,
            next_cursor,
            usage,
        })
    }

    /// A work's references cached within their time to live (none, past
    /// the cache).
    pub(super) fn cached_references(&self, work: &WorkId) -> Option<Vec<WorkId>> {
        let entry = self
            .cache
            .as_ref()?
            .get_edges(Direction::References, std::slice::from_ref(work))
            .ok()?
            .pop()??;
        self.current(&entry, self.cache_policy.references)
            .then_some(entry.value.ids)
    }

    pub(super) fn remember_references(&self, links: &[(&WorkId, &[WorkId])]) {
        if let Some(cache) = &self.cache {
            let edges: Vec<Edges> = links
                .iter()
                .map(|(_, ids)| Edges::new(ids.to_vec(), true))
                .collect();
            let rows: Vec<(&WorkId, &Edges)> = links.iter().map(|(w, _)| *w).zip(&edges).collect();
            let _ = cache.put_edges(Direction::References, &rows, (self.clock)());
        }
    }
}
