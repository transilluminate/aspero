//! Does a record match what a citation claims? Pure scoring, no I/O.
//!
//! A [`Claim`] is what someone (often an agent) asserted: a free-text
//! reference, structured fields, identifiers, or a mix. [`assess`] compares
//! one candidate record against it, field by field, and combines the fields
//! into a confidence in `0.0..=1.0`.
//!
//! Lessons from real data shape the rules (see DESIGN §9):
//! - **Lookalikes exist**: re-deposits with the real title, authors and
//!   abstract under a different year and publisher. A wrong year is a strong
//!   penalty, never a detail.
//! - **One field is not proof**: a title alone ("Deep learning") matches
//!   many works, so confidence is capped unless at least two fields agree.
//! - Free-text references are not parsed into fields. Instead the record's
//!   fields are looked for *in* the text: its title as a phrase, its authors'
//!   family names, its year. This is robust to citation styles.

mod authors;
mod derivative;
mod text;

use serde::{Deserialize, Serialize};

use crate::id::WorkId;
use crate::publication::{self, first_page, journal_in, same_journal};
use crate::record::SourceRecord;

use authors::{author_overlap, family_name};
use derivative::claim_mentions_derivative;
pub use derivative::is_derivative;
use text::Normalised;
pub use text::{normalise, search_terms, shared_title, title_similarity, unmarked};

/// Field weights in the confidence (before penalties).
const W_TITLE: f64 = 0.5;
const W_AUTHORS: f64 = 0.25;
const W_YEAR: f64 = 0.25;

/// Multiplier when the year is off by two or more.
const YEAR_MISMATCH_PENALTY: f64 = 0.5;
/// Multiplier when the title clearly differs.
const TITLE_MISMATCH_PENALTY: f64 = 0.5;
/// Multiplier when the authors clearly differ.
const AUTHORS_MISMATCH_PENALTY: f64 = 0.5;
/// Confidence ceiling when fewer than two fields could be compared.
const SINGLE_FIELD_CAP: f64 = 0.7;
/// Confidence ceiling without corroborating authors or identifiers.
const UNCORROBORATED_CAP: f64 = 0.8;
/// Author score that corroborates a match: the first author present
/// (0.75 for structured claims, 0.9 in free text).
const CORROBORATING_AUTHORS: f64 = 0.75;
/// Multiplier for replies, corrections, errata and commentaries.
const DERIVATIVE_PENALTY: f64 = 0.5;
/// Confidence ceiling when the claimed volume or first page differs.
const LOCATION_MISMATCH_CAP: f64 = 0.8;
/// Multiplier when a structured claim names a different journal.
const JOURNAL_MISMATCH_PENALTY: f64 = 0.9;

/// Verdict thresholds on confidence.
pub const VERIFIED: f64 = 0.85;
pub const PLAUSIBLE: f64 = 0.6;

/// Authors compared from the start of a byline.
const AUTHORS_COMPARED: usize = 3;

/// What a citation asserts. Any part may be missing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Claim {
    /// A free-text reference, in any style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Identifiers asserted for the work.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ids: Vec<WorkId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Names as written: "Ashish Vaswani", "Vaswani, A.", "A. Vaswani".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<i16>,
    /// The journal (or other container), full or abbreviated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<String>,
    /// The first page (or a range, of which the first page counts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    /// Other readings of claimed identifiers (a DOI without the hyphen at
    /// a line break), to try only if the claimed one doesn't exist.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<Alternative>,
}

/// Another reading of a claimed identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Alternative {
    pub claimed: WorkId,
    pub alternative: WorkId,
}

impl Alternative {
    pub fn new(claimed: WorkId, alternative: WorkId) -> Self {
        Alternative {
            claimed,
            alternative,
        }
    }
}

impl Claim {
    /// A claim from a free-text reference, with any DOIs and arXiv IDs it
    /// mentions extracted as asserted identifiers.
    pub fn from_text(text: impl Into<String>) -> Self {
        let text = text.into();
        Claim {
            ids: find_ids(&text),
            text: Some(text),
            ..Claim::default()
        }
    }

    /// A claim from whatever a user typed: if the whole input is an
    /// identifier, a claim of just that identifier (an existence check);
    /// otherwise a free-text reference (see [`Claim::from_text`]).
    pub fn from_input(input: &str) -> Self {
        let input = input.trim();
        match input.parse::<WorkId>() {
            Ok(id) => Claim {
                ids: vec![id],
                ..Claim::default()
            },
            Err(_) => Claim::from_text(input),
        }
    }

    /// Nothing to search with: no identifiers, and no text, title or
    /// authors that aren't blank.
    pub fn is_empty(&self) -> bool {
        let blank = |s: &Option<String>| s.as_deref().is_none_or(|s| s.trim().is_empty());
        blank(&self.text)
            && self.ids.is_empty()
            && blank(&self.title)
            && self.authors.iter().all(|a| a.trim().is_empty())
    }

    /// A reference string for citation matchers: the text, or one composed
    /// from the fields.
    pub fn reference(&self) -> Option<String> {
        if let Some(text) = &self.text {
            return Some(text.clone());
        }
        // Vancouver-like: "Authors. Title. Journal. 2017;376:2545".
        let location = match (&self.volume, &self.page) {
            (Some(v), Some(p)) => Some(format!("{v}:{p}")),
            (Some(v), None) => Some(v.clone()),
            (None, p) => p.clone(),
        };
        let when = [self.year.map(|y| y.to_string()), location]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(";");
        let parts: Vec<String> = [
            (!self.authors.is_empty()).then(|| self.authors.join(", ")),
            self.title.clone(),
            self.container.clone(),
            (!when.is_empty()).then_some(when),
        ]
        .into_iter()
        .flatten()
        .collect();
        (!parts.is_empty()).then(|| parts.join(". "))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Agreement {
    Match,
    Partial,
    Mismatch,
    /// The claim or the record doesn't say.
    Unknown,
}

/// One field's comparison: a score in `0.0..=1.0` when comparable.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FieldCheck {
    pub agreement: Agreement,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
}

impl FieldCheck {
    const UNKNOWN: FieldCheck = FieldCheck {
        agreement: Agreement::Unknown,
        score: None,
    };

    fn exact(agrees: bool) -> Self {
        FieldCheck::scored(if agrees { 1.0 } else { 0.0 })
    }

    fn scored(score: f64) -> Self {
        let agreement = match score {
            s if s >= 0.9 => Agreement::Match,
            s if s >= 0.5 => Agreement::Partial,
            _ => Agreement::Mismatch,
        };
        FieldCheck {
            agreement,
            score: Some(score),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Assessment {
    pub confidence: f64,
    pub title: FieldCheck,
    pub authors: FieldCheck,
    pub year: FieldCheck,
    /// The journal: `Match` when the claim names it (or an abbreviation),
    /// `Mismatch` when a structured claim names another.
    pub container: FieldCheck,
    pub volume: FieldCheck,
    /// The first page.
    pub page: FieldCheck,
    /// Whether the claim's identifiers are the record's: `Match` if any
    /// claimed ID is the record's, `Mismatch` if the record has a different
    /// ID of the same kind, else `Unknown`.
    pub ids: Agreement,
}

/// Scores how well `record` matches `claim`.
pub fn assess(claim: &Claim, record: &SourceRecord) -> Assessment {
    let work = &record.work;
    let text = claim.text.as_deref().map(Normalised::new);

    let title = match (&claim.title, &work.title, &text) {
        (Some(claimed), Some(actual), _) => FieldCheck::scored(title_similarity(claimed, actual)),
        (None, Some(actual), Some(text)) => FieldCheck::scored(text.title_coverage(actual)),
        _ => FieldCheck::UNKNOWN,
    };

    let families: Vec<String> = work.authors.iter().filter_map(family_name).collect();
    let authors = if families.is_empty() {
        FieldCheck::UNKNOWN
    } else if !claim.authors.is_empty() {
        FieldCheck::scored(author_overlap(&claim.authors, &families))
    } else if let Some(text) = &text {
        FieldCheck::scored(text.author_coverage(&families))
    } else {
        FieldCheck::UNKNOWN
    };

    let actual_years: Vec<i16> = work
        .issued
        .map(|d| d.year())
        .into_iter()
        .chain(work.other_years.iter().copied())
        .collect();
    let claimed_years: Vec<i16> = match (claim.year, &text) {
        (Some(y), _) => vec![y],
        (None, Some(text)) => text.years(),
        _ => Vec::new(),
    };
    let year = match actual_years.is_empty() || claimed_years.is_empty() {
        false => {
            // Nearest pairing: any edition's year may be the one cited.
            let delta = claimed_years
                .iter()
                .flat_map(|c| actual_years.iter().map(move |a| (c - a).unsigned_abs()))
                .min()
                .unwrap_or(u16::MAX);
            // Halves per year off: off by one is common (online-first vs
            // print, preprint vs published) and counts as partial; two or
            // more is a mismatch, but a nearer year still ranks higher.
            FieldCheck::scored(0.5f64.powi(i32::from(delta.min(30))))
        }
        true => FieldCheck::UNKNOWN,
    };

    let container = match (&claim.container, &work.container_title, &text) {
        (Some(claimed), Some(actual), _) => FieldCheck::exact(same_journal(claimed, actual)),
        (None, Some(actual), Some(text)) if journal_in(&text.words, actual) => {
            FieldCheck::exact(true)
        }
        _ => FieldCheck::UNKNOWN,
    };
    let located = match &claim.text {
        Some(text) if claim.volume.is_none() && claim.page.is_none() => publication::locator(text),
        _ => publication::Locator {
            volume: claim.volume.clone(),
            page: claim.page.as_deref().map(|p| first_page(p).to_owned()),
        },
    };
    let same = |claimed: Option<&str>, actual: Option<&str>| match (claimed, actual) {
        (Some(c), Some(a)) if !c.is_empty() && !a.is_empty() => {
            FieldCheck::exact(c.trim().eq_ignore_ascii_case(a.trim()))
        }
        _ => FieldCheck::UNKNOWN,
    };
    let volume = same(located.volume.as_deref(), work.volume.as_deref());
    let page = same(
        located.page.as_deref(),
        work.page.as_deref().map(first_page),
    );

    // Citing the exact place: volume and first page.
    let located = volume.agreement == Agreement::Match && page.agreement == Agreement::Match;
    // Short Vancouver references often leave the title out ("Daum RS et
    // al. N Engl J Med 2017;376:2545"); with the journal and volume
    // agreeing, a title missing from the text is not a different one (the
    // page then decides between verified and a wrong page).
    let placed = container.agreement == Agreement::Match && volume.agreement == Agreement::Match;
    let title = match placed {
        true if claim.title.is_none() && title.agreement == Agreement::Mismatch => {
            FieldCheck::UNKNOWN
        }
        _ => title,
    };

    let ids = id_agreement(&claim.ids, &record.ids);
    // A claim of identifiers alone is fully confirmed by a matching ID.
    let id_only = [title, authors, year].iter().all(|c| c.score.is_none());
    let mut confidence = if id_only && ids == Agreement::Match {
        1.0
    } else {
        combine(title, authors, year)
    };
    // Title and year alone can't verify: a short or generic title ("Health
    // technology assessment") turns up inside many references, and letters
    // and corrections often carry the original's title without authors.
    // Corroboration means the first author matches, or a claimed ID does.
    // So does citing the exact place.
    let corroborated = authors.score.is_some_and(|s| s >= CORROBORATING_AUTHORS)
        || ids == Agreement::Match
        || located;
    if !id_only && !corroborated {
        confidence = confidence.min(UNCORROBORATED_CAP);
    }
    // A wrong volume or page is an error in the citation, and tells an
    // original from the letters and corrections in the same journal.
    if [volume, page]
        .iter()
        .any(|c| c.agreement == Agreement::Mismatch)
    {
        confidence = confidence.min(LOCATION_MISMATCH_CAP);
    }
    // Journal names compare by rule, which misses some pairs ("Br Med J" ↔
    // "BMJ"), so a different journal only lowers the ranking.
    if container.agreement == Agreement::Mismatch {
        confidence *= JOURNAL_MISMATCH_PENALTY;
    }
    // A reply, correction or commentary is not the work it responds to,
    // unless the citation says it is citing one.
    if let Some(actual) = &work.title
        && is_derivative(actual)
        && !claim_mentions_derivative(claim)
    {
        confidence *= DERIVATIVE_PENALTY;
    }

    Assessment {
        confidence,
        title,
        authors,
        year,
        container,
        volume,
        page,
        ids,
    }
}

fn combine(title: FieldCheck, authors: FieldCheck, year: FieldCheck) -> f64 {
    let parts = [(title, W_TITLE), (authors, W_AUTHORS), (year, W_YEAR)];
    let known: Vec<(f64, f64)> = parts
        .iter()
        .filter_map(|(check, w)| check.score.map(|s| (s, *w)))
        .collect();
    let weight: f64 = known.iter().map(|(_, w)| w).sum();
    if weight == 0.0 {
        return 0.0;
    }
    let mut confidence = known.iter().map(|(s, w)| s * w).sum::<f64>() / weight;
    if year.agreement == Agreement::Mismatch {
        confidence *= YEAR_MISMATCH_PENALTY;
    }
    if title.agreement == Agreement::Mismatch {
        confidence *= TITLE_MISMATCH_PENALTY;
    }
    // Different authors usually mean a different work (a similarly titled
    // book or survey), even when title and year agree.
    if authors.agreement == Agreement::Mismatch {
        confidence *= AUTHORS_MISMATCH_PENALTY;
    }
    if known.len() < 2 {
        confidence = confidence.min(SINGLE_FIELD_CAP);
    }
    confidence
}

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

/// The distinct identifiers mentioned in free text (see [`crate::id::scan`]).
pub fn find_ids(text: &str) -> Vec<WorkId> {
    let mut found: Vec<WorkId> = Vec::new();
    for mention in crate::id::scan(text) {
        if !found.iter().any(|f| same_work_id(f, &mention.id)) {
            found.push(mention.id);
        }
    }
    found
}

#[cfg(test)]
mod tests;
