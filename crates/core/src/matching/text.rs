//! Free text as words (normalised, with their spans), and [`Normalised`]:
//! looking for a record's title, authors and years inside a free-text
//! reference.

use std::ops::Range;

use unicode_normalization::UnicodeNormalization;

use super::caps::{EXTENDED_TITLE, SHORT_TITLE_WORDS};
use super::titles::{
    first_sentence, main_title, opposite, probable_title, replaced, significant_word,
    significant_words, unmarked,
};
use crate::work::PLAUSIBLE_YEARS;

/// Authors compared from the start of a byline.
const AUTHORS_COMPARED: usize = 3;

/// Lowercase ASCII-folded words: accents dropped, punctuation as spaces.
pub fn normalise(s: &str) -> String {
    let folded: String = s
        .nfkd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .map(|c| if word_char(c) { c } else { ' ' })
        .collect();
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A character of a word: a letter or digit, but not the modifier letter
/// apostrophe some records print for an apostrophe ("womenʼs", holdout-5
/// #1074).
fn word_char(c: char) -> bool {
    c.is_alphanumeric() && c != '\u{2bc}'
}

/// [`normalise`]'s words, each with where it stands in `s` (a byte
/// range, in order and not overlapping), so what surrounds a word can be
/// read as written.
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
                _ if !word_char(f) => open = false,
                Some((word, span)) if open => {
                    word.push(f);
                    span.end = end;
                }
                // A word begun inside the character that ended the last one
                // ("½" folds to "1⁄2") starts where that word ends, so
                // spans stay in order.
                last => {
                    let start = last.map_or(at, |(_, span)| span.end.max(at));
                    found.push((f.to_string(), start..end));
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

/// A character of a script written without spaces between words: CJK
/// ideographs, kana.
fn unspaced(c: char) -> bool {
    matches!(c, '\u{3040}'..='\u{30ff}' | '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}')
}

pub(super) fn dedup(mut v: Vec<String>) -> Vec<String> {
    v.sort_unstable();
    v.dedup();
    v
}

/// Whether `title`, cut after its first `k` words, is cut where it has a
/// separator: a subtitle's colon or dash, a question's or sentence's end,
/// a bracket, a semicolon or a comma ("Does Money Matter?" of "Does Money
/// Matter? A Meta-Analysis …"): a part of it cited, as a subtitle dropped
/// is.
fn cut_at_separator(title: &str, k: usize) -> bool {
    let spans = spanned_words(title);
    let Some((_, span)) = spans.get(k.wrapping_sub(1)) else {
        return false;
    };
    let rest = title[span.end..].trim_start();
    rest.starts_with([':', '?', '!', '.', ',', ';', '(', '[', '—', '–']) || rest.starts_with("- ")
}

/// Whether the title cited (`claimed`, given apart, else found in `text`)
/// is `actual` cut off ([`Normalised::cut_off`]).
pub(super) fn title_cut(claimed: Option<&str>, text: Option<&Normalised>, actual: &str) -> bool {
    match (claimed, text) {
        (Some(claimed), _) => Normalised::new(claimed).cut_off(actual, true),
        (None, Some(text)) => text.cut_off(actual, false),
        _ => false,
    }
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

    /// The words outside the reference's links and identifiers, and
    /// outside `span` (a title's words) when given.
    pub(super) fn words_outside(&self, span: Option<&Range<usize>>) -> Vec<String> {
        self.words
            .iter()
            .zip(&self.linked)
            .enumerate()
            .filter(|(i, (_, linked))| !**linked && !span.is_some_and(|s| s.contains(i)))
            .map(|(_, (w, _))| w.clone())
            .collect()
    }

    /// The words (as indices into `words`) where `title` (or its first
    /// sentence, or its main title, without subtitle) stands as a phrase,
    /// first found.
    pub(super) fn title_span(&self, title: &str) -> Option<Range<usize>> {
        let title = unmarked(title);
        [title, first_sentence(title), main_title(title)]
            .into_iter()
            .find_map(|t| {
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

    /// [`Normalised::title_span`], an article ("the", "a", "an") given or
    /// left out on either side ("Quantifying Online News Media Coverage …"
    /// for "Quantifying the online news media coverage …").
    pub(super) fn loose_title_span(&self, title: &str) -> Option<Range<usize>> {
        let article = |w: &str| matches!(w, "the" | "a" | "an");
        let phrase: Vec<String> = words(unmarked(title))
            .into_iter()
            .filter(|w| !article(w))
            .collect();
        if phrase.len() < 3 {
            return None;
        }
        (0..self.words.len()).find_map(|start| {
            let mut k = 0;
            let mut i = start;
            while k < phrase.len() && i < self.words.len() {
                if self.words[i] == phrase[k] {
                    k += 1;
                } else if !article(&self.words[i]) || i == start {
                    return None;
                }
                i += 1;
            }
            (k == phrase.len()).then_some(start..i)
        })
    }

    /// Whether the reference gives `title` with a word or two put in place
    /// of its own, not misspelt ("Hydroxychloroquine in hospitalized
    /// patients with Covid-19" for "Dexamethasone in …"): another title
    /// built on this one's, as an invented reference's often is.
    /// `Some(true)` when a year was put in place of the title's own
    /// ("Cancer statistics, 2020" for "Cancer statistics, 2018"): another
    /// title of a series. A word put in place by its opposite counts,
    /// however near its spelling ("Southern" for "Northern").
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
                    (opposite(a, b) || !super::authors::near(a, b))
                        && !STOPWORDS.contains(&a.as_str())
                        && !STOPWORDS.contains(&b.as_str())
                });
            substituted.then(|| differ.iter().any(|(a, b)| year(a) && year(b)))
        })
    }

    /// Whether the reference gives `title` with significant words of its
    /// own put in place of the title's, anywhere in it and however many
    /// together ("Likelihood Determinants Associated With …" for "Risk
    /// Factors Associated With …", "Monte Carlo sampling approaches with
    /// Markov chains" for "… methods using Markov chains"): the title
    /// reworded, as [`super::titles::replaces`] tells for a title given apart. Words
    /// replaced stand in one sentence with the title's around them.
    pub(super) fn replaces_in(&self, title: &str) -> bool {
        let title = significant_words(&words(unmarked(title)));
        let (at, text): (Vec<usize>, Vec<String>) = self
            .words
            .iter()
            .enumerate()
            .filter(|(i, w)| !self.linked[*i] && significant_word(w))
            .map(|(i, w)| (i, w.clone()))
            .unzip();
        let together = |from: usize, to: usize, edge: bool| {
            (at[from]..at[to]).all(|i| {
                let gap = &self.raw[self.spans[i].end..self.spans[i + 1].start];
                // "vs." stops no sentence ("Pseudobinaural vs. real binaural
                // …", wild-6 #14).
                let gap = match self.words[i].as_str() {
                    "vs" | "v" => gap.strip_prefix('.').unwrap_or(gap),
                    _ => gap,
                };
                gap.chars().all(|c| {
                    c.is_whitespace()
                        || matches!(c, '-' | '‐' | '\'' | '’')
                        || (!edge && matches!(c, ',' | ':' | '(' | ')'))
                })
            })
        };
        replaced(&text, &title, &together)
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
    /// title shortened is the same title), and the title's own joining
    /// words may be left out ("Multiple HPV 16 infection with two strains:
    /// a possible marker" for "Multiple HPV infection as possible marker").
    pub(super) fn inserts_in(&self, title: &str) -> bool {
        /// Words inserted, at most, before the title is taken as not there.
        const GAP: usize = 8;
        let title = words(unmarked(title));
        if content_words(&title.join(" ")).len() < super::caps::SHORT_TITLE_WORDS {
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
                    } else if STOPWORDS.contains(&title[j].as_str())
                        && title.get(j + 1) == Some(word)
                    {
                        j += 2;
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
    /// A main title of the citation's own before it, the record's title its
    /// subtitle ("Assessment of Clinical Criteria for Sepsis: For the Third
    /// International Consensus …" for "The Third International …", #168),
    /// is more words too.
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
        // Back over the joining words opening a subtitle ("For the"), to a
        // colon or spaced dash with a title's word before it: not an
        // acronym's ("PROSEVA: Prone positioning …"), a number's
        // ("Sepsis-3: …"), nor the authors'.
        let subtitle_of = |start: usize| {
            let gap = |i: usize| &self.raw[self.spans[i - 1].end..self.spans[i].start];
            let mut i = start;
            while i > 0
                && gap(i).trim().is_empty()
                && STOPWORDS.contains(&self.words[i - 1].as_str())
            {
                i -= 1;
            }
            // No colon or dash before (or nothing but joining words, back
            // to the first word): no subtitle.
            let separated = i > 0
                && match gap(i).trim() {
                    ":" => true,
                    "—" | "–" | "-" => gap(i).contains(' '),
                    _ => false,
                };
            if !separated {
                return false;
            }
            // A title of four words or more, back to the sentence's start:
            // not a work's short name ("CodeAct: Executable Code Actions
            // …", "Multicalibration: …") nor a label ("Correction: …",
            // "Clinical practice guideline: …").
            let boundary = |gap: &str| {
                gap.contains(['.', '(', ')', '[', ']', ';', '"', '“', '”'])
                    || (gap.contains(['\'', '‘']) && gap.contains(' '))
            };
            let mut from = i - 1;
            while from > 0 && !boundary(gap(from)) {
                from -= 1;
            }
            i - from >= 4 && {
                let raw = &self.raw[self.spans[i - 1].clone()];
                raw.chars().any(char::is_lowercase)
                    && !names.contains(&self.words[i - 1])
                    && significant(i - 1..i)
            }
        };
        after(span.end) || (span.start > 0 && (before(span.start) || subtitle_of(span.start)))
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
        if phrase(title)
            || (first_sentence(title) != title && phrase(first_sentence(title)))
            || (main_title(title) != title && phrase(main_title(title)))
        {
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
        let share = if share < 2.0 / 3.0 {
            share / 2.0
        } else {
            share
        };
        // Its opening words, then "…": the title cut short, partly given.
        if self.cut_short(title) {
            share.max(EXTENDED_TITLE)
        } else {
            share
        }
    }

    /// Whether the text gives `title`'s opening words, at least
    /// [`SHORT_TITLE_WORDS`] of them content words, then an ellipsis ("…"
    /// or "..."): "Scaling Rectified Flow Transformers… (Stable Diffusion
    /// 3)" for "Scaling Rectified Flow Transformers for High-Resolution
    /// Image Synthesis".
    fn cut_short(&self, title: &str) -> bool {
        let title_words = words(title);
        (0..self.words.len()).any(|i| {
            let k = title_words
                .iter()
                .zip(&self.words[i..])
                .take_while(|(a, b)| a == b)
                .count();
            k > 0
                && k < title_words.len()
                && content_words(&title_words[..k].join(" ")).len() >= SHORT_TITLE_WORDS
                && {
                    let rest = self.raw[self.spans[i + k - 1].end..].trim_start();
                    rest.starts_with('…') || rest.starts_with("...")
                }
        })
    }

    /// Whether the reference gives `title` cut off: its opening words, at
    /// least [`SHORT_TITLE_WORDS`] of them content words, then nothing
    /// more of it, the words dropped neither found elsewhere in the
    /// reference nor marked dropped ("…"), and the cut not where the title
    /// has a subtitle, a question's or a sentence's end, a bracket or a
    /// comma ([`cut_at_separator`]). "Increases in Depressive Symptoms, …
    /// Among U.S. Adolescents After 2010" for "… After 2010 and Links to
    /// Increased New Media Screen Time": the words dropped may carry the
    /// finding, so the title isn't the work's as cited. A word misspelt
    /// where the run ends ("Scepticism" for "Scepticism1") isn't a cut.
    /// `whole`: the reference is the title cited, all of it the run.
    pub(super) fn cut_off(&self, title: &str, whole: bool) -> bool {
        let title = unmarked(title);
        let t = words(title);
        if t.is_empty() || self.title_span(title).is_some() {
            return false;
        }
        let Some((i, k)) = (0..self.words.len())
            .map(|i| {
                let k = t
                    .iter()
                    .zip(&self.words[i..])
                    .take_while(|(a, b)| a == b)
                    .count();
                (i, k)
            })
            .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
        else {
            return false;
        };
        if k == 0
            || k >= t.len()
            || content_words(&t[..k].join(" ")).len() < SHORT_TITLE_WORDS
            || (whole && (i != 0 || k != self.words.len()))
            || self
                .words
                .get(i + k)
                .is_some_and(|w| super::authors::near(w, &t[k]))
            || cut_at_separator(title, k)
        {
            return false;
        }
        // The cited title ends there (a stop, a quote, the field's end):
        // not one reworded at its end ("… stem cell transplant recipients"
        // for "… stem cell transplantation: a randomized …").
        let rest = self.raw[self.spans[i + k - 1].end..].trim_start();
        if rest.starts_with('…')
            || rest.starts_with("...")
            || rest.starts_with(char::is_alphanumeric)
        {
            return false;
        }
        let dropped = significant_words(&t[k..]);
        dropped
            .iter()
            .any(|w| w.chars().filter(|c| c.is_alphabetic()).count() >= 3)
            && dropped.iter().all(|w| !self.words.contains(w))
    }

    /// Whether `title` is found as a phrase only in a short form: its main
    /// title or first sentence, of fewer than [`SHORT_TITLE_WORDS`]
    /// content words, without the rest ("Iceland" of "Iceland: A Peaceful
    /// Secession"), or the whole title when it is a single word ("13th").
    /// A word or two turn up in any text. (A whole title of two words,
    /// "Philosophy of Psychology", is the title cited.)
    pub(super) fn found_only_short(&self, title: &str) -> bool {
        let title = unmarked(title);
        let phrase = |t: &str| {
            let norm = normalise(t);
            !norm.is_empty() && self.padded.contains(&format!(" {norm} "))
        };
        if phrase(title) {
            return content_words(title).len() < 2;
        }
        let parts: Vec<&str> = [first_sentence(title), main_title(title)]
            .into_iter()
            .filter(|t| *t != title && phrase(t))
            .collect();
        !parts.is_empty()
            && parts
                .iter()
                .all(|t| content_words(t).len() < SHORT_TITLE_WORDS)
    }

    /// Citation styles routinely list only the first author or two ("Vaswani
    /// et al."), so the first author's family name appearing is a match on
    /// its own (0.9), with the next authors a small bonus. Without the first
    /// author, later names count for at most half.
    pub(super) fn author_coverage(&self, families: &[String]) -> f64 {
        let present = |f: &String| {
            self.padded.contains(&format!(" {f} "))
                || self
                    .words
                    .iter()
                    .any(|w| super::authors::umlaut_spelling(w, f))
        };
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
    /// Chinese and Japanese, written without spaces, count a word for
    /// each character ("化学分析仪器计量检定分析" is a title).
    /// The journal's words count where it stands, its longest run: the
    /// same words in the title before ("The contribution of growth hormone
    /// in fetal design. Growth Hormone & IGF Research.") are the title's.
    pub(super) fn title_run(&self, families: &[String], journal: Option<&str>) -> usize {
        let mut unaccounted = self.unaccounted(families, None);
        if let Some(journal) = journal.map(crate::publication::significant) {
            let named: Vec<bool> = self
                .words
                .iter()
                .map(|w| crate::publication::journal_word(w, &journal))
                .collect();
            // Runs of the journal's words, joining words within them.
            let mut runs: Vec<(usize, usize, usize)> = Vec::new();
            let mut i = 0;
            while i < self.words.len() {
                if !named[i] {
                    i += 1;
                    continue;
                }
                let (start, mut end, mut count) = (i, i + 1, 1);
                let mut j = i + 1;
                while j < self.words.len()
                    && (named[j] || STOPWORDS.contains(&self.words[j].as_str()))
                {
                    if named[j] {
                        end = j + 1;
                        count += 1;
                    }
                    j += 1;
                }
                runs.push((start, end, count));
                i = end;
            }
            let longest = runs.iter().map(|r| r.2).max().unwrap_or(0);
            for &(start, end, count) in &runs {
                if count == longest {
                    for (k, u) in unaccounted.iter_mut().enumerate().take(end).skip(start) {
                        *u = *u && !named[k];
                    }
                }
            }
        }
        let mut longest = 0;
        let mut run = 0;
        for (word, unaccounted) in self.words.iter().zip(unaccounted) {
            if unaccounted {
                run += word.chars().filter(|c| unspaced(*c)).count().max(1);
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
            .zip(&self.linked)
            .map(|(w, &linked)| {
                !(linked
                    || w.chars().count() <= 2
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
        // A volume numbered like a year, with its part and issue
        // ("2007.0(52)", a working-paper series'), is no year; nor an arXiv
        // number's year and month ("1811.07871").
        let decimal = |i: usize| {
            let Some(after) = self.raw[self.spans[i].end..].strip_prefix('.') else {
                return false;
            };
            let digits = after.len() - after.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            let rest = &after[digits..];
            let month = self.words[i].get(2..).and_then(|m| m.parse::<u8>().ok());
            rest.starts_with('(')
                || ((4..=5).contains(&digits)
                    && month.is_some_and(|m| (1..=12).contains(&m))
                    && !rest.starts_with(|c: char| c.is_alphanumeric() && c != 'v'))
        };
        let years = |skip: &Range<usize>| -> Vec<i16> {
            self.words
                .iter()
                .enumerate()
                .filter(|(i, w)| {
                    w.len() == 4
                        && !span.contains(i)
                        && !skip.contains(i)
                        && !self.linked[*i]
                        && !decimal(*i)
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

/// A title that is a URL.
pub(super) fn is_url(title: &str) -> bool {
    let t = title.trim();
    ["http://", "https://", "www."]
        .iter()
        .any(|p| crate::id::starts_with_ci(t, p))
        || t.contains("://")
}
