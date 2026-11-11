//! Turning scored candidates into a verdict, why ([`Reason`]), and a
//! one-sentence reason in English.

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
#[non_exhaustive]
pub struct Verification {
    pub verdict: Verdict,
    /// One sentence, for people and agents: [`why`](Self::why) and
    /// [`notes`](Self::notes) in English.
    pub reason: String,
    /// Why, as data, for front ends that word it themselves.
    pub why: Reason,
    /// What to know about the work the verdict is about, whatever the
    /// verdict: its retraction or withdrawal, and its newer version.
    pub notes: Vec<Update>,
    /// The candidate the verdict is about, as an index into
    /// `report.candidates`: the matching work, or the best guess at what was
    /// meant.
    pub best: Option<usize>,
    pub report: ResolveReport,
}

/// Why a verdict was reached. "The work" is the verdict's candidate
/// ([`Verification::best`]); "the identifiers" are those the claim gave.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Reason {
    /// The citation matches the work. (Verified)
    Matches,
    /// The citation partly matches the work. (Plausible)
    PartlyMatches { differs: Vec<Difference> },
    /// The closest work shares the title, but other details differ.
    /// (Mismatch)
    Closest { differs: Vec<Difference> },
    /// No matching work. (NotFound)
    NoMatch,
    /// The identifiers exist; the claim gives nothing else to compare.
    /// (Verified)
    IdExists,
    /// The identifiers exist and the citation agrees. (Verified)
    IdAgrees,
    /// The identifiers exist and the citation partly matches, or shares
    /// the title with an error such as a wrong year. (Plausible)
    IdPartlyMatches { differs: Vec<Difference> },
    /// The identifiers are another work (candidate `cited`); the citation
    /// matches the work. (Mismatch)
    IdElsewhere { cited: usize },
    /// The identifiers' work (the work) doesn't match the citation, and no
    /// other does. (Mismatch)
    IdDisagrees,
    /// The identifiers don't exist; the citation matches the work.
    /// (Mismatch)
    IdMissingElsewhere,
    /// The identifiers don't exist, and no matching work was found.
    /// (NotFound)
    IdMissing,
    /// Providers failed, so nothing conclusive can be said. (Unverifiable)
    ProvidersFailed,
}

impl Reason {
    /// A short machine-friendly name.
    pub fn as_str(&self) -> &'static str {
        match self {
            Reason::Matches => "matches",
            Reason::PartlyMatches { .. } => "partly_matches",
            Reason::Closest { .. } => "closest",
            Reason::NoMatch => "no_match",
            Reason::IdExists => "id_exists",
            Reason::IdAgrees => "id_agrees",
            Reason::IdPartlyMatches { .. } => "id_partly_matches",
            Reason::IdElsewhere { .. } => "id_elsewhere",
            Reason::IdDisagrees => "id_disagrees",
            Reason::IdMissingElsewhere => "id_missing_elsewhere",
            Reason::IdMissing => "id_missing",
            Reason::ProvidersFailed => "providers_failed",
        }
    }

    /// The fields that differ, where the reason has them.
    pub fn differs(&self) -> &[Difference] {
        match self {
            Reason::PartlyMatches { differs }
            | Reason::Closest { differs }
            | Reason::IdPartlyMatches { differs } => differs,
            _ => &[],
        }
    }
}

/// A cited field the work disagrees with, and the work's value if it's
/// worth showing.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Difference {
    pub field: CitedField,
    pub actual: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CitedField {
    Title,
    Authors,
    Year,
    Journal,
    Volume,
    FirstPage,
}

impl CitedField {
    pub fn as_str(self) -> &'static str {
        match self {
            CitedField::Title => "title",
            CitedField::Authors => "authors",
            CitedField::Year => "year",
            CitedField::Journal => "journal",
            CitedField::Volume => "volume",
            CitedField::FirstPage => "first_page",
        }
    }
}

impl std::fmt::Display for Difference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.field.as_str().replace('_', " "))?;
        match (&self.actual, self.field) {
            (Some(v), CitedField::Journal) => write!(f, " (\"{v}\")"),
            (Some(v), _) => write!(f, " ({v})"),
            (None, _) => Ok(()),
        }
    }
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

/// The fields that don't fully agree. With `known_only`, only fields both
/// sides gave and that disagree; otherwise also those the citation doesn't
/// confirm.
fn differences(c: &Candidate, known_only: bool) -> Vec<Difference> {
    let a = &c.assessment;
    let w = &c.record.work;
    let off = |check: &matching::FieldCheck| match known_only {
        true => matches!(check.agreement, Agreement::Mismatch | Agreement::Partial),
        false => check.agreement != Agreement::Match,
    };
    let diff = |field, actual: Option<String>| Difference { field, actual };
    [
        off(&a.title).then(|| diff(CitedField::Title, None)),
        off(&a.authors).then(|| diff(CitedField::Authors, None)),
        matches!(a.year.agreement, Agreement::Mismatch | Agreement::Partial)
            .then(|| diff(CitedField::Year, w.issued.map(|d| d.year().to_string()))),
        (a.container.agreement == Agreement::Mismatch)
            .then(|| diff(CitedField::Journal, w.container_title.clone())),
        (a.volume.agreement == Agreement::Mismatch)
            .then(|| diff(CitedField::Volume, w.volume.clone())),
        (a.page.agreement == Agreement::Mismatch).then(|| {
            let first = w.page.as_deref().map(|p| first_page(p).to_owned());
            diff(CitedField::FirstPage, first)
        }),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The differences as a clause: "its year (2017) differs", "its authors and
/// year (2017) differ".
fn clause(differs: &[Difference]) -> Option<String> {
    let fields: Vec<String> = differs.iter().map(ToString::to_string).collect();
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
    let (verdict, why, best) = decide(claim, &report);
    // Existing is not the same as citable or current: say so whatever the
    // verdict.
    let notes: Vec<Update> = best
        .map(|i| &report.candidates[i])
        .into_iter()
        .flat_map(|c| {
            [
                c.retraction(),
                c.updates.iter().find(|u| u.kind == UpdateKind::NewVersion),
            ]
        })
        .flatten()
        .cloned()
        .collect();
    let mut reason = explain(claim, &report, &why, best);
    if !notes.is_empty() {
        let notes: Vec<String> = notes.iter().map(describe_update).collect();
        reason = format!("{reason}; note: {}", notes.join("; "));
    }
    Verification {
        verdict,
        reason,
        why,
        notes,
        best,
        report,
    }
}

fn decide(claim: &Claim, report: &ResolveReport) -> (Verdict, Reason, Option<usize>) {
    let best = report.candidates.first();
    let best_conf = best.map_or(0.0, |c| c.assessment.confidence);
    let failed = !report.errors.is_empty()
        || report
            .lookups
            .iter()
            .any(|l| l.record.is_none() && !l.is_not_found());

    if claim.ids.is_empty() {
        return match best {
            Some(_) if best_conf >= VERIFIED => (Verdict::Verified, Reason::Matches, Some(0)),
            Some(c) if best_conf >= PLAUSIBLE => (
                Verdict::Plausible,
                Reason::PartlyMatches {
                    differs: differences(c, true),
                },
                Some(0),
            ),
            Some(c) if near_miss(c) => (
                Verdict::Mismatch,
                Reason::Closest {
                    differs: differences(c, false),
                },
                Some(0),
            ),
            _ if failed => (Verdict::Unverifiable, Reason::ProvidersFailed, None),
            _ => (Verdict::NotFound, Reason::NoMatch, None),
        };
    }
    let by_id = report
        .candidates
        .iter()
        .position(|c| c.assessment.ids == Agreement::Match);
    let elsewhere = |skip: Option<usize>| {
        report
            .candidates
            .iter()
            .enumerate()
            .position(|(i, c)| Some(i) != skip && c.assessment.confidence >= PLAUSIBLE)
    };
    match by_id.map(|i| (i, &report.candidates[i])) {
        Some((i, _)) if !claim.is_descriptive() => (Verdict::Verified, Reason::IdExists, Some(i)),
        Some((i, c)) if c.assessment.confidence >= VERIFIED => {
            (Verdict::Verified, Reason::IdAgrees, Some(i))
        }
        Some((i, c)) if c.assessment.confidence >= PLAUSIBLE => (
            Verdict::Plausible,
            Reason::IdPartlyMatches {
                differs: differences(c, true),
            },
            Some(i),
        ),
        Some((i, c)) => match elsewhere(Some(i)) {
            Some(j) => (Verdict::Mismatch, Reason::IdElsewhere { cited: i }, Some(j)),
            // The identifiers' work, cited with an error (a wrong year):
            // the same title, and matching authors or year.
            None if near_miss(c) => (
                Verdict::Plausible,
                Reason::IdPartlyMatches {
                    differs: differences(c, true),
                },
                Some(i),
            ),
            None => (Verdict::Mismatch, Reason::IdDisagrees, Some(i)),
        },
        None if failed => (Verdict::Unverifiable, Reason::ProvidersFailed, None),
        None => match elsewhere(None) {
            Some(j) => (Verdict::Mismatch, Reason::IdMissingElsewhere, Some(j)),
            None => (Verdict::NotFound, Reason::IdMissing, None),
        },
    }
}

/// The reason in English, without notes.
fn explain(claim: &Claim, report: &ResolveReport, why: &Reason, best: Option<usize>) -> String {
    let claimed = claim
        .ids
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let work = || {
        best.map(|i| describe(&report.candidates[i]))
            .unwrap_or_default()
    };
    let details = || clause(why.differs());
    match why {
        Reason::Matches => format!("matches {}", work()),
        Reason::PartlyMatches { .. } => format!(
            "partly matches {}; {}",
            work(),
            details().unwrap_or_else(|| "check the details".into())
        ),
        Reason::Closest { .. } => format!(
            "closest work is {}, but {}",
            work(),
            details().unwrap_or_else(|| "its details differ".into())
        ),
        Reason::NoMatch => "no matching work found".into(),
        Reason::IdExists => format!("{claimed} exists: {}", work()),
        Reason::IdAgrees => format!("{claimed} and the citation agree: {}", work()),
        Reason::IdPartlyMatches { .. } => format!(
            "{claimed} exists and partly matches the citation: {}{}",
            work(),
            details().map(|d| format!("; {d}")).unwrap_or_default()
        ),
        Reason::IdElsewhere { cited } => format!(
            "{claimed} is {}, but the citation matches {}",
            describe(&report.candidates[*cited]),
            work()
        ),
        Reason::IdDisagrees => {
            format!("{claimed} is {}, which does not match the citation", work())
        }
        Reason::IdMissingElsewhere => {
            format!("{claimed} does not exist; the citation matches {}", work())
        }
        Reason::IdMissing => format!("{claimed} does not exist and no matching work was found"),
        Reason::ProvidersFailed if claim.ids.is_empty() => {
            "providers failed; nothing conclusive".into()
        }
        Reason::ProvidersFailed => format!("could not look up {claimed}: providers failed"),
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
