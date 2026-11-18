//! Identity: whether claimed identifiers are a work's, and whether two
//! records are one work (a preprint and its published version).

use super::Agreement;
use super::authors::{family_name, same_family};
use super::derivative::derivative_work_kind;
use super::titles::title_similarity;
use crate::id::{IdKind, WorkId};
use crate::record::SourceRecord;

/// Kinds of identifier an article has one of: a claimed one of these that
/// isn't among the work's, where the work has one of the kind, is
/// another's. Not DOIs, arXiv IDs or ISBNs, of which one work may have
/// several (a preprint's and the article's, a Zenodo version's, each
/// edition's).
const ONE_PER_WORK: &[IdKind] = &[IdKind::Pmid, IdKind::Pmcid];

/// Whether claimed identifiers are among a work's identifiers (see
/// [`super::Assessment::ids`]): `Partial` when one is, but another is of a
/// kind the work has one of (`ONE_PER_WORK`) and isn't the work's (a
/// paper's DOI beside another paper's PMID, #148).
pub fn id_agreement(claimed: &[WorkId], actual: &[WorkId]) -> Agreement {
    if claimed.is_empty() {
        return Agreement::Unknown;
    }
    if claimed
        .iter()
        .any(|c| actual.iter().any(|a| same_work_id(c, a)))
    {
        return if conflicting_id(claimed, actual).is_some() {
            Agreement::Partial
        } else {
            Agreement::Match
        };
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

/// The first claimed identifier of a kind a work has one of (`ONE_PER_WORK`)
/// that isn't the work's, with the work's of that kind: `pmid:19623055`
/// cited, `pmid:19623052` the work's. None when the work has none of the
/// kind (a Crossref record has no PMID): nothing contradicts it.
pub fn conflicting_id<'a>(
    claimed: &'a [WorkId],
    actual: &'a [WorkId],
) -> Option<(&'a WorkId, &'a WorkId)> {
    claimed
        .iter()
        .filter(|c| ONE_PER_WORK.contains(&c.kind()))
        .filter(|c| !actual.iter().any(|a| same_work_id(c, a)))
        .find_map(|c| Some((c, actual.iter().find(|a| a.kind() == c.kind())?)))
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
