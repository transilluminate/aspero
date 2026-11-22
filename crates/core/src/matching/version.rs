//! Versions of one work told apart by what a citation gives: a Cochrane
//! review's, each its own record, the newer naming the older.

use super::Claim;
use super::authors::{claimed_family, same_family};
use super::same_work::same_work_id;
use crate::record::SourceRecord;
use crate::work::NoticeKind;

/// The years of the newer versions superseding `record`'s, when its own
/// identifier is the one claimed: a record merged with its newer
/// version's (OpenAlex merges a Cochrane review's versions) carries the
/// newer one's year among its own, but the version claimed is this one
/// ("… Cochrane Database Syst Rev. 2020;12:CD011545. doi:
/// 10.1002/14651858.cd011545" for the 2015 version, superseded in 2020).
/// None when the newer version's identifier is claimed too.
pub(super) fn newer_years(claim: &Claim, record: &SourceRecord) -> Vec<i16> {
    let held = claim
        .ids
        .iter()
        .any(|c| record.ids.iter().any(|r| same_work_id(r, c)));
    if !held {
        return Vec::new();
    }
    let own = record.work.issued.map(|d| d.year());
    record
        .work
        .notices
        .iter()
        .filter(|n| n.kind == NoticeKind::NewVersion)
        .filter(|n| {
            !n.notice
                .as_ref()
                .is_some_and(|newer| claim.ids.iter().any(|c| same_work_id(c, newer)))
        })
        .filter_map(|n| n.date.map(|d| d.year()))
        .filter(|&y| Some(y) != own)
        .collect()
}

/// Whether a citation of a newer version names another of its authors
/// first: an earlier version's byline, whose authors came in another order
/// ("Kohmura K, Banno M, Tsujimoto H, Tsujimoto Y, Kataoka Y" for the 2021
/// version, first author Tsujimoto Y, of a review first published in
/// 2019 under Kohmura). `byline` is the claim's, as written; `families`
/// the record's authors' family names, in order. Not when an identifier is
/// claimed: it says which version is meant.
pub(super) fn earlier_byline(
    claim: &Claim,
    record: &SourceRecord,
    byline: Option<&str>,
    families: &[String],
) -> bool {
    let newer = record
        .work
        .notice_for
        .iter()
        .any(|n| n.kind == NoticeKind::NewVersion);
    if !newer || !claim.ids.is_empty() || families.len() < 2 {
        return false;
    }
    let first = match claim.authors.first() {
        Some(name) => claimed_family(name),
        None => byline
            .and_then(|b| b.split([',', ';', '&']).next())
            .and_then(claimed_family),
    };
    first.is_some_and(|first| {
        !same_family(&families[0], &first) && families[1..].iter().any(|f| same_family(f, &first))
    })
}

/// Whether a claimed identifier is that of a version `record`'s supersedes
/// (its record names it): "… 2020;12:CD011545. doi:
/// 10.1002/14651858.cd011545" matched to the 2020 version's record, which
/// names the 2015 version's DOI as the one before. OpenAlex's record merging
/// the two carries both DOIs, so the identifier agrees, but names another
/// version.
pub fn earlier_id_claimed(claim: &Claim, record: &SourceRecord) -> bool {
    claim.ids.iter().any(|c| {
        record
            .work
            .notice_for
            .iter()
            .any(|n| n.kind == NoticeKind::NewVersion && same_work_id(c, &n.work))
    })
}
