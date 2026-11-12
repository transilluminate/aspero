//! Text comparison: normalising, titles, and looking for a record's fields
//! inside a free-text reference.

use unicode_normalization::UnicodeNormalization;

use super::AUTHORS_COMPARED;
use crate::work::PLAUSIBLE_YEARS;

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

pub(super) fn words(s: &str) -> Vec<String> {
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

pub(super) fn dice(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let shared = a.iter().filter(|w| b.contains(w)).count();
    2.0 * shared as f64 / (a.len() + b.len()) as f64
}

/// Words too common to count as evidence that two titles agree.
pub(super) const STOPWORDS: &[&str] = &[
    "a", "an", "the", "of", "in", "on", "for", "and", "or", "to", "with", "by", "at", "from", "is",
    "are", "as", "via", "into", "its",
];

/// The words of a link written out (`https:// doi. org/ …`), which are no
/// evidence of a title.
const LINK_WORDS: &[&str] = &["http", "https", "www", "doi", "dx", "org", "com"];

/// A title's distinct words, without stopwords or link words (unless that
/// leaves none).
pub(super) fn content_words(title: &str) -> Vec<String> {
    let all = dedup(words(title));
    let content: Vec<String> = all
        .iter()
        .filter(|w| !STOPWORDS.contains(&w.as_str()) && !LINK_WORDS.contains(&w.as_str()))
        .cloned()
        .collect();
    if content.is_empty() { all } else { content }
}

/// The title before a subtitle separator (":" or a dash), if there is one.
pub(super) fn main_title(title: &str) -> &str {
    title
        .find([':', '\u{2014}', '\u{2013}'])
        .map_or(title, |i| title[..i].trim())
}

pub(super) fn dedup(mut v: Vec<String>) -> Vec<String> {
    v.sort_unstable();
    v.dedup();
    v
}

/// A reference string prepared for "is X mentioned?" questions.
pub(super) struct Normalised {
    /// Space-padded, so phrase matches respect word boundaries.
    padded: String,
    pub(super) words: Vec<String>,
}

impl Normalised {
    pub(super) fn new(text: &str) -> Self {
        let norm = normalise(text);
        Normalised {
            words: norm.split(' ').map(str::to_owned).collect(),
            padded: format!(" {norm} "),
        }
    }

    /// 1.0 if the title (or its main title, without subtitle) appears as a
    /// phrase; otherwise the share of its content words present. Under two
    /// thirds present is scored as a mismatch: a couple of topic words
    /// ("septic shock") shared with a reference are not a title match.
    pub(super) fn title_coverage(&self, title: &str) -> f64 {
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
    pub(super) fn author_coverage(&self, families: &[String]) -> f64 {
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

    /// Whether the reference gives no title: every word is accounted for
    /// by the candidate's authors (`families`, normalised) and journal,
    /// or is an initial, a number, a link's word or a reference's furniture
    /// ("et al", "epub", a month). "Wittermans E, et al. ERJ Open Res.
    /// 2022. doi:…" has none.
    pub(super) fn untitled(&self, families: &[String], journal: Option<&str>) -> bool {
        const FURNITURE: &[&str] = &[
            "doi",
            "epub",
            "ahead",
            "print",
            "online",
            "published",
            "accessed",
            "available",
            "vol",
            "pp",
            "suppl",
            "eds",
            "jan",
            "feb",
            "mar",
            "apr",
            "may",
            "jun",
            "jul",
            "aug",
            "sep",
            "sept",
            "oct",
            "nov",
            "dec",
            "january",
            "february",
            "march",
            "april",
            "june",
            "july",
            "august",
            "september",
            "october",
            "november",
            "december",
            "pmid",
            "pmcid",
            "arxiv",
            "isbn",
        ];
        let author_words: Vec<&str> = families.iter().flat_map(|f| f.split(' ')).collect();
        self.words.iter().filter(|w| !w.is_empty()).all(|w| {
            w.chars().count() <= 2
                || w.bytes().any(|b| b.is_ascii_digit())
                || STOPWORDS.contains(&w.as_str())
                || LINK_WORDS.contains(&w.as_str())
                || FURNITURE.contains(&w.as_str())
                || author_words.contains(&w.as_str())
                || journal.is_some_and(|j| crate::publication::journal_word(w, j))
        })
    }

    /// Four-digit numbers that look like publication years.
    pub(super) fn years(&self) -> Vec<i16> {
        self.words
            .iter()
            .filter(|w| w.len() == 4)
            .filter_map(|w| w.parse::<i16>().ok())
            .filter(|y| PLAUSIBLE_YEARS.contains(y))
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
