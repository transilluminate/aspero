//! Turning scored candidates into a verdict, why ([`Reason`]), and a
//! one-sentence reason in English.

use aspero_core::matching::{self, PLAUSIBLE, VERIFIED, WorkKind};
use aspero_core::publication::first_page;
use aspero_core::{Agreement, Claim, Notice, NoticeKind};

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
    every_variant!(Verdict: Verified, Plausible, Mismatch, NotFound, Unverifiable);

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
    /// One sentence, for logs and agents: [`reason`](Self::reason) and
    /// [`notices`](Self::notices) in English.
    pub reason_text: String,
    /// Why, as data, for front ends that word it themselves.
    pub reason: Reason,
    /// What to know about the work the verdict is about, whatever the
    /// verdict: its retraction or withdrawal, and its newer version.
    pub notices: Vec<Notice>,
    /// The claim as read: its identifiers as written (a DOI cut short at a
    /// line break, with the readings tried in `alternatives`), and from
    /// free text the year, journal (where it can be told), volume, first
    /// page, and the title and authors as written: where the best match's
    /// title stands in it, else where its layout tells them
    /// ([`Claim::as_read_beside`]).
    pub read: Claim,
    /// The candidate the verdict is about, as an index into
    /// `report.candidates`: the matching work, or the best guess at what was
    /// meant.
    pub best_match: Option<usize>,
    /// Whether the claim's site decided which work the verdict is about:
    /// of candidates matching about as well, the best match is the page's
    /// publisher's ([`Claim::site`], [`ResolveReport::chosen_by_site`]).
    pub chosen_by_site: bool,
    pub report: ResolveReport,
}

const INTERNAL: &str = "aspero failed on this citation (an internal error); please report it";

impl Verification {
    /// The verification of a claim whose verify panicked: unverifiable,
    /// [`Reason::InternalError`], the panic's message in `reason_text`.
    pub(crate) fn internal(claim: &Claim, panic: &(dyn std::any::Any + Send)) -> Self {
        let message = panic
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("no message");
        Verification {
            verdict: Verdict::Unverifiable,
            reason_text: format!("{INTERNAL}: {message}"),
            reason: Reason::InternalError,
            notices: Vec::new(),
            read: claim.as_read(),
            best_match: None,
            chosen_by_site: false,
            report: ResolveReport::default(),
        }
    }
}

/// Why a verdict was reached. "The work" is the verdict's candidate
/// ([`Verification::best_match`]); "the identifiers" are those the claim gave.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Reason {
    /// The citation matches the work. (Verified)
    Matches,
    /// The citation partly matches the work. (Plausible) `differs` is
    /// never empty.
    PartlyMatches { differs: Vec<Difference> },
    /// The citation agrees with the work as far as it goes, but gives too
    /// little to confirm it: nothing differs, and no authors, identifier
    /// or journal and place corroborate it. (Plausible)
    Unconfirmed,
    /// The closest work shares the title, but other details differ.
    /// (Mismatch) `differs` is never empty.
    Closest { differs: Vec<Difference> },
    /// No matching work. (`NotFound`)
    NoMatch,
    /// The identifiers exist; the claim gives nothing else to compare.
    /// (Verified)
    IdExists,
    /// The identifiers exist and the citation agrees. (Verified)
    IdAgrees,
    /// The identifiers exist and the citation partly matches, or shares
    /// the title with an error such as a wrong year. (Plausible) `differs`
    /// is never empty.
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
    /// (`NotFound`)
    IdMissing,
    /// Providers failed, so nothing conclusive can be said. (Unverifiable)
    ProvidersFailed,
    /// aspero itself failed on this claim (a bug, caught so the rest of a
    /// batch goes on). (Unverifiable)
    InternalError,
}

impl Reason {
    every_variant!(Reason:
        Matches,
        PartlyMatches { differs: Vec::new() },
        Unconfirmed,
        Closest { differs: Vec::new() },
        NoMatch,
        IdExists,
        IdAgrees,
        IdPartlyMatches { differs: Vec::new() },
        IdElsewhere { cited: 0 },
        IdDisagrees,
        IdMissingElsewhere,
        IdMissing,
        ProvidersFailed,
        InternalError,
    );

    /// A short machine-friendly name.
    pub fn as_str(&self) -> &'static str {
        match self {
            Reason::Matches => "matches",
            Reason::PartlyMatches { .. } => "partly_matches",
            Reason::Unconfirmed => "unconfirmed",
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
            Reason::InternalError => "internal_error",
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
    /// The citation's value, as compared: the year, journal, volume or
    /// first page read from it, or the title, authors or journal given.
    /// `None` when free text was searched for the work's own (title,
    /// authors).
    pub cited: Option<String>,
    /// For [`CitedField::Kind`]: what the work is (`actual` as a value).
    pub kind: Option<WorkKind>,
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
    /// What kind of work it is: the work is a reply, correction or
    /// commentary on the one cited (`actual`: "correction", "reply", …).
    Kind,
    /// A claimed identifier that isn't the work's, beside another that is:
    /// a PMID or PMCID other than the work's, or one a different work's
    /// record carries (`actual`: the work's of that kind, `pmid:19623052`;
    /// `cited`: the one claimed).
    Identifier,
}

impl CitedField {
    every_variant!(CitedField: Title, Authors, Year, Journal, Volume, FirstPage, Kind, Identifier);

    pub fn as_str(self) -> &'static str {
        match self {
            CitedField::Title => "title",
            CitedField::Authors => "authors",
            CitedField::Year => "year",
            CitedField::Journal => "journal",
            CitedField::Volume => "volume",
            CitedField::FirstPage => "first_page",
            CitedField::Kind => "kind",
            CitedField::Identifier => "identifier",
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

/// Titles alike enough to be one book's.
const SAME_TITLE: f64 = 0.9;

/// Whether `other` is `cited`'s book in another edition, or another record
/// of it: the same title (not a review or correction of it), and an author
/// in common, or none on `cited`'s record to tell them apart. Not another
/// author's book of a common title ("Organic Chemistry").
fn another_edition(cited: &Candidate, other: &Candidate) -> bool {
    let (a, b) = (&cited.record.work, &other.record.work);
    let same_title = match (&a.title, &b.title) {
        (Some(x), Some(y)) => {
            matching::title_similarity(x, y) >= SAME_TITLE
                && matching::derivative_work_kind(x) == matching::derivative_work_kind(y)
        }
        _ => false,
    };
    same_title && (a.authors.is_empty() || matching::share_an_author(&a.authors, &b.authors))
}

/// The fields that don't fully agree. With `known_only`, only fields both
/// sides gave and that disagree; otherwise also those the citation doesn't
/// confirm.
fn differences(claim: &Claim, c: &Candidate, known_only: bool) -> Vec<Difference> {
    let a = &c.assessment;
    let w = &c.record.work;
    let off = |check: &matching::FieldCheck| {
        if known_only {
            matches!(check.agreement, Agreement::Mismatch | Agreement::Partial)
        } else {
            check.agreement != Agreement::Match
        }
    };
    let read = claim.as_read();
    let diff = |field, actual: Option<String>, cited: Option<String>| Difference {
        field,
        actual,
        cited,
        kind: None,
    };
    let authors = (!claim.authors.is_empty()).then(|| claim.authors.join("; "));
    // Of the years a text gives, the one nearest the work's, as compared.
    let actual_year = w.issued.map(|d| d.year());
    let cited_year = match (read.year, actual_year) {
        (Some(y), _) => Some(y),
        (None, Some(actual)) => claim
            .cited_years_beside(w.title.as_deref())
            .into_iter()
            .min_by_key(|y| y.abs_diff(actual)),
        (None, None) => None,
    };
    [
        off(&a.title).then(|| diff(CitedField::Title, None, claim.title.clone())),
        off(&a.authors).then(|| diff(CitedField::Authors, None, authors.clone())),
        matches!(a.year.agreement, Agreement::Mismatch | Agreement::Partial).then(|| {
            diff(
                CitedField::Year,
                actual_year.map(|y| y.to_string()),
                cited_year.map(|y| y.to_string()),
            )
        }),
        // The journal's article, cited by its preprint's DOI: the
        // identifier names the preprint server's version.
        (a.container.agreement == Agreement::Mismatch).then(|| {
            diff(
                CitedField::Journal,
                matching::preprint_version_claimed(claim, &c.record)
                    .map(str::to_owned)
                    .or_else(|| w.container_title.clone()),
                read.container.clone(),
            )
        }),
        (a.volume.agreement == Agreement::Mismatch)
            .then(|| diff(CitedField::Volume, w.volume.clone(), read.volume.clone())),
        (a.page.agreement == Agreement::Mismatch).then(|| {
            let first = w.page.as_deref().map(|p| first_page(p).to_owned());
            let cited = read.page.as_deref().map(|p| first_page(p).to_owned());
            diff(CitedField::FirstPage, first, cited)
        }),
        w.title
            .as_ref()
            .and_then(|_| matching::record_derivative_kind(w))
            .filter(|_| !matching::claim_mentions_derivative(claim))
            .map(|kind| Difference {
                kind: Some(kind),
                ..diff(CitedField::Kind, Some(kind.as_str().to_owned()), None)
            }),
        (a.ids == Agreement::Partial)
            .then(|| contested_id(claim, c))
            .flatten(),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The claimed identifier that isn't the work's when its identifiers
/// partly agree: a PMID or PMCID other than the work's, judged against
/// every identifier its records carry; else one that isn't the work's (a
/// different work's record carries it), beside the work's of its kind.
fn contested_id(claim: &Claim, c: &Candidate) -> Option<Difference> {
    // An earlier version's identifier, beside the newer version's record.
    let earlier = || {
        claim.ids.iter().find(|id| {
            c.record.work.notice_for.iter().any(|n| {
                n.kind == aspero_core::NoticeKind::NewVersion && matching::same_work_id(id, &n.work)
            })
        })
    };
    let (cited, actual) =
        if let Some((cited, actual)) = matching::conflicting_id(&claim.ids, &c.ids) {
            (cited, Some(actual))
        } else if let Some(cited) = earlier() {
            let own = c.record.ids.iter().find(|k| {
                k.kind() == cited.kind()
                    && !c
                        .record
                        .work
                        .notice_for
                        .iter()
                        .any(|n| matching::same_work_id(k, &n.work))
            });
            (cited, own)
        } else {
            let cited = claim
                .ids
                .iter()
                .find(|id| !c.ids.iter().any(|k| matching::same_work_id(k, id)))?;
            (cited, c.ids.iter().find(|k| k.kind() == cited.kind()))
        };
    Some(Difference {
        field: CitedField::Identifier,
        actual: actual.map(ToString::to_string),
        cited: Some(cited.to_string()),
        kind: None,
    })
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
    let notices: Vec<Notice> = best
        .map(|i| &report.candidates[i])
        .into_iter()
        .flat_map(|c| {
            [
                c.retraction(),
                c.notices.iter().find(|u| u.kind == NoticeKind::NewVersion),
            ]
        })
        .flatten()
        .cloned()
        .collect();
    let mut reason = explain(claim, &report, &why, best);
    if !notices.is_empty() {
        let described: Vec<String> = notices.iter().map(describe_notice).collect();
        reason = format!("{reason}; notice: {}", described.join("; "));
    }
    Verification {
        verdict,
        reason_text: reason,
        reason: why,
        notices,
        read: claim.as_read_beside(best.map(|i| &report.candidates[i].record.work)),
        best_match: best,
        chosen_by_site: report.chosen_by_site && best == Some(0),
        report,
    }
}

/// [`weigh`], settled: a reason that lists differences always lists
/// one. Agreeing without them is [`Reason::Unconfirmed`]; a closest work
/// that differs in nothing named isn't a mismatch to report.
fn decide(claim: &Claim, report: &ResolveReport) -> (Verdict, Reason, Option<usize>) {
    match weigh(claim, report) {
        (
            verdict,
            Reason::PartlyMatches { differs } | Reason::IdPartlyMatches { differs },
            best,
        ) if differs.is_empty() => (verdict, Reason::Unconfirmed, best),
        (_, Reason::Closest { differs }, _) if differs.is_empty() => {
            (Verdict::NotFound, Reason::NoMatch, None)
        }
        settled => settled,
    }
}

fn weigh(claim: &Claim, report: &ResolveReport) -> (Verdict, Reason, Option<usize>) {
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
                    differs: differences(claim, c, true),
                },
                Some(0),
            ),
            Some(c) if near_miss(c) => (
                Verdict::Mismatch,
                Reason::Closest {
                    differs: differences(claim, c, false),
                },
                Some(0),
            ),
            _ if failed => (Verdict::Unverifiable, Reason::ProvidersFailed, None),
            _ => (Verdict::NotFound, Reason::NoMatch, None),
        };
    }
    // The most confident candidate a claimed identifier names, though
    // another claimed (a PMID) may be another's (#148).
    let by_id = report
        .candidates
        .iter()
        .position(|c| matches!(c.assessment.ids, Agreement::Match | Agreement::Partial));
    // Not, from the identifiers' work at the place cited (its journal,
    // volume and first page) with its title or authors, a lookalike whose
    // identifiers or page disagree: the identifier and the place both name
    // the work. (Another paper's title and authors, cited at this one's
    // place, are still that paper's.)
    // `skip`'s own book in another edition (#322) is no reason to call its
    // identifier another work's: not when the identifier's record has the
    // title and year cited.
    let elsewhere = |skip: Option<usize>| {
        let own = skip.map(|i| &report.candidates[i]).filter(|c| {
            c.assessment.title.agreement == Agreement::Match
                && c.assessment.year.agreement == Agreement::Match
        });
        let placed = skip.is_some_and(|i| {
            let a = &report.candidates[i].assessment;
            [a.container, a.volume, a.page]
                .iter()
                .all(|f| f.agreement == Agreement::Match)
                && !(a.title.agreement == Agreement::Mismatch
                    && a.authors.agreement == Agreement::Mismatch)
        });
        report.candidates.iter().enumerate().position(|(i, c)| {
            let a = &c.assessment;
            Some(i) != skip
                && a.confidence >= PLAUSIBLE
                && !own.is_some_and(|own| another_edition(own, c))
                && !(placed
                    && (a.ids == Agreement::Mismatch || a.page.agreement == Agreement::Mismatch))
        })
    };
    match by_id.map(|i| (i, &report.candidates[i])) {
        // Beside the identifiers, at most a year, journal, volume or page:
        // the identifiers name the work, and any of those that disagree are
        // the citation's errors.
        Some((i, c)) if !claim.is_descriptive() => {
            let differs: Vec<Difference> = differences(claim, c, true)
                .into_iter()
                .filter(|d| d.field != CitedField::Kind)
                .collect();
            if differs.is_empty() {
                (Verdict::Verified, Reason::IdExists, Some(i))
            } else {
                (
                    Verdict::Plausible,
                    Reason::IdPartlyMatches { differs },
                    Some(i),
                )
            }
        }
        Some((i, c)) if c.assessment.confidence >= VERIFIED => {
            (Verdict::Verified, Reason::IdAgrees, Some(i))
        }
        Some((i, c)) if c.assessment.confidence >= PLAUSIBLE => (
            Verdict::Plausible,
            Reason::IdPartlyMatches {
                differs: differences(claim, c, true),
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
                    differs: differences(claim, c, true),
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

/// The reason in English, without notices.
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
        Reason::Unconfirmed => format!(
            "agrees with {} as far as it goes, but gives too little to confirm it",
            work()
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
        Reason::InternalError => INTERNAL.into(),
    }
}

/// "RETRACTED (2010-02-06, notice doi:…, via retraction-watch)".
pub fn describe_notice(u: &Notice) -> String {
    // The notice of a new version is the newer work itself.
    if u.kind == NoticeKind::NewVersion {
        let by = u
            .notice
            .as_ref()
            .map(|n| format!(" by {n}"))
            .unwrap_or_default();
        let date = u.date.map(|d| format!(" ({d})")).unwrap_or_default();
        return format!("superseded{by}{date}");
    }
    let what = match &u.kind {
        NoticeKind::Retraction => "RETRACTED".to_owned(),
        NoticeKind::Withdrawal => "WITHDRAWN".to_owned(),
        NoticeKind::ExpressionOfConcern => "expression of concern".to_owned(),
        NoticeKind::Correction => "corrected".to_owned(),
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
