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

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::id::{ArxivId, WorkId};
use crate::record::SourceRecord;
use crate::work::Person;

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

    pub fn is_empty(&self) -> bool {
        self.text.is_none()
            && self.ids.is_empty()
            && self.title.is_none()
            && self.authors.is_empty()
    }

    /// A reference string for citation matchers: the text, or one composed
    /// from the fields.
    pub fn reference(&self) -> Option<String> {
        if let Some(text) = &self.text {
            return Some(text.clone());
        }
        let parts: Vec<String> = [
            (!self.authors.is_empty()).then(|| self.authors.join(", ")),
            self.title.clone(),
            self.year.map(|y| y.to_string()),
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
    let corroborated =
        authors.score.is_some_and(|s| s >= CORROBORATING_AUTHORS) || ids == Agreement::Match;
    if !id_only && !corroborated {
        confidence = confidence.min(UNCORROBORATED_CAP);
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

/// Words marking a record as *about* another work rather than the work
/// itself: replies, corrections, errata, commentaries.
const DERIVATIVE_MARKERS: &[&str] = &[
    "reply",
    "erratum",
    "errata",
    "corrigendum",
    "correction",
    "comment on",
    "response to",
    "in response",
    "faculty opinions recommendation",
    "retraction of",
    "expression of concern",
];

/// Whether a title marks a derivative record: "…—Reply", "Correction to:
/// …", "Comment on “…”", "Faculty Opinions recommendation of …".
pub fn is_derivative(title: &str) -> bool {
    let words = format!(" {} ", normalise(title));
    DERIVATIVE_MARKERS
        .iter()
        .any(|m| words.contains(&format!(" {m} ")))
}

/// Whether the claim itself cites a derivative (its text or title uses a
/// marker), in which case derivatives are fair matches.
fn claim_mentions_derivative(claim: &Claim) -> bool {
    [claim.text.as_deref(), claim.title.as_deref()]
        .into_iter()
        .flatten()
        .any(is_derivative)
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

/// Lowercase ASCII-folded words: accents dropped, punctuation as spaces.
pub fn normalise(s: &str) -> String {
    let folded: String = s
        .nfkd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn words(s: &str) -> Vec<String> {
    normalise(s)
        .split(' ')
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A title without a leading status marker publishers add to retracted or
/// withdrawn works ("RETRACTED: Ileal-lymphoid…", "RETRACTED ARTICLE: …"),
/// so a citation of the original title still matches. Notices themselves
/// ("Retraction: …") are left alone.
pub fn unmarked(title: &str) -> &str {
    let trimmed = title.trim_start();
    for marker in ["retracted article", "retracted", "withdrawn"] {
        if let Some(head) = trimmed.get(..marker.len())
            && head.eq_ignore_ascii_case(marker)
        {
            let rest = &trimmed[marker.len()..];
            if let Some(rest) = rest.trim_start().strip_prefix([':', '.', '-', '—']) {
                return rest.trim_start();
            }
        }
    }
    title
}

/// Dice coefficient over the two titles' word sets: 1.0 for the same
/// words, lower as either title gains words the other lacks.
pub fn title_similarity(a: &str, b: &str) -> f64 {
    // Citations often drop a subtitle: compare main titles too.
    let (a, b) = (unmarked(a), unmarked(b));
    [(a, b), (main_title(a), b), (a, main_title(b))]
        .into_iter()
        .map(|(x, y)| dice(&content_words(x), &content_words(y)))
        .fold(0.0, f64::max)
}

fn dice(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let shared = a.iter().filter(|w| b.contains(w)).count();
    2.0 * shared as f64 / (a.len() + b.len()) as f64
}

/// Words too common to count as evidence that two titles agree.
const STOPWORDS: &[&str] = &[
    "a", "an", "the", "of", "in", "on", "for", "and", "or", "to", "with", "by", "at", "from", "is",
    "are", "as", "via", "into", "its",
];

/// A title's distinct words, without stopwords (unless that leaves none).
fn content_words(title: &str) -> Vec<String> {
    let all = dedup(words(title));
    let content: Vec<String> = all
        .iter()
        .filter(|w| !STOPWORDS.contains(&w.as_str()))
        .cloned()
        .collect();
    if content.is_empty() { all } else { content }
}

/// The title before a subtitle separator (":" or a dash), if there is one.
fn main_title(title: &str) -> &str {
    title
        .find([':', '\u{2014}', '\u{2013}'])
        .map_or(title, |i| title[..i].trim())
}

fn dedup(mut v: Vec<String>) -> Vec<String> {
    v.sort_unstable();
    v.dedup();
    v
}

/// A person's family name, normalised: the `family` field, or the last
/// word of a literal name.
fn family_name(p: &Person) -> Option<String> {
    let name = p.family.as_deref().or(p.literal.as_deref())?;
    let last = if p.family.is_some() {
        normalise(name)
    } else {
        words(name).pop()?
    };
    (!last.is_empty()).then_some(last)
}

/// The family name in a claimed author string: "Vaswani, A." → "vaswani",
/// "Ashish Vaswani" → "vaswani", "A. Vaswani" → "vaswani".
fn claimed_family(name: &str) -> Option<String> {
    let family = match name.split_once(',') {
        Some((before, _)) => normalise(before),
        // The last word that isn't an initial.
        None => words(name).into_iter().rfind(|w| w.chars().count() > 1)?,
    };
    (!family.is_empty()).then_some(family)
}

/// Claimed authors vs the record's family names: half for the first author
/// matching, half for the share of claimed authors found anywhere.
fn author_overlap(claimed: &[String], families: &[String]) -> f64 {
    let claimed: Vec<String> = claimed.iter().filter_map(|n| claimed_family(n)).collect();
    let Some(first) = claimed.first() else {
        return 0.0;
    };
    let first_matches = families.first() == Some(first);
    let found = claimed.iter().filter(|c| families.contains(c)).count();
    0.5 * f64::from(u8::from(first_matches)) + 0.5 * found as f64 / claimed.len() as f64
}

/// A reference string prepared for "is X mentioned?" questions.
struct Normalised {
    /// Space-padded, so phrase matches respect word boundaries.
    padded: String,
    words: Vec<String>,
}

impl Normalised {
    fn new(text: &str) -> Self {
        let norm = normalise(text);
        Normalised {
            words: norm.split(' ').map(str::to_owned).collect(),
            padded: format!(" {norm} "),
        }
    }

    /// 1.0 if the title appears as a phrase; otherwise the share of its
    /// words present.
    /// 1.0 if the title (or its main title, without subtitle) appears as a
    /// phrase; otherwise the share of its content words present. Under two
    /// thirds present is scored as a mismatch: a couple of topic words
    /// ("septic shock") shared with a reference are not a title match.
    fn title_coverage(&self, title: &str) -> f64 {
        let title = unmarked(title);
        let phrase = |t: &str| {
            let norm = normalise(t);
            !norm.is_empty() && self.padded.contains(&format!(" {norm} "))
        };
        if phrase(title) || (main_title(title) != title && phrase(main_title(title))) {
            return 1.0;
        }
        let title_words = content_words(title);
        if title_words.is_empty() {
            return 0.0;
        }
        let present = title_words
            .iter()
            .filter(|w| self.words.contains(w))
            .count();
        let share = present as f64 / title_words.len() as f64;
        if share < 2.0 / 3.0 {
            share / 2.0
        } else {
            share
        }
    }

    /// Citation styles routinely list only the first author or two ("Vaswani
    /// et al."), so the first author's family name appearing is a match on
    /// its own (0.9), with the next authors a small bonus. Without the first
    /// author, later names count for at most half.
    fn author_coverage(&self, families: &[String]) -> f64 {
        let present = |f: &String| self.padded.contains(&format!(" {f} "));
        let Some((first, rest)) = families.split_first() else {
            return 0.0;
        };
        let rest = &rest[..rest.len().min(AUTHORS_COMPARED - 1)];
        let rest_present = rest.iter().filter(|f| present(f)).count();
        match (present(first), rest.len()) {
            (true, 0) => 1.0,
            (true, n) => 0.9 + 0.1 * rest_present as f64 / n as f64,
            // A sole author who isn't mentioned is no match at all.
            (false, 0) => 0.0,
            (false, n) => 0.5 * rest_present as f64 / n as f64,
        }
    }

    /// Four-digit numbers that look like publication years.
    fn years(&self) -> Vec<i16> {
        self.words
            .iter()
            .filter(|w| w.len() == 4)
            .filter_map(|w| w.parse::<i16>().ok())
            .filter(|y| (1600..=2100).contains(y))
            .collect()
    }
}

/// The longest run of words a candidate's title shares with a reference,
/// as a search query for the title the reference actually cites. Uses the
/// candidate as a guide to find the title inside the reference without
/// parsing citation styles: for a re-deposit it is the whole title; for
/// "Faculty Opinions recommendation of *Highly accurate … AlphaFold*" it is
/// just the cited title. `None` if the run is too short to be a title (under
/// three words, unless it is the candidate's whole title).
pub fn shared_title(candidate_title: &str, reference: &str) -> Option<String> {
    let (title, text) = (words(unmarked(candidate_title)), words(reference));
    // Longest common substring over words; references are short.
    let mut best = (0, 0); // (length, end index in title)
    let mut prev = vec![0usize; text.len() + 1];
    for (i, tw) in title.iter().enumerate() {
        let mut cur = vec![0usize; text.len() + 1];
        for (j, rw) in text.iter().enumerate() {
            if tw == rw {
                cur[j + 1] = prev[j] + 1;
                if cur[j + 1] > best.0 {
                    best = (cur[j + 1], i + 1);
                }
            }
        }
        prev = cur;
    }
    let (len, end) = best;
    if len == 0 || (len < 3 && len < title.len()) {
        return None;
    }
    Some(title[end - len..end].join(" "))
}

/// Keywords for a plain search engine from a messy reference: years,
/// numbers, initials and "et al." dropped, since they dilute keyword
/// ranking (observed with OpenAlex).
pub fn search_terms(text: &str) -> String {
    words(text)
        .into_iter()
        .filter(|w| w.chars().count() > 1)
        .filter(|w| !w.chars().all(|c| c.is_ascii_digit()))
        .filter(|w| !matches!(w.as_str(), "et" | "al" | "doi" | "vol" | "pp" | "no"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// DOIs and arXiv IDs mentioned in free text: `doi:…`, `https://doi.org/…`,
/// bare `10.…/…`, `arXiv:…` and arxiv.org URLs. Surrounding brackets,
/// quotes and sentence punctuation are ignored.
pub fn find_ids(text: &str) -> Vec<WorkId> {
    let mut found: Vec<WorkId> = Vec::new();
    // An ISBN only counts when labelled ("ISBN 978-…"): in prose, a bare
    // run of digits could be anything.
    let mut isbn_label = false;
    for token in text.split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'')) {
        let token = token
            .trim_start_matches(['(', '[', '{'])
            .trim_end_matches(['.', ',', ';', ')', ']', '}']);
        let lower = token.to_ascii_lowercase();
        let labelled = std::mem::replace(
            &mut isbn_label,
            matches!(
                lower.trim_end_matches(':'),
                "isbn" | "isbn-13" | "isbn-10" | "isbn13" | "isbn10"
            ),
        );
        let token = token.trim_end_matches(':');
        let candidate = if labelled || lower.starts_with("isbn") {
            WorkId::parse_as(crate::id::IdKind::Isbn, token).ok()
        } else if let Some(at) = lower.find("10.") {
            // A DOI, possibly inside a URL or after "doi:".
            WorkId::parse_as(crate::id::IdKind::Doi, &token[at..]).ok()
        } else if lower.starts_with("arxiv:") || lower.contains("arxiv.org/") {
            token
                .split_once(':')
                .filter(|_| lower.starts_with("arxiv:"))
                .map_or(token, |(_, rest)| rest)
                .parse::<ArxivId>()
                .ok()
                .map(WorkId::Arxiv)
        } else {
            None
        };
        if let Some(id) = candidate
            && !found.iter().any(|f| same_work_id(f, &id))
        {
            found.push(id);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;

    use super::*;
    use crate::provider::ProviderId;
    use crate::record::{FieldSet, Raw};
    use crate::work::{PartialDate, Work};

    fn record(ids: &[&str], title: &str, authors: &[&str], year: i16) -> SourceRecord {
        let work = Work {
            title: Some(title.into()),
            authors: authors.iter().map(|a| Person::literal(*a)).collect(),
            issued: Some(PartialDate::from_year(year)),
            ..Work::default()
        };
        let raw = Raw {
            media_type: "application/json".into(),
            body: "{}".into(),
        };
        SourceRecord::new(
            ProviderId::from_static("test"),
            ids.iter().map(|i| i.parse().unwrap()).collect(),
            work,
            FieldSet::ALL,
            raw,
            Timestamp::UNIX_EPOCH,
        )
    }

    const VASWANI: [&str; 3] = ["Ashish Vaswani", "Noam Shazeer", "Niki Parmar"];

    fn attention(ids: &[&str], year: i16) -> SourceRecord {
        record(ids, "Attention Is All You Need", &VASWANI, year)
    }

    #[test]
    fn the_real_paper_verifies_and_the_lookalike_does_not() {
        let claim = Claim::from_text("Vaswani 2017 Attention is all you need");
        let real = assess(&claim, &attention(&["arXiv:1706.03762"], 2017));
        let lookalike = assess(&claim, &attention(&["10.65215/r5bs2d54"], 2025));

        assert!(real.confidence >= VERIFIED, "{real:?}");
        assert_eq!(real.title.agreement, Agreement::Match);
        assert_eq!(lookalike.year.agreement, Agreement::Mismatch);
        assert!(lookalike.confidence < PLAUSIBLE, "{lookalike:?}");
    }

    #[test]
    fn a_messy_reference_matches_its_work_not_its_namesakes() {
        let claim = Claim::from_text(
            "LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015;521:436-444.",
        );
        let authors = ["Yann LeCun", "Yoshua Bengio", "Geoffrey Hinton"];
        let right = assess(
            &claim,
            &record(&["10.1038/nature14539"], "Deep learning", &authors, 2015),
        );
        let later = assess(
            &claim,
            &record(&["10.1145/3448250"], "Deep learning for AI", &authors, 2021),
        );
        assert!(right.confidence >= VERIFIED, "{right:?}");
        assert!(later.confidence < PLAUSIBLE, "{later:?}");
    }

    #[test]
    fn structured_claims_compare_field_by_field() {
        let mut claim = Claim {
            title: Some("Attention is all you need".into()),
            authors: vec!["Vaswani, A.".into(), "N. Shazeer".into()],
            year: Some(2017),
            ..Claim::default()
        };
        let a = assess(&claim, &attention(&[], 2017));
        assert_eq!(a.authors.agreement, Agreement::Match);
        assert!(a.confidence >= VERIFIED);

        claim.year = Some(2018);
        let off_by_one = assess(&claim, &attention(&[], 2017));
        assert_eq!(off_by_one.year.agreement, Agreement::Partial);
        assert!(off_by_one.confidence >= PLAUSIBLE);
    }

    #[test]
    fn first_author_alone_matches_but_a_missing_first_author_does_not() {
        let families = [
            "vaswani".to_owned(),
            "shazeer".to_owned(),
            "parmar".to_owned(),
        ];
        let etal = Normalised::new("Vaswani et al. Attention is all you need");
        let others = Normalised::new("Shazeer N, Parmar N. Attention is all you need");
        assert_eq!(
            FieldCheck::scored(etal.author_coverage(&families)).agreement,
            Agreement::Match
        );
        assert_eq!(
            FieldCheck::scored(others.author_coverage(&families)).agreement,
            Agreement::Partial
        );
        let sole = ["anfinsen".to_owned()];
        assert_eq!(
            etal.author_coverage(&sole),
            0.0,
            "an unmentioned sole author"
        );
    }

    #[test]
    fn any_edition_year_counts_for_books() {
        let mut first_edition = record(
            &[],
            "Reinforcement Learning: An Introduction",
            &["Richard S. Sutton", "Andrew G. Barto"],
            1998,
        );
        let claim = Claim::from_text(
            "Sutton RS, Barto AG. Reinforcement Learning: An Introduction. 2nd ed. MIT Press; 2018.",
        );
        assert_eq!(
            assess(&claim, &first_edition).year.agreement,
            Agreement::Mismatch
        );
        first_edition.work.other_years = vec![2018];
        let a = assess(&claim, &first_edition);
        assert_eq!(a.year.agreement, Agreement::Match);
        assert!(a.confidence >= VERIFIED, "{a:?}");
    }

    #[test]
    fn different_authors_are_penalised() {
        let claim =
            Claim::from_text("Sutton RS, Barto AG. Reinforcement Learning: An Introduction. 2018.");
        let other = record(
            &[],
            "An Introduction to Deep Reinforcement Learning",
            &["Vincent Francois-Lavet", "Peter Henderson"],
            2018,
        );
        let a = assess(&claim, &other);
        assert_eq!(a.authors.agreement, Agreement::Mismatch);
        assert!(a.confidence < PLAUSIBLE, "{a:?}");
    }

    #[test]
    fn nearer_years_score_higher_even_when_both_are_wrong() {
        let claim =
            Claim::from_text("Vaswani A, Shazeer N. Attention is all you need. NeurIPS 2019.");
        let real = assess(&claim, &attention(&[], 2017));
        let lookalike = assess(&claim, &attention(&[], 2025));
        assert_eq!(real.year.agreement, Agreement::Mismatch);
        assert!(real.confidence > lookalike.confidence);
    }

    #[test]
    fn a_title_alone_cannot_verify() {
        let claim = Claim {
            title: Some("Deep learning".into()),
            ..Claim::default()
        };
        let a = assess(&claim, &record(&[], "Deep learning", &[], 2015));
        assert_eq!(a.title.agreement, Agreement::Match);
        assert!(a.confidence <= SINGLE_FIELD_CAP);
    }

    #[test]
    fn accents_and_punctuation_do_not_matter() {
        assert_eq!(
            normalise("Schölkopf, B. — Kernel‐Methods!"),
            "scholkopf b kernel methods"
        );
        assert_eq!(
            title_similarity("Attention Is All You Need", "attention is all you need."),
            1.0
        );
        let partial = title_similarity(
            "Attention Is All You Need",
            "Attention Is All You Need In Speech Separation",
        );
        assert!((0.5..0.9).contains(&partial), "{partial}");
    }

    #[test]
    fn claimed_ids_are_checked_against_the_record() {
        let claim = Claim::from_text("Deep learning. Nature 2015. doi:10.1038/nature14539.");
        assert_eq!(
            claim.ids,
            [WorkId::parse_as(crate::IdKind::Doi, "10.1038/nature14539").unwrap()]
        );
        let right = assess(
            &claim,
            &record(&["10.1038/nature14539"], "Deep learning", &[], 2015),
        );
        let wrong = assess(
            &claim,
            &record(&["10.1145/3448250"], "Deep learning for AI", &[], 2021),
        );
        assert_eq!(right.ids, Agreement::Match);
        assert_eq!(wrong.ids, Agreement::Mismatch);
    }

    #[test]
    fn input_is_an_identifier_or_a_reference() {
        let id_only = Claim::from_input(" 10.1038/nature14539 ");
        assert!(id_only.text.is_none() && id_only.ids.len() == 1);
        let reference = Claim::from_input("Deep learning. Nature 2015. doi:10.1038/nature14539");
        assert!(reference.text.is_some() && reference.ids.len() == 1);
    }

    #[test]
    fn shared_title_finds_the_cited_title_inside_a_derivative_one() {
        let reference = "Jumper J, et al. Highly accurate protein structure prediction with AlphaFold. Nature. 2021.";
        assert_eq!(
            shared_title(
                "Faculty Opinions recommendation of Highly accurate protein structure prediction with AlphaFold.",
                reference
            )
            .as_deref(),
            Some("highly accurate protein structure prediction with alphafold")
        );
        assert_eq!(
            shared_title("Deep learning", "LeCun Y. Deep learning. Nature 2015").as_deref(),
            Some("deep learning"),
            "a short title counts when whole"
        );
        assert_eq!(
            shared_title("Deep learning for AI", "Smith J. On learning. 2020"),
            None
        );
    }

    #[test]
    fn retraction_markers_do_not_hide_a_title_match() {
        assert_eq!(
            unmarked("RETRACTED: Ileal-lymphoid hyperplasia"),
            "Ileal-lymphoid hyperplasia"
        );
        assert_eq!(unmarked("Retracted Article - Something"), "Something");
        assert_eq!(unmarked("Retraction: Something"), "Retraction: Something");
        assert_eq!(
            unmarked("Retracted proteins in yeast"),
            "Retracted proteins in yeast"
        );
        assert_eq!(
            title_similarity(
                "RETRACTED: Ileal-lymphoid hyperplasia",
                "Ileal-lymphoid hyperplasia"
            ),
            1.0
        );
    }

    #[test]
    fn title_and_year_alone_cannot_verify() {
        // Dogfood: a generic title contained in the reference, no authors.
        let claim = Claim::from_text(
            "3CPO (NEJM 2008; Health Technol Assess 2009). New England Journal of Medicine; Health Technology Assessment 2009 2008",
        );
        let a = assess(
            &claim,
            &record(&[], "Health technology assessment", &[], 2009),
        );
        assert_eq!(a.title.agreement, Agreement::Match);
        assert!(a.confidence < VERIFIED, "{a:?}");

        // An unauthored letter with the same title loses to the article.
        let claim = Claim {
            text: Some(
                "Fibrinolysis for patients with intermediate-risk pulmonary embolism".into(),
            ),
            authors: vec!["Meyer".into()],
            year: Some(2014),
            ..Claim::default()
        };
        let letter = assess(
            &claim,
            &record(
                &[],
                "Fibrinolysis for Intermediate-Risk Pulmonary Embolism",
                &[],
                2014,
            ),
        );
        let article = assess(
            &claim,
            &record(
                &[],
                "Fibrinolysis for Patients with Intermediate-Risk Pulmonary Embolism",
                &["Guy Meyer"],
                2014,
            ),
        );
        assert!(letter.confidence < VERIFIED && article.confidence >= VERIFIED);
    }

    #[test]
    fn replies_and_corrections_are_not_the_original() {
        assert!(is_derivative(
            "Age-Adjusted D-Dimer Cutoff Levels and Pulmonary Embolism—Reply"
        ));
        assert!(is_derivative("Correction to: Attention is all you need"));
        assert!(is_derivative(
            "Comment on “Fibrinolysis for patients with intermediate-risk PE”"
        ));
        assert!(!is_derivative(
            "Age-Adjusted D-Dimer Cutoff Levels to Rule Out Pulmonary Embolism"
        ));

        let claim = Claim {
            title: Some("Age-adjusted D-dimer cutoff levels to rule out pulmonary embolism".into()),
            authors: vec!["Righini".into()],
            year: Some(2014),
            ..Claim::default()
        };
        let reply = assess(
            &claim,
            &record(
                &[],
                "Age-Adjusted D-Dimer Cutoff Levels and Pulmonary Embolism—Reply",
                &["Marc Righini"],
                2014,
            ),
        );
        assert!(reply.confidence < PLAUSIBLE, "{reply:?}");
        // Citing the reply itself is fine.
        let citing_reply = Claim {
            title: Some("Age-adjusted D-dimer cutoff levels and pulmonary embolism—reply".into()),
            ..claim
        };
        let a = assess(
            &citing_reply,
            &record(
                &[],
                "Age-Adjusted D-Dimer Cutoff Levels and Pulmonary Embolism—Reply",
                &["Marc Righini"],
                2014,
            ),
        );
        assert!(a.confidence >= VERIFIED, "{a:?}");
    }

    #[test]
    fn a_few_shared_topic_words_are_not_a_title_match() {
        // Dogfood control: a fabricated citation sharing "septic shock".
        let claim = Claim::from_text("Smith 2019 Ketamine reduces mortality in septic shock");
        let a = assess(
            &claim,
            &record(
                &[],
                "Leukotrienes in Endotoxic and Septic Shock",
                &["J Smith"],
                2019,
            ),
        );
        assert_eq!(a.title.agreement, Agreement::Mismatch);
        assert!(a.confidence < PLAUSIBLE, "{a:?}");
    }

    #[test]
    fn dropped_subtitles_still_match() {
        let structured = title_similarity(
            "Age-adjusted D-dimer cutoff levels to rule out pulmonary embolism",
            "Age-Adjusted D-Dimer Cutoff Levels to Rule Out Pulmonary Embolism: The ADJUST-PE Study",
        );
        assert_eq!(structured, 1.0);
        let claim = Claim::from_text(
            "Righini M et al. Age-adjusted D-dimer cutoff levels to rule out pulmonary embolism. JAMA 2014",
        );
        let a = assess(
            &claim,
            &record(
                &[],
                "Age-Adjusted D-Dimer Cutoff Levels to Rule Out Pulmonary Embolism: The ADJUST-PE Study",
                &["Marc Righini"],
                2014,
            ),
        );
        assert_eq!(a.title.agreement, Agreement::Match);
    }

    #[test]
    fn search_terms_drop_noise() {
        assert_eq!(
            search_terms("LeCun Y, Bengio Y, et al. Deep learning. Nature. 2015;521:436-444."),
            "lecun bengio deep learning nature"
        );
    }

    #[test]
    fn finds_ids_in_running_text() {
        let text = "See (https://doi.org/10.1145/3368089.3409741), arXiv:1706.03762v5 and \
                    https://arxiv.org/abs/hep-th/9901001; also arXiv:1706.03762.";
        let ids: Vec<String> = find_ids(text).iter().map(ToString::to_string).collect();
        assert_eq!(
            ids,
            [
                "doi:10.1145/3368089.3409741",
                "arxiv:1706.03762v5",
                "arxiv:hep-th/9901001"
            ]
        );
        let book = "Goodfellow I, et al. Deep Learning. MIT Press; 2016. ISBN 978-0-262-03561-3. Also ISBN-10: 0262035618.";
        let ids: Vec<String> = find_ids(book).iter().map(ToString::to_string).collect();
        assert_eq!(ids, ["isbn:9780262035613"], "both forms are one book");
        assert!(
            find_ids("pages 9780262035613 and 2016").is_empty(),
            "unlabelled digits ignored"
        );
        assert!(find_ids("Published in 2017, pages 10-20.").is_empty());
    }
}
