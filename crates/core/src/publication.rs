//! Where a citation says a work appeared: journal, volume, first page.
//!
//! Journals are compared by rule rather than with an abbreviation list:
//! each word of an abbreviation must abbreviate the matching significant
//! word of the full title, in order ("N Engl J Med" ↔ "New England Journal
//! of Medicine", "Lancet" ↔ "The Lancet"), or the abbreviation is the
//! initials ("NEJM", "JAMA", "BMJ"). Pairs the rule can't see are
//! reported as different, so a mismatch by [`same_journal`] alone is a hint,
//! not proof; [`other_journal`] is the stricter test of a journal clearly
//! cited in place of another. Volume and page are numbers and compare
//! exactly.

use crate::id::{is_digits, starts_with_ci};
use crate::matching::normalise;
use crate::work::PLAUSIBLE_YEARS;

/// Words a journal abbreviation leaves out.
const JOURNAL_STOPWORDS: &[&str] = &[
    "the", "of", "and", "for", "in", "on", "de", "der", "des", "du", "la", "le", "und", "fur",
    "fuer", "et", "y", "di", "del", "della", "voor",
];

/// Whether two journal names are the same journal: equal, or one
/// abbreviates the other.
pub fn same_journal(a: &str, b: &str) -> bool {
    let (a, b) = (significant(a), significant(b));
    !a.is_empty() && (a == b || abbreviates(&a, &b) || abbreviates(&b, &a))
}

/// Whether `cited` is clearly another journal than `actual`, not just
/// named in a way [`same_journal`] doesn't see: neither name, in full or
/// abbreviated (spelling aside: "Anaesth" for "Anesthesia"), runs inside
/// the other ("Proc Natl Acad Sci U S A" ↔ "Proceedings of the National
/// Academy of Sciences", "BMJ" ↔ "BMJ (Clinical research ed.)"), nor is
/// either the initials of a run of the other's words ("PNAS"). "Environ
/// Sci Technol" is not "Environment International".
pub fn other_journal(cited: &str, actual: &str) -> bool {
    let (c, a) = (significant(cited), significant(actual));
    if c.is_empty() || a.is_empty() || same_journal(cited, actual) {
        return false;
    }
    let inside = |short: &[String], long: &[String]| {
        long.windows(short.len()).any(|run| {
            run.iter()
                .zip(short)
                .all(|(f, s)| loosely_abbreviates(s, f) || loosely_abbreviates(f, s))
        }) || short.iter().any(|w| {
            (1..=long.len())
                .flat_map(|n| long.windows(n))
                .any(|run| run.len() > 1 && is_initials(w, run))
        })
    };
    !(inside(&c, &a) || inside(&a, &c))
}

/// [`abbreviates_word`], or the same with vowels dropped after the first
/// letter, for spellings that differ ("anaesth" ↔ "anesthesia",
/// "paediatr" ↔ "pediatrics").
fn loosely_abbreviates(short: &str, full: &str) -> bool {
    let skeleton = |w: &str| -> String {
        let mut chars = w.chars();
        chars
            .next()
            .into_iter()
            .chain(chars.filter(|c| !"aeiouy".contains(*c)))
            .collect()
    };
    abbreviates_word(short, full) || abbreviates_word(&skeleton(short), &skeleton(full))
}

/// Whether `journal`, in full or abbreviated, appears in `words` (a
/// normalised reference, split into words).
pub(crate) fn journal_in(words: &[String], journal: &str) -> bool {
    let full = significant(journal);
    if full.is_empty() {
        return false;
    }
    let text: Vec<&str> = words
        .iter()
        .map(String::as_str)
        .filter(|w| !JOURNAL_STOPWORDS.contains(w))
        .collect();
    text.windows(full.len())
        .any(|run| run.iter().zip(&full).all(|(s, f)| abbreviates_word(s, f)))
        || (full.len() > 1 && text.iter().any(|w| is_initials(w, &full)))
}

/// Whether `word` (normalised) is one of a journal's significant words
/// (from [`significant`]), or abbreviates one, or is its initials ("res"
/// or "erj" in "ERJ Open Research").
pub(crate) fn journal_word(word: &str, full: &[String]) -> bool {
    full.iter().any(|f| abbreviates_word(word, f)) || (full.len() > 1 && is_initials(word, full))
}

/// A journal name's words that tell it apart, normalised.
pub(crate) fn significant(name: &str) -> Vec<String> {
    normalise(name)
        .split(' ')
        .filter(|w| !w.is_empty() && !JOURNAL_STOPWORDS.contains(w))
        .map(str::to_owned)
        .collect()
}

/// `short` abbreviates `full` word by word, or is its initials.
fn abbreviates(short: &[String], full: &[String]) -> bool {
    match short {
        [one] if full.len() > 1 => is_initials(one, full),
        _ => {
            short.len() == full.len() && short.iter().zip(full).all(|(s, f)| abbreviates_word(s, f))
        }
    }
}

/// "med" for "medicine", "natl" for "national": same first letter, the
/// rest in order.
fn abbreviates_word(short: &str, full: &str) -> bool {
    let mut full_chars = full.chars();
    short.chars().next() == full.chars().next() && short.chars().all(|c| full_chars.any(|f| f == c))
}

fn is_initials(word: &str, full: &[String]) -> bool {
    word.chars().count() == full.len() && word.chars().zip(full).all(|(c, f)| f.starts_with(c))
}

/// The first page of a page or range: "2545" from "2545-55" or "2545–2555".
pub fn first_page(pages: &str) -> &str {
    pages
        .split(['-', '–', '—', ','])
        .next()
        .unwrap_or("")
        .trim()
}

/// Volume and first page as a reference gives them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Locator {
    pub volume: Option<String>,
    pub page: Option<String>,
}

/// The volume and first page in a free-text reference, from the common
/// styles: Vancouver ("2017;376(26):2545-55"), APA ("376(26), 2545–2555",
/// or without an issue "Journal Name, 146, 106274.") and labelled ("vol.
/// 376, pp. 2545–2555").
pub(crate) fn locator(text: &str) -> Locator {
    let compact = compact(text);
    let tokens: Vec<&str> = compact.split_whitespace().collect();
    match styled(&tokens) {
        Some((_, found)) => found,
        None => labelled(&tokens),
    }
}

/// The reference with the spaces that styles put around their separators
/// dropped, so each locator is one token: "2014; 370 : 1402" →
/// "2014;370:1402", "376 (9734)" → "376(9734)".
fn compact(text: &str) -> String {
    let mut compact = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_whitespace() && compact.ends_with([';', ':']) {
            continue;
        }
        if c == '(' && compact.trim_end().ends_with(|p: char| p.is_ascii_digit()) {
            let trimmed = compact.trim_end().len();
            compact.truncate(trimmed);
        }
        if matches!(c, ';' | ':') {
            let trimmed = compact.trim_end().len();
            compact.truncate(trimmed);
        }
        compact.push(c);
    }
    compact
}

/// The first styled locator among a compacted reference's tokens, with the
/// index of the token it starts at: what comes before it is the journal.
fn styled(tokens: &[&str]) -> Option<(usize, Locator)> {
    let at = |volume: String, page: String| Locator {
        volume: Some(volume),
        page: Some(page),
    };
    for (i, token) in tokens.iter().enumerate() {
        let token = token.trim_end_matches(['.', ',']);
        // Vancouver: [year;]volume[(issue)]:page[-end], with a year, issue
        // or page range to tell it from a time of day ("at 10:30").
        if let Some((head, pages)) = token.split_once(':') {
            let volume = head.rsplit(';').next().unwrap_or(head);
            let styled = head.contains([';', '(']) || pages.contains(['-', '–', '—']);
            if styled
                && let (Some(volume), Some(page)) = (volume_number(volume), page_number(pages))
            {
                return Some((i, at(volume, page)));
            }
        }
        // APA: volume(issue), page[–end]
        if tokens[i].ends_with(',')
            && token.contains('(')
            && let Some(volume) = volume_number(token)
            && let Some(page) = tokens.get(i + 1).and_then(|p| page_number(p))
        {
            return Some((i, at(volume, page)));
        }
        // Running headers, Elsevier's "47 (2016) 797–804" and Nature's
        // "521, 436–444 (2015)": a year in brackets tells them apart.
        let next_page = tokens.get(i + 1).map(|p| p.trim_end_matches(['.', ',']));
        if let Some((volume, year)) = token.split_once('(')
            && bracketed_year(year)
            && let (Some(volume), Some(page)) = (
                volume_number(volume),
                next_page.filter(|p| ranged(p)).and_then(page_number),
            )
        {
            return Some((i, at(volume, page)));
        }
        if tokens[i].ends_with(',')
            && let Some(volume) = volume_number(token)
            && let Some((pages, year)) = next_page.and_then(|p| p.split_once('('))
            && bracketed_year(year)
            && ranged(pages)
            && let Some(page) = page_number(pages)
        {
            return Some((i, at(volume, page)));
        }
        // APA without an issue: "Journal Name, 146, 106274." Bare numbers
        // after commas are common, so only right after a word and its
        // comma (the journal), not a year, and with the page ending the
        // reference or followed by its link.
        if tokens[i].ends_with(',')
            && let Some(volume) = volume_number(token).filter(|v| !is_year(v))
            && i > 0
            && tokens[i - 1]
                .strip_suffix(',')
                .is_some_and(|w| !w.is_empty() && w.chars().all(char::is_alphabetic))
            && let Some(page) = tokens.get(i + 1).and_then(|p| page_number(p))
            && (tokens[i + 1].ends_with('.')
                || tokens
                    .get(i + 2)
                    .is_none_or(|t| starts_with_ci(t, "doi") || starts_with_ci(t, "http")))
        {
            return Some((i, at(volume, page)));
        }
    }
    None
}

/// Volume and page labelled as such: "vol. 376, pp. 2545–2555".
fn labelled(tokens: &[&str]) -> Locator {
    let mut found = Locator::default();
    for (i, token) in tokens.iter().enumerate() {
        let label = token.trim_end_matches(['.', ',']).to_ascii_lowercase();
        let next = || tokens.get(i + 1).copied();
        match label.trim_end_matches('.') {
            "vol" | "volume" if found.volume.is_none() => {
                found.volume = next().and_then(|v| volume_number(v.trim_end_matches([',', '.'])));
            }
            "pp" | "p" | "pages" | "page" if found.page.is_none() => {
                found.page = next().and_then(page_number);
            }
            _ => {}
        }
    }
    found
}

/// Words at most a journal's name runs to ("Proceedings of the National
/// Academy of Sciences of the United States of America").
const JOURNAL_WORDS: usize = 16;

/// The journal a free-text reference cites, when it can be told with
/// confidence: the words right before a styled locator (Vancouver
/// "Environ Sci Technol. 2021;146:106274", APA "Environmental Science &
/// Technology, 146, 106274", running headers "Nature 521, 436–444
/// (2015)"), each capitalised (or a joining word: "of", "&"), back to the
/// end of the sentence before (the title's, or "et al.") or the start of
/// the text. Abbreviations with stops ("J. Am. Chem. Soc.") are read too,
/// back to a sentence ending in a lowercase word.
///
/// `None` when anything is unusual: a labelled locator ("vol. 3"), a
/// publisher in the locator ("Springer; 2010:12-30"), a lowercase or
/// numbered word, punctuation inside the name, or a run of dotted
/// abbreviations that may have begun earlier ("Injury, Int. J. Care
/// Injured 47 (2016)"). A journal misread would be called another journal,
/// so better none.
pub(crate) fn cited_journal(text: &str) -> Option<String> {
    let compact = compact(text);
    let tokens: Vec<&str> = compact.split_whitespace().collect();
    let (at, found) = styled(&tokens)?;
    // "Springer;2010:12-30": a publisher, not a year, before the volume;
    // "Proceedings of X. 2016:770-778": a year, not a volume.
    let head = tokens[at].split([';', ':', '(', ',']).next().unwrap_or("");
    let semicolon = tokens[at].contains(';');
    if (semicolon && !is_digits(head) && !is_month(head))
        || (!semicolon && found.volume.as_deref().is_some_and(is_year))
    {
        return None;
    }
    // The date between journal and locator: "N Engl J Med. 2017 Jun 29;376…".
    let mut end = at;
    while end > 0 && is_date_word(tokens[end - 1]) {
        end -= 1;
    }
    let last = *tokens[..end].last()?;
    // The journal's last word, with the stop or comma ending it.
    let ending = last.strip_suffix(['.', ',']);
    let last_word = ending.unwrap_or(last);
    if !name_word(last_word) || is_stopword(last_word) {
        return None;
    }
    let mut k = end - 1;
    // Abbreviated with stops: the word before ends with one too. The name
    // is kept as printed, stops and all.
    let dotted = last.ends_with('.')
        && k > 0
        && tokens[k - 1]
            .strip_suffix('.')
            .is_some_and(|w| name_word(w) && capitalised(w));
    let mut words = vec![if dotted { last } else { last_word }];
    while k > 0 {
        let t = tokens[k - 1];
        let stopped = t.strip_suffix('.');
        if dotted {
            let abbreviated = stopped.is_some_and(|w| name_word(w) && capitalised(w));
            if abbreviated || t == "&" || is_stopword(t) {
                words.push(t);
            } else if sentence_end(t) && !capitalised(t) {
                break;
            } else {
                return None;
            }
        } else if name_word(t) {
            words.push(t);
        } else if sentence_end(t) {
            // "Int. J. Care Injured": a stop inside a dotted name.
            let abbreviation = stopped.is_some_and(|w| w.chars().count() <= 5 && capitalised(w))
                && k > 1
                && tokens[k - 2].ends_with('.');
            if abbreviation {
                return None;
            }
            break;
        } else {
            return None;
        }
        if words.len() > JOURNAL_WORDS {
            return None;
        }
        k -= 1;
    }
    words.reverse();
    // A name begins with a name's word ("The", "eLife"), not "of" or "&".
    if words[0] == "&" || is_stopword(words[0]) {
        return None;
    }
    Some(words.join(" "))
}

/// A word of a journal's name as references print it: letters (and
/// hyphens, apostrophes), capitalised or with a capital inside ("eLife"),
/// or a joining word ("of", "&").
fn name_word(w: &str) -> bool {
    if w == "&" || is_stopword(w) {
        return true;
    }
    let letters = !w.is_empty()
        && w.chars()
            .all(|c| c.is_alphabetic() || matches!(c, '-' | '\'' | '’'))
        && w.chars().next().is_some_and(char::is_alphabetic);
    letters && w.chars().any(char::is_uppercase)
}

fn capitalised(w: &str) -> bool {
    w.chars().next().is_some_and(char::is_uppercase)
}

fn is_stopword(w: &str) -> bool {
    JOURNAL_STOPWORDS.contains(&w)
}

/// A word ending a sentence: "placenta.", "al.", "Need?", "(2021).".
fn sentence_end(t: &str) -> bool {
    t.trim_end_matches(['"', '”', '’', '\'', ')'])
        .ends_with(['.', '?', '!'])
}

/// Part of a date between a journal and its locator: "2017", "Jun", "29".
fn is_date_word(t: &str) -> bool {
    let w = t.trim_matches(|c: char| !c.is_alphanumeric());
    is_year(w) || is_month(w) || (is_digits(w) && w.len() <= 2)
}

fn is_year(w: &str) -> bool {
    w.len() == 4 && w.parse().is_ok_and(|y| PLAUSIBLE_YEARS.contains(&y))
}

fn is_month(w: &str) -> bool {
    const MONTHS: &[&str] = &[
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
    ];
    MONTHS.iter().any(|m| m.eq_ignore_ascii_case(w))
}

/// "2016)": a year, its bracket closed.
fn bracketed_year(s: &str) -> bool {
    s.strip_suffix(')')
        .and_then(|y| y.parse().ok())
        .is_some_and(|y| PLAUSIBLE_YEARS.contains(&y))
}

/// A page range: "797–804".
fn ranged(pages: &str) -> bool {
    pages.contains(['-', '–', '—'])
}

/// "376" or "376(26)": the volume number.
fn volume_number(s: &str) -> Option<String> {
    let volume = s.split('(').next().unwrap_or(s);
    (volume.len() <= 6 && is_digits(volume)).then(|| volume.to_owned())
}

/// "2545-55", "e1234" or "S12": the first page.
fn page_number(s: &str) -> Option<String> {
    let page = first_page(s.trim_end_matches(['.', ',', ';']));
    let digits = page.trim_start_matches(['e', 'E', 's', 'S']);
    (digits.len() <= 8 && is_digits(digits)).then(|| page.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journals_match_their_abbreviations_and_initials() {
        for (a, b) in [
            ("N Engl J Med", "New England Journal of Medicine"),
            ("N. Engl. J. Med.", "The New England journal of medicine"),
            ("Lancet", "The Lancet"),
            ("NEJM", "New England Journal of Medicine"),
            ("JAMA", "Journal of the American Medical Association"),
            (
                "Am J Respir Crit Care Med",
                "American Journal of Respiratory and Critical Care Medicine",
            ),
            (
                "Proc Natl Acad Sci U S A",
                "Proceedings of the National Academy of Sciences of the United States of America",
            ),
            (
                "Cochrane Database Syst Rev",
                "Cochrane Database of Systematic Reviews",
            ),
            ("Circulation", "Circulation"),
        ] {
            assert!(same_journal(a, b), "{a} ↔ {b}");
        }
        for (a, b) in [
            ("Lancet", "New England Journal of Medicine"),
            ("N Engl J Med", "Nature Medicine"),
            ("Ann Emerg Med", "Annals of Internal Medicine"),
            ("JAMA", "JAMA Internal Medicine"),
        ] {
            assert!(!same_journal(a, b), "{a} ≠ {b}");
        }
    }

    #[test]
    fn journals_are_found_inside_references() {
        let words =
            |s: &str| -> Vec<String> { normalise(s).split(' ').map(str::to_owned).collect() };
        let text = words("Daum RS, et al. N Engl J Med. 2017;376(26):2545-55.");
        assert!(journal_in(&text, "New England Journal of Medicine"));
        assert!(!journal_in(&text, "The Lancet"));
        assert!(journal_in(&words("Lancet 2010;376:23"), "The Lancet"));
    }

    #[test]
    fn locators_from_common_styles() {
        let loc = |v: &str, p: &str| Locator {
            volume: Some(v.into()),
            page: Some(p.into()),
        };
        assert_eq!(
            locator("Daum RS et al. N Engl J Med 2017;376:2545"),
            loc("376", "2545")
        );
        assert_eq!(
            locator("N Engl J Med. 2014;370(15):1402-11."),
            loc("370", "1402")
        );
        assert_eq!(locator("Lancet. 2010; 376 (9734): 23-32"), loc("376", "23"));
        assert_eq!(
            locator("Meyer, G. (2014). Fibrinolysis. NEJM, 370(15), 1402–1411."),
            loc("370", "1402")
        );
        assert_eq!(
            locator("Nature, vol. 521, pp. 436-444, 2015"),
            loc("521", "436")
        );
        assert_eq!(locator("JAMA. 2014;311(11):e1117."), loc("311", "e1117"));
        assert_eq!(locator("Nature 521:436–444 (2015)"), loc("521", "436"));
        assert_eq!(
            locator("Injury, Int. J. Care Injured 47 (2016) 797–804"),
            loc("47", "797")
        );
        assert_eq!(locator("Nature 521, 436–444 (2015)"), loc("521", "436"));
        // A bracketed year without a page range: no place.
        assert_eq!(locator("Smith 47 (2016) 797 patients"), Locator::default());
        assert_eq!(
            locator("Vaswani 2017. doi:10.48550/arXiv.1706.03762 at 10:30"),
            Locator::default()
        );
        // APA without an issue (#80): a journal, its comma, then numbers.
        assert_eq!(
            locator(
                "Ragusa, A. (2021). Plasticenta. Environmental Science & Technology, 146, 106274."
            ),
            loc("146", "106274")
        );
        assert_eq!(
            locator("Smith, J. (2020). Title. Nature, 521, 436-444. https://doi.org/10.1/x"),
            loc("521", "436")
        );
        // Bare numbers elsewhere are not a place.
        for text in [
            "Smith, J. (2020). Title. Publisher, 2010, 12.",
            "We saw 12, 34 and 56, 78, 90 patients.",
            "Smith, J. (2020). Title. Journal, 146, 106274 and more",
        ] {
            assert_eq!(locator(text), Locator::default(), "{text}");
        }
    }

    #[test]
    fn the_cited_journal_is_read_where_a_reference_gives_it() {
        for (text, journal) in [
            (
                "Ragusa A, Svelato A, Santacroce C, et al. Plasticenta: first evidence of microplastics in human placenta. Environ Sci Technol. 2021;146:106274.",
                "Environ Sci Technol",
            ),
            (
                "Ragusa, A., Svelato, A. (2021). Plasticenta: First evidence of microplastics in human placenta. Environmental Science & Technology, 146, 106274.",
                "Environmental Science & Technology",
            ),
            (
                "Meyer G, et al. Fibrinolysis. N Engl J Med. 2014;370(15):1402-11.",
                "N Engl J Med",
            ),
            ("Daum RS et al. N Engl J Med 2017;376:2545", "N Engl J Med"),
            (
                "Daum RS. Fibrinolysis. N Engl J Med. 2017 Jun 29;376(26):2545-55.",
                "N Engl J Med",
            ),
            (
                "Lancet Infect Dis. 2020 Jan;20(1):e1-e10.",
                "Lancet Infect Dis",
            ),
            (
                "Meyer, G. (2014). Fibrinolysis. The New England Journal of Medicine, 370(15), 1402–1411.",
                "The New England Journal of Medicine",
            ),
            (
                "LeCun Y, Bengio Y, Hinton G. Deep learning. Nature 521, 436–444 (2015).",
                "Nature",
            ),
            ("Injury 47 (2016) 797–804", "Injury"),
            (
                "Doe J. A study of salts in water. J. Am. Chem. Soc. 2001;123:45-50.",
                "J. Am. Chem. Soc.",
            ),
            ("Smith J. Is it safe? eLife. 2020;9:e1234.", "eLife"),
        ] {
            assert_eq!(cited_journal(text).as_deref(), Some(journal), "{text}");
        }
    }

    #[test]
    fn a_journal_not_clearly_given_is_not_read() {
        for text in [
            // No journal, or no styled locator.
            "Vaswani A, et al. Attention is all you need. 2017.",
            "Goodfellow I, et al. Deep Learning. MIT Press; 2016.",
            "Nature, vol. 521, pp. 436-444, 2015",
            // A book or proceedings: a publisher or a year before the pages.
            "Smith J. Chapter. In: Jones K, ed. Handbook. Springer; 2010:12-30.",
            "He K. Deep residual learning. Proceedings of the IEEE Conference. 2016:770-778.",
            // A dotted name that may have begun earlier.
            "Injury, Int. J. Care Injured 47 (2016) 797–804",
            "Attention Is All You Need. Environ. Sci. Technol. 2021;146:106274.",
            // Lowercase or punctuated words: sentence-case names, notes.
            "Smith J. Title. Journal of clinical medicine. 2020;9:12-20.",
            "Smith J. Title. Lancet (London, England). 2020;395:12-20.",
            "Smith J, Jones K, N Engl J Med 2017;376:2545",
        ] {
            assert_eq!(cited_journal(text), None, "{text}");
        }
    }

    #[test]
    fn another_journal_is_clearly_another() {
        for (cited, actual) in [
            ("Environ Sci Technol", "Environment International"),
            (
                "Environmental Science & Technology",
                "Environment International",
            ),
            ("Lancet", "New England Journal of Medicine"),
            ("N Engl J Med", "Nature Medicine"),
        ] {
            assert!(other_journal(cited, actual), "{cited} ≠ {actual}");
        }
        for (cited, actual) in [
            ("Environ Int", "Environment International"),
            ("N Engl J Med", "New England Journal of Medicine"),
            ("New England Journal of Medicine", "N Engl J Med"),
            ("Lancet", "The Lancet"),
            ("The Lancet", "Lancet"),
            ("Br Med J", "BMJ"),
            ("BMJ", "BMJ (Clinical research ed.)"),
            (
                "Proc Natl Acad Sci U S A",
                "Proceedings of the National Academy of Sciences",
            ),
            (
                "PNAS",
                "Proceedings of the National Academy of Sciences of the United States of America",
            ),
            (
                "MMWR Morb Mortal Wkly Rep",
                "Morbidity and Mortality Weekly Report",
            ),
            ("Can J Anaesth", "Canadian Journal of Anesthesia"),
            ("Z Gastroenterol", "Zeitschrift für Gastroenterologie"),
            // Possibly a sister journal, possibly the same: not clearly.
            ("Lancet Infect Dis", "The Lancet"),
            ("", "The Lancet"),
        ] {
            assert!(!other_journal(cited, actual), "{cited} ~ {actual}");
        }
    }
}
