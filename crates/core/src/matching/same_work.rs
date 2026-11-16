//! Identity: whether claimed identifiers are a work's, and whether two
//! records are one work (a preprint and its published version).

use super::Agreement;
use super::authors::{family_name, same_family};
use super::derivative::derivative_work_kind;
use super::text::title_similarity;
use crate::id::{IdKind, WorkId};
use crate::record::SourceRecord;

/// Whether claimed identifiers are among a work's identifiers (see
/// [`Assessment::ids`]).
pub fn id_agreement(claimed: &[WorkId], actual: &[WorkId]) -> Agreement {
    if claimed.is_empty() {
        return Agreement::Unknown;
    }
    if claimed
        .iter()
        .any(|c| actual.iter().any(|a| same_work_id(c, a)))
    {
        return Agreement::Match;
    }
    let shares_kind = claimed
        .iter()
        .any(|c| actual.iter().any(|a| a.kind() == c.kind()));
    if shares_kind {
        Agreement::Mismatch
    } else {
        Agreement::Unknown
    }
}

/// Identifier equality at the level of works: arXiv versions are the same work.
pub fn same_work_id(a: &WorkId, b: &WorkId) -> bool {
    match (a, b) {
        (WorkId::Arxiv(x), WorkId::Arxiv(y)) => x.unversioned() == y.unversioned(),
        _ => a == b,
    }
}

/// Title similarity at which two records' titles count as one title when
/// telling whether they are the same work.
const SAME_WORK_TITLE: f64 = 0.9;

/// Whether two records are one work, as a preprint and its published
/// version are: a shared DOI, PMID or arXiv ID (published records carry
/// their preprint's arXiv ID); else the same title, the same first author
/// and years at most one apart. A reply or erratum repeating a title is
/// not the work it answers.
pub fn same_work(a: &SourceRecord, b: &SourceRecord) -> bool {
    let shared = a.ids.iter().any(|x| {
        matches!(x.kind(), IdKind::Doi | IdKind::Pmid | IdKind::Arxiv)
            && b.ids.iter().any(|y| same_work_id(x, y))
    });
    if shared {
        return true;
    }
    let (wa, wb) = (&a.work, &b.work);
    let (Some(ta), Some(tb)) = (&wa.title, &wb.title) else {
        return false;
    };
    if title_similarity(ta, tb) < SAME_WORK_TITLE
        || derivative_work_kind(ta) != derivative_work_kind(tb)
    {
        return false;
    }
    let first = |w: &crate::work::Work| w.authors.first().and_then(family_name);
    let same_first = match (first(wa), first(wb)) {
        (Some(x), Some(y)) => same_family(&x, &y),
        _ => false,
    };
    let near_years = match (wa.issued, wb.issued) {
        (Some(x), Some(y)) => (x.year() - y.year()).abs() <= 1,
        _ => false,
    };
    same_first && near_years
}
