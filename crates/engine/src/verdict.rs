//! Turning scored candidates into a verdict and a one-sentence reason.

use aspero_core::matching::{self, PLAUSIBLE, VERIFIED};
use aspero_core::publication::first_page;
use aspero_core::{Agreement, Claim, Update, UpdateKind};

use crate::resolve::{Candidate, ClaimExt, ResolveReport};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Verdict {
    /// A work matching the claim exists (and so do its claimed identifiers).
    Verified,
    /// A work partly matching the claim exists; check the details.
    Plausible,
    /// The claimed identifier is wrong: it doesn't exist, or it belongs to
    /// a work that doesn't match the rest of the claim.
    Mismatch,
    /// No matching work was found.
    NotFound,
    /// Providers failed, so nothing conclusive can be said.
    Unverifiable,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Verified => "verified",
            Verdict::Plausible => "plausible",
            Verdict::Mismatch => "mismatch",
            Verdict::NotFound => "not_found",
            Verdict::Unverifiable => "unverifiable",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Verification {
    pub verdict: Verdict,
    /// One sentence, for people and agents.
    pub reason: String,
    /// The candidate the verdict is about, as an index into
    /// `report.candidates`: the matching work, or the best guess at what was
    /// meant.
    pub best: Option<usize>,
    pub report: ResolveReport,
}

/// The same title plus matching authors *or* year, with the other wrong: a
/// real work cited with an error (wrong year, misattributed authors), not a
/// missing one. The title alone proves little: short titles like "Protein
/// Folding" occur inside many longer ones.
fn near_miss(c: &Candidate) -> bool {
    let a = &c.assessment;
    a.title.agreement == Agreement::Match
        && (a.authors.agreement == Agreement::Match || a.year.agreement == Agreement::Match)
}

/// The fields that don't fully agree, as a clause: "its year (2017)
/// differs", "its authors and year (2017) differ". With `known_only`, only
/// fields both sides gave and that disagree; otherwise also those the
/// citation doesn't confirm.
fn differences(c: &Candidate, known_only: bool) -> Option<String> {
    let a = &c.assessment;
    let w = &c.record.work;
    let off = |check: &matching::FieldCheck| match known_only {
        true => matches!(check.agreement, Agreement::Mismatch | Agreement::Partial),
        false => check.agreement != Agreement::Match,
    };
    let value = |v: Option<String>| v.map(|v| format!(" ({v})")).unwrap_or_default();
    let fields: Vec<String> = [
        off(&a.title).then(|| "title".to_owned()),
        off(&a.authors).then(|| "authors".to_owned()),
        matches!(a.year.agreement, Agreement::Mismatch | Agreement::Partial)
            .then(|| format!("year{}", value(w.issued.map(|d| d.year().to_string())))),
        (a.container.agreement == Agreement::Mismatch).then(|| {
            let journal = w.container_title.as_ref().map(|t| format!("\"{t}\""));
            format!("journal{}", value(journal))
        }),
        (a.volume.agreement == Agreement::Mismatch)
            .then(|| format!("volume{}", value(w.volume.clone()))),
        (a.page.agreement == Agreement::Mismatch).then(|| {
            let first = w.page.as_deref().map(|p| first_page(p).to_owned());
            format!("first page{}", value(first))
        }),
    ]
    .into_iter()
    .flatten()
    .collect();
    match fields.len() {
        0 => None,
        1 => Some(format!("its {} differs", fields[0])),
        _ => Some(format!("its {} differ", fields.join(" and "))),
    }
}

fn describe(c: &Candidate) -> String {
    let w = &c.record.work;
    let title = w.title.as_deref().unwrap_or("(untitled)");
    let year = w
        .issued
        .map(|d| format!(" ({})", d.year()))
        .unwrap_or_default();
    let id = c.ids.first().map(|i| format!(", {i}")).unwrap_or_default();
    format!("\"{title}\"{year}{id}")
}

/// Turns gathered candidates into a verdict.
pub(crate) fn judge(claim: &Claim, report: ResolveReport) -> Verification {
    let best = report.candidates.first();
    let best_conf = best.map_or(0.0, |c| c.assessment.confidence);
    let failed = !report.errors.is_empty()
        || report
            .lookups
            .iter()
            .any(|l| l.record.is_none() && !l.is_not_found());

    let (verdict, reason, best_index) = if claim.ids.is_empty() {
        match best {
            Some(c) if best_conf >= VERIFIED => (
                Verdict::Verified,
                format!("matches {}", describe(c)),
                Some(0),
            ),
            Some(c) if best_conf >= PLAUSIBLE => (
                Verdict::Plausible,
                format!(
                    "partly matches {}; {}",
                    describe(c),
                    differences(c, true).unwrap_or_else(|| "check the details".into())
                ),
                Some(0),
            ),
            Some(c) if near_miss(c) => (
                Verdict::Mismatch,
                format!(
                    "closest work is {}, but {}",
                    describe(c),
                    differences(c, false).unwrap_or_else(|| "its details differ".into())
                ),
                Some(0),
            ),
            _ if failed => (
                Verdict::Unverifiable,
                "providers failed; nothing conclusive".into(),
                None,
            ),
            _ => (Verdict::NotFound, "no matching work found".into(), None),
        }
    } else {
        let claimed = claim
            .ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let by_id = report
            .candidates
            .iter()
            .position(|c| c.assessment.ids == Agreement::Match);
        let elsewhere = |skip: Option<usize>| {
            report
                .candidates
                .iter()
                .enumerate()
                .find(|(i, c)| Some(*i) != skip && c.assessment.confidence >= PLAUSIBLE)
        };
        match by_id.map(|i| (i, &report.candidates[i])) {
            Some((i, c)) if !claim.is_descriptive() => (
                Verdict::Verified,
                format!("{claimed} exists: {}", describe(c)),
                Some(i),
            ),
            Some((i, c)) if c.assessment.confidence >= VERIFIED => (
                Verdict::Verified,
                format!("{claimed} and the citation agree: {}", describe(c)),
                Some(i),
            ),
            Some((i, c)) if c.assessment.confidence >= PLAUSIBLE => (
                Verdict::Plausible,
                format!(
                    "{claimed} exists and partly matches the citation: {}{}",
                    describe(c),
                    differences(c, true)
                        .map(|d| format!("; {d}"))
                        .unwrap_or_default()
                ),
                Some(i),
            ),
            Some((i, c)) => match elsewhere(Some(i)) {
                Some((j, other)) => (
                    Verdict::Mismatch,
                    format!(
                        "{claimed} is {}, but the citation matches {}",
                        describe(c),
                        describe(other)
                    ),
                    Some(j),
                ),
                None => (
                    Verdict::Mismatch,
                    format!(
                        "{claimed} is {}, which does not match the citation",
                        describe(c)
                    ),
                    Some(i),
                ),
            },
            None if failed => (
                Verdict::Unverifiable,
                format!("could not look up {claimed}: providers failed"),
                None,
            ),
            None => match elsewhere(None) {
                Some((j, other)) => (
                    Verdict::Mismatch,
                    format!(
                        "{claimed} does not exist; the citation matches {}",
                        describe(other)
                    ),
                    Some(j),
                ),
                None => (
                    Verdict::NotFound,
                    format!("{claimed} does not exist and no matching work was found"),
                    None,
                ),
            },
        }
    };
    // Existing is not the same as citable or current: say so whatever the
    // verdict.
    let best = best_index.map(|i| &report.candidates[i]);
    let notes: Vec<String> = [
        best.and_then(Candidate::retraction),
        best.and_then(|c| c.updates.iter().find(|u| u.kind == UpdateKind::NewVersion)),
    ]
    .into_iter()
    .flatten()
    .map(describe_update)
    .collect();
    let reason = match notes.is_empty() {
        true => reason,
        false => format!("{reason}; note: {}", notes.join("; ")),
    };
    Verification {
        verdict,
        reason,
        best: best_index,
        report,
    }
}

/// "RETRACTED (2010-02-06, notice doi:…, via retraction-watch)".
pub fn describe_update(u: &Update) -> String {
    // The notice of a new version is the newer work itself.
    if u.kind == UpdateKind::NewVersion {
        let by = u
            .notice
            .as_ref()
            .map(|n| format!(" by {n}"))
            .unwrap_or_default();
        let date = u.date.map(|d| format!(" ({d})")).unwrap_or_default();
        return format!("superseded{by}{date}");
    }
    let what = match &u.kind {
        UpdateKind::Retraction => "RETRACTED".to_owned(),
        UpdateKind::Withdrawal => "WITHDRAWN".to_owned(),
        UpdateKind::ExpressionOfConcern => "expression of concern".to_owned(),
        UpdateKind::Correction => "corrected".to_owned(),
        other => other.as_str().replace('_', " "),
    };
    let details: Vec<String> = [
        u.date.map(|d| d.to_string()),
        u.notice.as_ref().map(|n| format!("notice {n}")),
        u.source.as_ref().map(|s| format!("via {s}")),
    ]
    .into_iter()
    .flatten()
    .collect();
    if details.is_empty() {
        what
    } else {
        format!("{what} ({})", details.join(", "))
    }
}
