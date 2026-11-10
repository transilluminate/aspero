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
/// Confidence ceiling when fewer than two fields could be compared.
const SINGLE_FIELD_CAP: f64 = 0.7;

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

    let actual_year = work.issued.map(|d| d.year());
    let claimed_years: Vec<i16> = match (claim.year, &text) {
        (Some(y), _) => vec![y],
        (None, Some(text)) => text.years(),
        _ => Vec::new(),
    };
    let year = match actual_year {
        Some(actual) if !claimed_years.is_empty() => {
            let delta = claimed_years
                .iter()
                .map(|y| (y - actual).unsigned_abs())
                .min()
                .unwrap_or(u16::MAX);
            // Halves per year off: off by one is common (online-first vs
            // print, preprint vs published) and counts as partial; two or
            // more is a mismatch, but a nearer year still ranks higher.
            FieldCheck::scored(0.5f64.powi(i32::from(delta.min(30))))
        }
        _ => FieldCheck::UNKNOWN,
    };

    let ids = id_agreement(&claim.ids, &record.ids);
    // A claim of identifiers alone is fully confirmed by a matching ID.
    let id_only = [title, authors, year].iter().all(|c| c.score.is_none());
    let confidence = if id_only && ids == Agreement::Match {
        1.0
    } else {
        combine(title, authors, year)
    };

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
    let (a, b) = (dedup(words(unmarked(a))), dedup(words(unmarked(b))));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let shared = a.iter().filter(|w| b.contains(w)).count();
    2.0 * shared as f64 / (a.len() + b.len()) as f64
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
    fn title_coverage(&self, title: &str) -> f64 {
        let title = unmarked(title);
        let norm = normalise(title);
        if norm.is_empty() {
            return 0.0;
        }
        if self.padded.contains(&format!(" {norm} ")) {
            return 1.0;
        }
        let title_words = dedup(words(title));
        let present = title_words
            .iter()
            .filter(|w| self.words.contains(w))
            .count();
        present as f64 / title_words.len() as f64
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
