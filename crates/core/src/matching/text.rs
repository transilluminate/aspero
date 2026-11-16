//! Text comparison: normalising, titles, and looking for a record's fields
//! inside a free-text reference.

use std::ops::Range;

use unicode_normalization::UnicodeNormalization;

use crate::work::PLAUSIBLE_YEARS;

/// Authors compared from the start of a byline.
const AUTHORS_COMPARED: usize = 3;

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

/// A title's first sentence, when a record's runs on into a description
/// ("Nurse staffing and patient outcomes: Strengths and limitations of
/// the evidence to inform policy and practice. A review and discussion
/// paper based on evidence reviewed for …"): cited without it, the title
/// is the record's with words dropped, its subtitle kept.
pub(super) fn first_sentence(title: &str) -> &str {
    title
        .find(". ")
        .filter(|&i| {
            // Not an initial or abbreviation's stop ("J. Smith", "vs. ").
            title[..i]
                .rsplit(' ')
                .next()
                .is_some_and(|w| w.chars().count() > 3)
        })
        .map_or(title, |i| title[..i].trim())
}

/// Whether a claimed title, given apart (a structured claim's), is the
/// record's with significant words of its own put in place of the
/// title's ("A Controlled Study of …" for "A Randomized Trial of …"):
/// reworded, not misspelt, abbreviated or respelt.
pub(super) fn replaces(claimed: &str, actual: &str) -> bool {
    let claimed = significant_words(&words(unmarked(claimed)));
    let actual = significant_words(&words(unmarked(actual)));
    replaced(&claimed, &actual, &|_, _, _| true)
}

/// A title's words that can be replaced: not joining words, nor a letter
/// on its own (a possessive's "s").
fn significant_words(words: &[String]) -> Vec<String> {
    words
        .iter()
        .filter(|w| significant_word(w))
        .cloned()
        .collect()
}

fn significant_word(w: &str) -> bool {
    (w.chars().count() > 1 || w.chars().all(|c| c.is_ascii_digit())) && !STOPWORDS.contains(&w)
}

/// The share of a title's words kept, at least, and replaced, at most,
/// for a title reworded: under half kept is another title, or the title
/// not found ("Function of Insulin Resistance in Human Illness" keeps
/// three of "Role of Insulin Resistance in Human Disease"'s five).
const KEPT_SHARE: usize = 2;

/// Whether `claimed` (a title's words, or a reference's) gives `title`'s
/// words with some of them put in place of others: aligned on the words
/// they share (give or take a letter), each stretch between where
/// `claimed` and `title` both have words of their own counts a word
/// replaced for each pair, unless the words are the same written another
/// way ([`same_written_otherwise`]). The words of `claimed` between two
/// it shares must run together (`together`, over indices into `claimed`:
/// in a reference, no stop or quote between); where it runs past the
/// title's first or last shared word, only as many of its words as the
/// title's count, run together with it as one phrase (`together` with
/// `edge`: no punctuation). A replacement needs half the title's words
/// kept, at least, and half replaced, at most.
fn replaced(
    claimed: &[String],
    title: &[String],
    together: &dyn Fn(usize, usize, bool) -> bool,
) -> bool {
    /// Words of a stretch, at most, on either side: more is no rewording.
    const STRETCH: usize = 3;
    let (n, m) = (claimed.len(), title.len());
    if m < 3 || n == 0 {
        return false;
    }
    // Numbers alike only when the same: "2004" is not "2006".
    let same = |a: &str, b: &str| {
        a == b || (!a.chars().any(|c| c.is_ascii_digit()) && super::authors::near(a, b))
    };
    // Longest common subsequence, words alike as `same` says.
    let mut table = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i][j] = if same(&claimed[i], &title[j]) {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    let mut anchors: Vec<(usize, usize)> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if same(&claimed[i], &title[j]) && table[i][j] == table[i + 1][j + 1] + 1 {
            anchors.push((i, j));
            i += 1;
            j += 1;
        } else if table[i + 1][j] >= table[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    let kept = anchors.len();
    if kept < 2 || kept * KEPT_SHARE < m {
        return false;
    }
    let mut count = 0;
    let mut stretch = |c: &[String], t: &[String]| {
        if !c.is_empty() && !t.is_empty() && c.len() <= STRETCH && t.len() <= STRETCH {
            count += replacements(c, t);
        }
    };
    let (first, last) = (anchors[0], anchors[kept - 1]);
    // Before the first shared word, and after the last.
    let lead = first.1.min(first.0);
    let from = (first.0 - lead..first.0)
        .find(|&k| together(k, first.0, true))
        .unwrap_or(first.0);
    stretch(&claimed[from..first.0], &title[..first.1]);
    let trail = (m - last.1 - 1).min(n - last.0 - 1);
    let to = (last.0 + 1..=last.0 + trail)
        .rev()
        .find(|&k| together(last.0, k, true))
        .map_or(last.0 + 1, |k| k + 1);
    stretch(&claimed[last.0 + 1..to], &title[last.1 + 1..]);
    for pair in anchors.windows(2) {
        let ((ci, ti), (cj, tj)) = (pair[0], pair[1]);
        if together(ci, cj, false) {
            stretch(&claimed[ci + 1..cj], &title[ti + 1..tj]);
        }
    }
    count > 0 && count * KEPT_SHARE <= m
}

/// Words of `c` put in place of words of `t`, once those written otherwise
/// are paired off.
fn replacements(c: &[String], t: &[String]) -> usize {
    let mut c: Vec<&str> = c.iter().map(String::as_str).collect();
    let mut t: Vec<&str> = t.iter().map(String::as_str).collect();
    // A word split in two on one side ("mort- ality", "non small").
    let unsplit = |parts: &mut Vec<&str>, whole: &mut Vec<&str>| {
        let mut k = 0;
        while k + 1 < parts.len() {
            let joined = format!("{}{}", parts[k], parts[k + 1]);
            if let Some(found) = whole.iter().position(|w| *w == joined) {
                whole.remove(found);
                parts.drain(k..k + 2);
            } else {
                k += 1;
            }
        }
    };
    unsplit(&mut c, &mut t);
    unsplit(&mut t, &mut c);
    let mut k = 0;
    while k < c.len() {
        if let Some(found) = t.iter().position(|w| same_written_otherwise(c[k], w)) {
            t.remove(found);
            c.remove(k);
        } else {
            k += 1;
        }
    }
    c.len().min(t.len())
}

/// Number words, Roman numerals and Greek letters, as titles write them
/// either way.
const SPELLED: &[(&str, &[&str])] = &[
    ("1", &["one", "i", "first"]),
    ("2", &["two", "ii", "second"]),
    ("3", &["three", "iii", "third"]),
    ("4", &["four", "iv", "fourth"]),
    ("5", &["five", "v", "fifth"]),
    ("6", &["six", "vi"]),
    ("7", &["seven", "vii"]),
    ("8", &["eight", "viii"]),
    ("9", &["nine", "ix"]),
    ("10", &["ten", "x"]),
    ("α", &["alpha"]),
    ("β", &["beta"]),
    ("γ", &["gamma"]),
    ("δ", &["delta"]),
    ("κ", &["kappa"]),
    ("λ", &["lambda"]),
    ("μ", &["mu", "micro"]),
    ("ω", &["omega"]),
];

/// The same word written another way: abbreviated ("vs" for "versus",
/// "intl"), shortened or extended ("program", "programme"), run together
/// with another ("theeffect"), a number or Greek letter spelt out.
fn same_written_otherwise(a: &str, b: &str) -> bool {
    let (short, long) = if a.chars().count() <= b.chars().count() {
        (a, b)
    } else {
        (b, a)
    };
    let abbreviation =
        short.chars().count() <= 5 && long.chars().count() >= short.chars().count() + 2 && {
            let mut rest = long.chars();
            short.chars().next() == long.chars().next()
                && short.chars().all(|c| rest.any(|l| l == c))
        };
    let within = short.chars().count() >= 4 && long.contains(short);
    let spelled = SPELLED.iter().any(|(form, others)| {
        (a == *form && others.contains(&b)) || (b == *form && others.contains(&a))
    });
    abbreviation || within || spelled
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

    /// Whether the reference gives `title` with significant words of its
    /// own put in place of the title's, anywhere in it and however many
    /// together ("Likelihood Determinants Associated With …" for "Risk
    /// Factors Associated With …", "Monte Carlo sampling approaches with
    /// Markov chains" for "… methods using Markov chains"): the title
    /// reworded, as [`replaces`] tells for a title given apart. Words
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
    /// title shortened is the same title).
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
        // A volume numbered like a year, with its part and issue
        // ("2007.0(52)", a working-paper series'), is no year.
        let decimal = |i: usize| {
            self.raw[self.spans[i].end..]
                .strip_prefix('.')
                .map(|s| s.trim_start_matches(|c: char| c.is_ascii_digit()))
                .is_some_and(|rest| rest.starts_with('('))
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
    if let Some(given) = given_title(text) {
        return Some(given);
    }
    // Of its sentences, not a byline, a publisher or locator (";"), the
    // one most in sentence case, then the longest.
    text.split(". ")
        .filter(|s| !s.contains(';') && !super::authors::names_given(s))
        .filter_map(title_like)
        .max_by_key(|s| {
            let words: Vec<&str> = s.split_whitespace().collect();
            let lower = words
                .iter()
                .filter(|w| w.chars().count() >= 3 && w.starts_with(char::is_lowercase))
                .count();
            (lower, words.len())
        })
}

/// A span of a reference that may be a title: words, one of three letters
/// or more, not "In: …".
fn title_like(s: &str) -> Option<String> {
    let s = s
        .trim()
        .trim_matches(|c: char| c.is_whitespace() || ",.;:'\"“”‘’".contains(c));
    let wordy = s
        .split_whitespace()
        .any(|w| w.chars().filter(|c| c.is_alphabetic()).count() >= 3);
    (wordy && !s.starts_with("In:") && !s.starts_with("In ")).then(|| s.to_owned())
}

/// The title a reference marks as one: a BibTeX entry's title, else the
/// longest quoted span (Harvard's 'An image is worth 16x16 words', IEEE's
/// "…,").
pub fn given_title(text: &str) -> Option<String> {
    let title = title_like;
    if let Some(t) = crate::publication::bibtex_field(text, "title") {
        return title(&t);
    }
    [('"', '"'), ('“', '”'), ('‘', '’'), ('\'', '\'')]
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
        .max_by_key(|t| t.split_whitespace().count())
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
