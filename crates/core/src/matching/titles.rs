//! Comparing titles: similarity, a title with words added, inserted,
//! replaced or spelt otherwise; and reading the title a reference gives.

use super::text::{STOPWORDS, content_words, dedup, words};

/// A title without a leading status marker publishers add to retracted or
/// withdrawn works ("RETRACTED: Ileal-lymphoid…", "RETRACTED ARTICLE: …",
/// IEEE's "Notice of Retraction: …"), so a citation of the original title
/// still matches. Notices themselves ("Retraction: …") are left alone.
pub fn unmarked(title: &str) -> &str {
    let trimmed = title.trim_start();
    for marker in [
        "retracted article",
        "retracted",
        "withdrawn",
        "notice of retraction",
    ] {
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

/// A reference with the acronyms it writes for a run of `title`'s words
/// written out as those words ("Theory of Mind in LLMs" for "… in Large
/// Language Models", "RLHF" for "Reinforcement Learning from Human
/// Feedback", "ToM"): models abbreviate titles so. An acronym is two
/// capitals or more, a plural's "s" aside, spelling the initials of
/// consecutive words of the title, its joining words' optional; one the
/// title holds as a word stays. `None` when there is none.
pub fn expand_acronyms(text: &str, title: &str) -> Option<String> {
    // The run of title words from `at` whose initials spell `letters`.
    fn spelt(letters: &[char], words: &[String], at: usize) -> Option<usize> {
        let Some(&c) = letters.first() else {
            return Some(at);
        };
        let word = words.get(at)?;
        let joining = STOPWORDS.contains(&word.as_str());
        if word.starts_with(c)
            && let Some(end) = spelt(&letters[1..], words, at + 1)
        {
            return Some(end);
        }
        // A joining word inside the run may give no letter ("Learning
        // from Human" in "RLHF").
        if joining && at > 0 {
            return spelt(letters, words, at + 1);
        }
        None
    }
    let title_words: Vec<&str> = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let lower: Vec<String> = title_words.iter().map(|w| w.to_lowercase()).collect();
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    let mut rest = text;
    let mut previous = "";
    while !rest.is_empty() {
        let start = rest.find(char::is_alphanumeric).unwrap_or(rest.len());
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let end = rest
            .find(|c: char| !c.is_alphanumeric())
            .unwrap_or(rest.len());
        let token = &rest[..end];
        rest = &rest[end..];
        let capitals = token.chars().filter(char::is_ascii_uppercase).count();
        let stem = token
            .strip_suffix('s')
            .filter(|_| token.len() > 2)
            .unwrap_or(token);
        let letters: Vec<char> = stem.to_lowercase().chars().collect();
        // A name's initials ("Kishore CP,") are no acronym.
        let initials = token.chars().all(|c| c.is_ascii_uppercase())
            && previous.starts_with(char::is_uppercase)
            && previous.chars().any(char::is_lowercase)
            && (rest.is_empty() || rest.starts_with([',', '.', ';']));
        let expansion = (capitals >= 2
            && !initials
            && (2..=8).contains(&letters.len())
            && token.chars().all(|c| c.is_ascii_alphabetic())
            && !lower
                .iter()
                .any(|w| w.eq_ignore_ascii_case(token) || *w == stem.to_lowercase()))
        .then(|| {
            (0..lower.len())
                .filter(|&i| !STOPWORDS.contains(&lower[i].as_str()))
                .find_map(|i| {
                    let end = spelt(&letters, &lower, i)?;
                    // A run ends on a word of its own, not a joining word.
                    (end - i >= 2 && !STOPWORDS.contains(&lower[end - 1].as_str()))
                        .then(|| title_words[i..end].join(" "))
                })
        })
        .flatten();
        match expansion {
            Some(words) => {
                out.push_str(&words);
                changed = true;
            }
            None => out.push_str(token),
        }
        previous = token;
    }
    changed.then_some(out)
}

/// Whether `cited` (a reference's words, or a title cited apart) has
/// every content word of `title`, but for misspellings ("Ailgnment",
/// "colour"); with `both`, and no content words of its own either.
pub(super) fn misspelt_only(cited: &[String], title: &str, both: bool) -> bool {
    let near = |w: &String, among: &[String]| among.iter().any(|o| super::authors::near(w, o));
    let title = content_words(unmarked(title));
    let cited_content = content_words(&cited.join(" "));
    title.iter().all(|w| near(w, cited)) && (!both || cited_content.iter().all(|w| near(w, &title)))
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
pub(super) fn significant_words(words: &[String]) -> Vec<String> {
    words
        .iter()
        .filter(|w| significant_word(w))
        .cloned()
        .collect()
}

pub(super) fn significant_word(w: &str) -> bool {
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
pub(super) fn replaced(
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
    if let Some(title) = after_comma_byline(text) {
        return Some(title);
    }
    // Of its sentences, not a byline, a publisher or locator (";"), the
    // one most in sentence case, then the longest; a year opening one
    // (Harvard's "(2011) Viking coinage …", after initials) is the year's.
    // Nor editors' names ("Ragep, eds").
    let editors = |s: &str| {
        s.split_whitespace().last().is_some_and(|w| {
            matches!(
                w.trim_matches(|c: char| !c.is_alphabetic())
                    .to_lowercase()
                    .as_str(),
                "ed" | "eds" | "editor" | "editors" | "dir" | "hrsg"
            )
        })
    };
    text.split(". ")
        .filter(|s| !s.contains(';') && !super::authors::names_given(s) && !editors(s))
        .map(without_leading_year)
        .filter_map(title_like)
        .enumerate()
        // Of sentences alike, the first: a title precedes its place.
        .max_by_key(|(i, s)| {
            let words: Vec<&str> = s.split_whitespace().collect();
            let lower = words
                .iter()
                .filter(|w| w.chars().count() >= 3 && w.starts_with(char::is_lowercase))
                .count();
            (lower, words.len(), std::cmp::Reverse(*i))
        })
        .map(|(_, s)| s)
}

/// A sentence without the year in brackets opening it: "(2011) Viking
/// coinage …".
fn without_leading_year(s: &str) -> &str {
    let t = s.trim_start();
    let Some(inner) = t.strip_prefix('(') else {
        return s;
    };
    let digits = inner.chars().take_while(char::is_ascii_digit).count();
    let rest = &inner[digits..];
    let rest = rest
        .strip_prefix(|c: char| c.is_ascii_lowercase())
        .unwrap_or(rest);
    match rest.strip_prefix(')') {
        Some(after) if digits == 4 => after.trim_start_matches(['.', ',']).trim_start(),
        _ => s,
    }
}

/// The title after a byline written "Family, Given," (French and some
/// Chicago references: "Vidal-Naquet, Pierre, La torture dans la
/// République, Paris, Minuit, 1972"), up to a sentence's end, a place (a
/// lone capitalised word before a comma or colon), a number or a bracket.
/// Given names are written out ("Charles-Robert", "Thomas S."), unlike
/// APA's initials; a second author ("…, and Jane Doe") is no title.
fn after_comma_byline(text: &str) -> Option<String> {
    let name = |w: &str| {
        w.starts_with(char::is_uppercase)
            && w.chars()
                .all(|c| c.is_alphabetic() || matches!(c, '-' | '\'' | '’'))
    };
    // Initials in capitals ("Radford A, Kim JW,") are Vancouver's.
    let capitals = |w: &str| {
        let w = w.trim_end_matches('.');
        w.chars().count() >= 2 && w.chars().all(|c| c.is_uppercase() || c == '.')
    };
    let (family, rest) = text.split_once(", ")?;
    if !family.split_whitespace().all(name)
        || family.split_whitespace().count() > 3
        || family
            .split_whitespace()
            .any(|w| w.chars().count() <= 3 && w.chars().all(char::is_uppercase))
    {
        return None;
    }
    let (given, rest) = rest.split_once(", ")?;
    let given = given
        .trim_end_matches(' ')
        .trim_end_matches(['.'])
        .split(" (")
        .next()
        .unwrap_or_default();
    let parts: Vec<&str> = given.split_whitespace().collect();
    let initial = |w: &str| {
        let w = w.trim_end_matches('.');
        w.chars().count() == 1 && w.chars().all(char::is_uppercase)
    };
    let written_out = parts.iter().any(|w| {
        name(w)
            && w.chars().filter(|c| c.is_alphabetic()).count() >= 2
            && w.chars().any(char::is_lowercase)
    });
    if parts.is_empty()
        || parts.len() > 3
        || !written_out
        || !parts
            .iter()
            .all(|w| name(w.trim_end_matches('.')) || initial(w))
        || parts.iter().any(|w| capitals(w))
    {
        return None;
    }
    let first = rest.split_whitespace().next()?;
    if matches!(
        first.to_lowercase().as_str(),
        "and" | "&" | "et" | "y" | "und" | "e" | "ed" | "eds" | "dir"
    ) || first.starts_with(|c: char| c.is_ascii_digit())
    {
        return None;
    }
    // The title ends at a sentence's end, a bracket or semicolon, or a
    // comma before a place, a publisher's or a number.
    let mut end = rest.len();
    for stop in [". ", " (", ";"] {
        if let Some(at) = rest.find(stop) {
            end = end.min(at);
        }
    }
    let mut from = 0;
    while let Some(at) = rest[from..end].find(", ") {
        let after = &rest[from + at + 2..];
        let next = after.split_whitespace().next().unwrap_or_default();
        let bare = next.trim_end_matches([',', ':', '.']);
        let place = name(bare) && next.ends_with([',', ':']);
        if place || next.starts_with(|c: char| c.is_ascii_digit()) {
            end = from + at;
            break;
        }
        from += at + 2;
    }
    let title = rest[..end].trim().trim_end_matches(['.', ',']);
    (title.split_whitespace().count() >= 2)
        .then(|| title.to_owned())
        .and_then(|t| title_like(&t))
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
