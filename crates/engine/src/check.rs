//! `check`: verifying a whole bibliography, with fixes to suggest.
//!
//! Each claim is verified (identifiers prefetched in one batch first), then
//! its verification is turned into [`Finding`]s beyond the verdict: the DOI
//! to add, the identifier to replace, a year that differs, and entries that
//! cite the same work twice.

use aspero_core::matching::same_work_id;
use aspero_core::{Claim, IdKind, WorkId};

use crate::{Candidate, Engine, Verdict, Verification};

/// arXiv's DataCite prefix; a publisher DOI is preferred over it.
const ARXIV_DOI_PREFIX: &str = "10.48550";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

/// Something to fix or know about one entry.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Finding {
    /// The verdict itself, when it isn't `verified`.
    Verdict { verdict: Verdict, reason: String },
    /// Verified, but the entry has no DOI; the matched work's.
    AddDoi { doi: WorkId },
    /// The entry's identifier is wrong; the matched work's, if known.
    ReplaceId {
        claimed: WorkId,
        with: Option<WorkId>,
    },
    /// The entry's year differs from the matched work's: an error beyond
    /// one year, otherwise info.
    YearDiffers { claimed: i16, actual: i16 },
    /// Another entry (by index) cites the same work.
    Duplicate { of: usize },
}

impl Finding {
    pub fn severity(&self) -> Severity {
        match self {
            Finding::Verdict { verdict, .. } => match verdict {
                Verdict::Mismatch | Verdict::NotFound => Severity::Error,
                _ => Severity::Warning,
            },
            Finding::ReplaceId { .. } => Severity::Error,
            // Off by one is usually legitimate (preprint vs published,
            // online vs print), so it is only worth knowing.
            Finding::YearDiffers { claimed, actual } if (claimed - actual).abs() > 1 => {
                Severity::Error
            }
            Finding::YearDiffers { .. } | Finding::AddDoi { .. } => Severity::Info,
            Finding::Duplicate { .. } => Severity::Warning,
        }
    }

    /// A short machine-friendly name.
    pub fn kind(&self) -> &'static str {
        match self {
            Finding::Verdict { .. } => "verdict",
            Finding::AddDoi { .. } => "add_doi",
            Finding::ReplaceId { .. } => "replace_id",
            Finding::YearDiffers { .. } => "year_differs",
            Finding::Duplicate { .. } => "duplicate",
        }
    }
}

/// One checked claim.
#[derive(Debug, Clone)]
pub struct Checked {
    pub verification: Verification,
    pub findings: Vec<Finding>,
}

impl Checked {
    pub fn matched(&self) -> Option<&Candidate> {
        let v = &self.verification;
        v.best.map(|i| &v.report.candidates[i])
    }

    /// The most severe finding, if any.
    pub fn severity(&self) -> Option<Severity> {
        self.findings.iter().map(Finding::severity).max()
    }
}

impl Engine {
    /// Verifies every claim and derives findings, including duplicates
    /// across claims. `progress` is called with each claim's index as it
    /// completes.
    pub async fn check(
        &self,
        claims: &[Claim],
        limit: u32,
        mut progress: impl FnMut(usize),
    ) -> Vec<Checked> {
        self.prefetch(claims).await;
        let mut checked = Vec::with_capacity(claims.len());
        for (i, claim) in claims.iter().enumerate() {
            let verification = self.verify(claim, limit).await;
            let findings = findings(claim, &verification);
            checked.push(Checked {
                verification,
                findings,
            });
            progress(i);
        }
        let matches: Vec<Option<&Candidate>> = checked.iter().map(Checked::matched).collect();
        let duplicates = duplicates(&matches);
        for (c, dup) in checked.iter_mut().zip(duplicates) {
            if let Some(of) = dup {
                c.findings.push(Finding::Duplicate { of });
            }
        }
        checked
    }
}

/// Findings for one claim from its verification (not duplicates, which
/// need the whole set).
pub fn findings(claim: &Claim, v: &Verification) -> Vec<Finding> {
    let mut out = Vec::new();
    if v.verdict != Verdict::Verified {
        out.push(Finding::Verdict {
            verdict: v.verdict,
            reason: v.reason.clone(),
        });
    }
    let Some(best) = v.best.map(|i| &v.report.candidates[i]) else {
        return out;
    };
    let suggested = preferred_doi(&best.ids);

    // Each claimed identifier that isn't the matched work's: flagged when
    // the work has a different one of the same kind, or the verdict says the
    // claimed one doesn't belong (e.g. it doesn't exist). A kind the match
    // simply lacks (a PMID on a Crossref record) proves nothing.
    for claimed in &claim.ids {
        let as_arxiv = arxiv_of(claimed);
        let same = best.ids.iter().any(|id| {
            same_work_id(id, claimed) || as_arxiv.as_ref().is_some_and(|a| same_work_id(id, a))
        });
        let contradicted = best.ids.iter().any(|id| id.kind() == claimed.kind());
        if !same && (contradicted || v.verdict == Verdict::Mismatch) {
            out.push(Finding::ReplaceId {
                claimed: claimed.clone(),
                with: suggested.cloned(),
            });
        }
    }
    // No DOI claimed, but the work has one.
    let has_doi = claim.ids.iter().any(|id| id.kind() == IdKind::Doi);
    if !has_doi
        && matches!(v.verdict, Verdict::Verified | Verdict::Plausible)
        && let Some(doi) = suggested
    {
        out.push(Finding::AddDoi { doi: doi.clone() });
    }
    if let (Some(claimed), Some(actual)) = (claim.year, best.record.work.issued.map(|d| d.year()))
        && claimed != actual
    {
        out.push(Finding::YearDiffers { claimed, actual });
    }
    out
}

/// The arXiv ID behind an arXiv DataCite DOI, which names the same work as
/// a published version's DOI.
fn arxiv_of(id: &WorkId) -> Option<WorkId> {
    let WorkId::Doi(doi) = id else { return None };
    (doi.prefix() == ARXIV_DOI_PREFIX)
        .then(|| {
            doi.suffix()
                .strip_prefix("arxiv.")?
                .parse()
                .ok()
                .map(WorkId::Arxiv)
        })
        .flatten()
}

/// A publisher DOI if the work has one, else its arXiv DOI.
fn preferred_doi(ids: &[WorkId]) -> Option<&WorkId> {
    let dois = || ids.iter().filter(|id| id.kind() == IdKind::Doi);
    dois()
        .find(|id| {
            !id.to_string()
                .starts_with(&format!("doi:{ARXIV_DOI_PREFIX}/"))
        })
        .or_else(|| dois().next())
}

/// For each match, the earliest other match that is the same work (shares
/// an identifier).
pub fn duplicates(matches: &[Option<&Candidate>]) -> Vec<Option<usize>> {
    matches
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let m = (*m)?;
            matches[..i].iter().position(|other| {
                other.is_some_and(|o| {
                    o.ids
                        .iter()
                        .any(|a| m.ids.iter().any(|b| same_work_id(a, b)))
                })
            })
        })
        .collect()
}
