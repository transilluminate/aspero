//! `check`: verifying a whole bibliography, with fixes to suggest.
//!
//! The claims are verified as a batch ([`Engine::verify_batch`]), then each
//! verification is turned into [`Finding`]s beyond the verdict: the DOI to
//! add, the identifier to replace, a year that differs, and entries that
//! cite the same work twice. For a bibliography's entries
//! ([`Engine::check_entries`]), malformed fields and entries with nothing
//! to check are findings too.

use aspero_bib::{BibEntry, Note};
use aspero_core::matching::same_work_id;
use aspero_core::{Claim, IdKind, ProviderId, Update, UpdateKind, Usage, WorkId};
use futures::StreamExt;

use crate::{Candidate, Engine, Verdict, Verification, describe_update};

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
    /// The work has an editorial notice: retraction, correction, ….
    Updated { update: Update },
    /// One of a bibliography entry's own fields is malformed.
    Entry { note: Note },
    /// The bibliography entry has nothing to check: no title or identifier.
    Skipped,
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
            Finding::Duplicate { .. } | Finding::Entry { .. } | Finding::Skipped => {
                Severity::Warning
            }
            Finding::Updated { update } => match update.kind {
                UpdateKind::Retraction | UpdateKind::Withdrawal => Severity::Error,
                UpdateKind::ExpressionOfConcern | UpdateKind::NewVersion => Severity::Warning,
                _ => Severity::Info,
            },
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
            Finding::Updated { .. } => "updated",
            Finding::Entry { .. } => "entry",
            Finding::Skipped => "skipped",
        }
    }

    /// One line in English. `keys` name the entries a duplicate's `of`
    /// indexes.
    pub fn message(&self, keys: &[&str]) -> String {
        match self {
            Finding::Verdict { reason, .. } => reason.clone(),
            Finding::AddDoi { doi } => format!("add doi = {{{}}}", bare_doi(doi)),
            Finding::ReplaceId {
                claimed,
                with: Some(with),
            } => format!("replace {claimed} with {with}"),
            Finding::ReplaceId {
                claimed,
                with: None,
            } => {
                format!("{claimed} is not this work's identifier")
            }
            Finding::YearDiffers { claimed, actual } if (claimed - actual).abs() == 1 => {
                format!(
                    "year {claimed} → {actual} (often preprint vs published, or online vs print)"
                )
            }
            Finding::YearDiffers { claimed, actual } => format!("year {claimed} → {actual}"),
            Finding::Duplicate { of } => {
                format!("duplicate of {}", keys.get(*of).copied().unwrap_or("?"))
            }
            Finding::Updated { update } => describe_update(update),
            Finding::Entry { note } => note.to_string(),
            Finding::Skipped => "nothing to check: no title or identifier".into(),
        }
    }
}

/// A DOI without its `doi:` prefix, as a .bib file has it.
pub fn bare_doi(doi: &WorkId) -> String {
    doi.to_string().trim_start_matches("doi:").to_owned()
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

/// Every claim checked, and what it cost.
#[derive(Debug, Clone)]
pub struct CheckReport {
    /// In claim order.
    pub checked: Vec<Checked>,
    pub usage: Vec<(ProviderId, Usage)>,
}

impl Engine {
    /// Verifies every claim and derives findings, including duplicates
    /// across claims; in claim order. `progress` is called with each
    /// claim's index as it completes, in any order.
    pub async fn check(
        &self,
        claims: &[Claim],
        limit: u32,
        mut progress: impl FnMut(usize),
    ) -> CheckReport {
        let mut slots: Vec<Option<Checked>> = (0..claims.len()).map(|_| None).collect();
        let mut verifications = self.verify_batch(claims, limit);
        while let Some((i, verification)) = verifications.next().await {
            let findings = findings(&claims[i], &verification);
            slots[i] = Some(Checked {
                verification,
                findings,
            });
            progress(i);
        }
        let usage = verifications.into_usage();
        let mut checked: Vec<Checked> = slots
            .into_iter()
            .map(|c| c.expect("every claim verified"))
            .collect();
        let matches: Vec<Option<&Candidate>> = checked.iter().map(Checked::matched).collect();
        let duplicates = duplicates(&matches);
        for (c, dup) in checked.iter_mut().zip(duplicates) {
            if let Some(of) = dup {
                c.findings.push(Finding::Duplicate { of });
            }
        }
        CheckReport { checked, usage }
    }

    /// Checks a bibliography's entries as [`check`](Engine::check) does
    /// claims, those with nothing to check skipped; in file order, each
    /// entry's own problems included. A duplicate's `of` is an entry's
    /// index. `progress` is called with each checked entry's index as it
    /// completes.
    pub async fn check_entries(
        &self,
        entries: &[BibEntry],
        limit: u32,
        mut progress: impl FnMut(usize),
    ) -> BibliographyReport {
        let (positions, claims): (Vec<usize>, Vec<Claim>) = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.is_checkable())
            .map(|(i, e)| (i, e.claim.clone()))
            .unzip();
        let report = self.check(&claims, limit, |i| progress(positions[i])).await;
        let mut checked = report.checked.into_iter();
        let entries = entries
            .iter()
            .map(|entry| {
                let mut findings: Vec<Finding> = entry
                    .notes
                    .iter()
                    .map(|note| Finding::Entry { note: note.clone() })
                    .collect();
                if !entry.is_checkable() {
                    findings.push(Finding::Skipped);
                    return CheckedEntry {
                        verification: None,
                        findings,
                    };
                }
                let c = checked.next().expect("one per checkable entry");
                findings.extend(c.findings.into_iter().map(|f| match f {
                    Finding::Duplicate { of } => Finding::Duplicate { of: positions[of] },
                    f => f,
                }));
                CheckedEntry {
                    verification: Some(c.verification),
                    findings,
                }
            })
            .collect();
        BibliographyReport {
            entries,
            usage: report.usage,
        }
    }
}

/// A bibliography's entries checked, and what it cost.
#[derive(Debug, Clone)]
pub struct BibliographyReport {
    /// In file order.
    pub entries: Vec<CheckedEntry>,
    pub usage: Vec<(ProviderId, Usage)>,
}

impl BibliographyReport {
    pub fn summary(&self) -> CheckSummary {
        let mut s = CheckSummary {
            entries: self.entries.len(),
            ..CheckSummary::default()
        };
        for e in &self.entries {
            match e.verdict() {
                None => s.skipped += 1,
                Some(Verdict::Verified) => s.verified += 1,
                Some(_) => {}
            }
            match e.severity() {
                Some(Severity::Error) => s.errors += 1,
                Some(Severity::Warning) => s.warnings += 1,
                Some(Severity::Info) => s.suggestions += 1,
                None => {}
            }
        }
        s
    }
}

/// One bibliography entry checked.
#[derive(Debug, Clone)]
pub struct CheckedEntry {
    /// `None` when skipped.
    pub verification: Option<Verification>,
    /// Problems with the entry's own fields first, then
    /// [`Finding::Skipped`] or the verification's findings.
    pub findings: Vec<Finding>,
}

impl CheckedEntry {
    pub fn verdict(&self) -> Option<Verdict> {
        self.verification.as_ref().map(|v| v.verdict)
    }

    pub fn matched(&self) -> Option<&Candidate> {
        let v = self.verification.as_ref()?;
        v.best.map(|i| &v.report.candidates[i])
    }

    /// The most severe finding, if any.
    pub fn severity(&self) -> Option<Severity> {
        self.findings.iter().map(Finding::severity).max()
    }
}

/// Entries counted by outcome: each by its most severe finding.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct CheckSummary {
    pub entries: usize,
    pub verified: usize,
    pub errors: usize,
    pub warnings: usize,
    /// Entries whose findings are all only worth knowing (info).
    pub suggestions: usize,
    pub skipped: usize,
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
    out.extend(
        best.updates
            .iter()
            .map(|u| Finding::Updated { update: u.clone() }),
    );

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
    // Any edition's year counts (as in scoring); otherwise report the
    // nearest one.
    let work = &best.record.work;
    let years: Vec<i16> = work
        .issued
        .map(|d| d.year())
        .into_iter()
        .chain(work.other_years.iter().copied())
        .collect();
    if let Some(claimed) = claim.year
        && let Some(&actual) = years.iter().min_by_key(|y| (*y - claimed).abs())
        && actual != claimed
    {
        out.push(Finding::YearDiffers { claimed, actual });
    }
    out
}

/// The arXiv ID behind an arXiv DataCite DOI, which names the same work as
/// a published version's DOI.
fn arxiv_of(id: &WorkId) -> Option<WorkId> {
    let WorkId::Doi(doi) = id else { return None };
    doi.arxiv().map(WorkId::Arxiv)
}

/// A publisher DOI if the work has one, else its arXiv DOI.
fn preferred_doi(ids: &[WorkId]) -> Option<&WorkId> {
    let dois = || ids.iter().filter(|id| id.kind() == IdKind::Doi);
    dois()
        .find(|id| !matches!(id, WorkId::Doi(d) if d.is_arxiv()))
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
