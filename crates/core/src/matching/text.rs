//! Text comparison: normalising, titles, and looking for a record's fields
//! inside a free-text reference.

use std::ops::Range;

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

/// [`normalise`]'s words, each with where it stands in `s` (a byte
/// range), so what surrounds a word can be read as written.
pub(super) fn spanned_words(s: &str) -> Vec<(String, Range<usize>)> {
    let mut found: Vec<(String, Range<usize>)> = Vec::new();
    let mut open = false;
    for (at, c) in s.char_indices() {
        let folded: String = c
            .nfkd()
            .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
            .flat_map(char::to_lowercase)
            .collect();
        let end = at + c.len_utf8();
        for f in folded.chars() {
            match found.last_mut() {
                _ if !f.is_alphanumeric() => open = false,
                Some((word, span)) if open => {
                    word.push(f);
                    span.end = end;
                }
                _ => {
                    found.push((f.to_string(), at..end));
                    open = true;
                }
            }
        }
    }
    found
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

/// Whether a claimed title is a record's with words added: every content
/// word of the record's, and more ("Deep learning: a primer for
/// clinicians" for "Deep learning"). Not a dropped subtitle, which is the
/// other way round; nor, with `subtitle_ok`, a subtitle added to the
/// record's whole title.
pub(super) fn extends(claimed: &str, actual: &str, subtitle_ok: bool) -> bool {
    let (claimed, actual) = (unmarked(claimed), unmarked(actual));
    if subtitle_ok
        && main_title(claimed) != claimed
        && dedup(content_words(main_title(claimed))) == dedup(content_words(actual))
    {
        return false;
    }
    let (claimed, actual) = (content_words(claimed), content_words(actual));
    !actual.is_empty() && claimed.len() > actual.len() && actual.iter().all(|w| claimed.contains(w))
}

/// [`title_similarity`] of the whole titles, subtitles included: of two
/// works whose main title is the one cited, the one whose subtitle is too
/// ("Treatment of Acute Ischemic Stroke: A Review" over "…: Open Trial
/// With Continuous Intravenous Heparinization").
pub fn whole_title_similarity(a: &str, b: &str) -> f64 {
    dice(&content_words(unmarked(a)), &content_words(unmarked(b)))
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

/// A reference's furniture: words around its fields, not in them.
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
    "pmid",
    "pmcid",
    "arxiv",
    "isbn",
];

/// Words naming where a work appeared rather than continuing its title:
/// "… supervision In Proceedings of the 38th Conference".
const VENUE_WORDS: &[&str] = &[
    "proceedings",
    "proc",
    "conference",
    "symposium",
    "workshop",
    "journal",
    "preprint",
];

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
    /// The reference as written.
    pub(super) raw: String,
    /// Space-padded, so phrase matches respect word boundaries.
    padded: String,
    pub(super) words: Vec<String>,
    /// Where each word stands in `raw`.
    pub(super) spans: Vec<Range<usize>>,
    /// Which words are inside a link or identifier ("10.1113/jphysiol.
    /// 1974.sp010694"): no year, journal or name is cited there.
    linked: Vec<bool>,
}

/// Where a reference's links and identifiers stand: its tokens that are a
/// URL or a DOI, PMID or arXiv ID, written out or labelled.
fn link_spans(raw: &str) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut at = 0;
    for token in raw.split_whitespace() {
        let start = at + raw[at..].find(token).unwrap_or(0);
        at = start + token.len();
        let t = token
            .trim_start_matches(['(', '[', '{', '<', '"', '\''])
            .to_ascii_lowercase();
        let link = [
            "http", "www.", "doi:", "doi.org", "dx.doi", "arxiv:", "pmid:", "pmcid:",
        ]
        .iter()
        .any(|p| t.starts_with(p))
            || (t.starts_with("10.") && t.contains('/'));
        if link {
            spans.push(start..at);
        }
    }
    spans
}

impl Normalised {
    pub(super) fn new(text: &str) -> Self {
        let (words, spans): (Vec<String>, Vec<Range<usize>>) =
            spanned_words(text).into_iter().unzip();
        let links = link_spans(text);
        let linked = spans
            .iter()
            .map(|w| links.iter().any(|l| l.start <= w.start && w.end <= l.end))
            .collect();
        Normalised {
            raw: text.to_owned(),
            padded: format!(" {} ", words.join(" ")),
            words,
            spans,
            linked,
        }
    }

    /// The words outside the reference's links and identifiers.
    pub(super) fn words_outside_links(&self) -> Vec<String> {
        self.words
            .iter()
            .zip(&self.linked)
            .filter(|(_, linked)| !**linked)
            .map(|(w, _)| w.clone())
            .collect()
    }

    /// The words (as indices into `words`) where `title` (or its main
    /// title, without subtitle) stands as a phrase, first found.
    pub(super) fn title_span(&self, title: &str) -> Option<Range<usize>> {
        let title = unmarked(title);
        [title, main_title(title)].into_iter().find_map(|t| {
            let phrase = words(t);
            if phrase.is_empty() {
                return None;
            }
            self.words
                .windows(phrase.len())
                .position(|run| run == phrase.as_slice())
                .map(|at| at..at + phrase.len())
        })
    }

    /// Whether the reference gives `title` with a word or two put in place
    /// of its own, not misspelt ("Hydroxychloroquine in hospitalized
    /// patients with Covid-19" for "Dexamethasone in …"): another title
    /// built on this one's, as an invented reference's often is.
    /// `Some(true)` when a year was put in place of the title's own
    /// ("Cancer statistics, 2020" for "Cancer statistics, 2018"): another
    /// title of a series.
    pub(super) fn substitutes_in(&self, title: &str) -> Option<bool> {
        let title = words(unmarked(title));
        let n = title.len();
        if n < 3 || self.words.len() < n {
            return None;
        }
        let year = |w: &str| w.len() == 4 && w.parse().is_ok_and(|y| PLAUSIBLE_YEARS.contains(&y));
        self.words.windows(n).enumerate().find_map(|(at, run)| {
            let same = |k: usize| run[k] == title[k];
            // Each word put in place stands in a sentence with one of the
            // title's own beside it: not the journal after the title's
            // stop ("… septic shock. JAMA").
            let placed = (0..n).filter(|&k| !same(k)).all(|k| {
                (k > 0 && same(k - 1) && self.one_sentence(at + k - 1, at + k))
                    || (k + 1 < n && same(k + 1) && self.one_sentence(at + k, at + k + 1))
            });
            let differ: Vec<(&String, &String)> =
                run.iter().zip(&title).filter(|(a, b)| a != b).collect();
            let substituted = !differ.is_empty()
                && placed
                && differ.len() * 4 <= n.max(4)
                && differ.iter().all(|(a, b)| {
                    !super::authors::near(a, b)
                        && !STOPWORDS.contains(&a.as_str())
                        && !STOPWORDS.contains(&b.as_str())
                });
            substituted.then(|| differ.iter().any(|(a, b)| year(a) && year(b)))
        })
    }

    /// Whether the words `from..=to` stand in one sentence: no stop between
    /// them ("… septic shock. JAMA" runs into the journal).
    fn one_sentence(&self, from: usize, to: usize) -> bool {
        (from..to).all(|i| {
            let gap = &self.raw[self.spans[i].end..self.spans[i + 1].start];
            !gap.contains(['.', '?', '!', ';'])
        })
    }

    /// Whether the reference gives `title`'s words in order with words of
    /// its own among them ("… for Sepsis and Septic Shock in Adults and
    /// Children (Sepsis-3)" for "… Septic Shock (Sepsis-3)"): another
    /// title. Joining words inserted don't count, nor words dropped (a
    /// title shortened is the same title).
    pub(super) fn inserts_in(&self, title: &str) -> bool {
        /// Words inserted, at most, before the title is taken as not there.
        const GAP: usize = 8;
        let title = words(unmarked(title));
        if content_words(&title.join(" ")).len() < super::SHORT_TITLE_WORDS {
            return false;
        }
        (0..self.words.len())
            .filter(|&i| self.words[i] == title[0])
            .any(|start| {
                let (mut j, mut inserted, mut end) = (1, 0, start);
                for (k, word) in self.words.iter().enumerate().skip(start + 1) {
                    if j == title.len() || inserted > GAP {
                        break;
                    }
                    if *word == title[j] {
                        j += 1;
                        end = k;
                    } else if !STOPWORDS.contains(&word.as_str()) {
                        inserted += 1;
                    }
                }
                j == title.len() && (1..=GAP).contains(&inserted) && self.one_sentence(start, end)
            })
    }

    /// The words from `from` on that run together as written: joined by
    /// spaces or a word's own hyphens and apostrophes, up to punctuation.
    fn run_from(&self, from: usize) -> Range<usize> {
        let mut end = from;
        while end < self.words.len() {
            let joined = end == from
                || self.raw[self.spans[end - 1].end..self.spans[end].start]
                    .chars()
                    .all(|c| c.is_whitespace() || matches!(c, '-' | '‐' | '\'' | '’'));
            if !joined {
                break;
            }
            end += 1;
        }
        from..end
    }

    /// Whether the claim's title is the record's (found at `span`) with more
    /// words: carried on in the same sentence ("Deep learning for medical
    /// image analysis", "… with AlphaFold enables drug discovery"), given a
    /// subtitle the record's lacks or another ("Deep learning: a primer"),
    /// or with a word before it ("Colorectal cancer statistics"). A title
    /// is the record's only up to punctuation, a capitalised word (a
    /// venue's, in a reference without stops: "Attention is all you need
    /// ICML 2017"), or the journal and year ("… in Nature 2015"); a word
    /// before it may be its authors' (`names`) or a joining word ("The").
    /// With `subtitle_ok`, a subtitle after the record's whole title is
    /// not more words.
    pub(super) fn extends_title(
        &self,
        span: &Range<usize>,
        names: &[String],
        journal: Option<&[String]>,
        subtitle_ok: bool,
    ) -> bool {
        let significant = |run: Range<usize>| {
            self.words[run].iter().any(|w| {
                !STOPWORDS.contains(&w.as_str())
                    && !LINK_WORDS.contains(&w.as_str())
                    && !FURNITURE.contains(&w.as_str())
                    && !VENUE_WORDS.contains(&w.as_str())
                    && !w.chars().all(|c| c.is_ascii_digit())
                    && !crate::publication::is_month(w)
                    && !journal.is_some_and(|j| crate::publication::journal_word(w, j))
            })
        };
        let after = |i: usize| {
            let Some(next) = self.words.get(i) else {
                return false;
            };
            let gap = &self.raw[self.spans[i - 1].end..self.spans[i].start];
            let gap = gap.trim_start_matches([')', ']']).trim();
            let raw = &self.raw[self.spans[i].clone()];
            let continues = match gap {
                "" => {
                    STOPWORDS.contains(&next.as_str())
                        || raw.chars().next().is_some_and(char::is_lowercase)
                }
                ":" | "—" | "–" | "-" => {
                    !subtitle_ok && raw.chars().next().is_some_and(char::is_alphabetic)
                }
                _ => false,
            };
            continues && significant(self.run_from(i))
        };
        let before = |i: usize| {
            let gap = &self.raw[self.spans[i - 1].end..self.spans[i].start];
            let word = &self.words[i - 1];
            let raw = &self.raw[self.spans[i - 1].clone()];
            gap.trim().is_empty()
                && word.chars().count() > 1
                && raw.chars().any(char::is_lowercase)
                && !matches!(word.as_str(), "et" | "al")
                && !STOPWORDS.contains(&word.as_str())
                && !names.contains(word)
                && significant(i - 1..i)
        };
        after(span.end) || (span.start > 0 && before(span.start))
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
        self.unaccounted(families, journal).iter().all(|u| !u)
    }

    /// The longest run of words, joining words aside, that aren't
    /// accounted for as [`Normalised::untitled`] accounts for them: three
    /// or more are a title ("Transfection of plasmid DNA by nanocarriers"),
    /// not a journal's words the rule misses ("Int. J. Care Injured").
    pub(super) fn title_run(&self, families: &[String], journal: Option<&str>) -> usize {
        let mut longest = 0;
        let mut run = 0;
        for (word, unaccounted) in self.words.iter().zip(self.unaccounted(families, journal)) {
            if unaccounted {
                run += 1;
                longest = longest.max(run);
            } else if !STOPWORDS.contains(&word.as_str()) {
                run = 0;
            }
        }
        longest
    }

    /// For each word, whether it is none of the candidate's authors
    /// (`families`, normalised), journal, an initial, a number, a link's
    /// word or a reference's furniture.
    fn unaccounted(&self, families: &[String], journal: Option<&str>) -> Vec<bool> {
        let author_words: Vec<&str> = families.iter().flat_map(|f| f.split(' ')).collect();
        let journal = journal.map(crate::publication::significant);
        self.words
            .iter()
            .map(|w| {
                !(w.chars().count() <= 2
                    || w.bytes().any(|b| b.is_ascii_digit())
                    || STOPWORDS.contains(&w.as_str())
                    || LINK_WORDS.contains(&w.as_str())
                    || FURNITURE.contains(&w.as_str())
                    || crate::publication::is_month(w)
                    || author_words.contains(&w.as_str())
                    || journal
                        .as_ref()
                        .is_some_and(|j| crate::publication::journal_word(w, j)))
            })
            .collect()
    }

    /// Four-digit numbers that look like publication years, but for those
    /// inside `title` where the reference gives it: "Cancer statistics,
    /// 2020" cites no year. Nor, when it gives another, does one inside
    /// the title the reference itself suggests ([`probable_title`]),
    /// whichever work it's compared with: "Cancer statistics, 2020. CA
    /// Cancer J Clin. 2018;70:7" cites 2018, for "Colorectal cancer
    /// statistics, 2020" too.
    pub(super) fn years_beside(&self, title: Option<&str>) -> Vec<i16> {
        let span = title.and_then(|t| self.title_span(t)).unwrap_or(0..0);
        let years = |skip: &Range<usize>| -> Vec<i16> {
            self.words
                .iter()
                .enumerate()
                .filter(|(i, w)| {
                    w.len() == 4 && !span.contains(i) && !skip.contains(i) && !self.linked[*i]
                })
                .filter_map(|(_, w)| w.parse::<i16>().ok())
                .filter(|y| PLAUSIBLE_YEARS.contains(y))
                .collect()
        };
        let suggested = probable_title(&self.raw)
            .and_then(|t| self.title_span(&t))
            .unwrap_or(0..0);
        match years(&suggested) {
            outside if !outside.is_empty() => outside,
            _ => years(&(0..0)),
        }
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

/// The title a reference probably cites, read from its shape, to search
/// for when no citation matcher's hit points to it: a BibTeX entry's
/// title, else the longest quoted span (Harvard's 'An image is worth
/// 16x16 words', IEEE's "…,"), else the sentence after the byline
/// ("Lundberg SM, Lee SI. A unified approach to interpreting model
/// predictions. Adv Neural …"). `None` when nothing looks like one.
pub fn probable_title(text: &str) -> Option<String> {
    let title = |s: &str| {
        let s = s
            .trim()
            .trim_matches(|c: char| c.is_whitespace() || ",.;:'\"“”‘’".contains(c));
        let wordy = s
            .split_whitespace()
            .any(|w| w.chars().filter(|c| c.is_alphabetic()).count() >= 3);
        (wordy && !s.starts_with("In:") && !s.starts_with("In ")).then(|| s.to_owned())
    };
    if let Some(t) = crate::publication::bibtex_field(text, "title") {
        return title(&t);
    }
    let quoted = [('"', '"'), ('“', '”'), ('‘', '’'), ('\'', '\'')]
        .iter()
        .filter_map(|&(open, close)| {
            let start = text
                .char_indices()
                .find(|&(i, c)| {
                    c == open
                        && text[..i]
                            .chars()
                            .next_back()
                            .is_none_or(char::is_whitespace)
                })?
                .0
                + open.len_utf8();
            let end = text[start..]
                .char_indices()
                .find(|&(i, c)| {
                    c == close
                        && text[start + i + c.len_utf8()..]
                            .chars()
                            .next()
                            .is_none_or(|n| n.is_whitespace() || ",.;".contains(n))
                })?
                .0;
            title(&text[start..start + end])
        })
        .max_by_key(|t| t.split_whitespace().count());
    if quoted.is_some() {
        return quoted;
    }
    // Of its sentences, not a byline, a publisher or locator (";"), the
    // one most in sentence case, then the longest.
    text.split(". ")
        .filter(|s| !s.contains(';') && !super::authors::names_given(s))
        .filter_map(title)
        .max_by_key(|s| {
            let words: Vec<&str> = s.split_whitespace().collect();
            let lower = words
                .iter()
                .filter(|w| w.chars().count() >= 3 && w.starts_with(char::is_lowercase))
                .count();
            (lower, words.len())
        })
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
