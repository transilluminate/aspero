//! Where a citation says a work appeared: journal, volume, first page.
//!
//! Journals are compared by rule rather than with an abbreviation list:
//! each word of an abbreviation must abbreviate the matching significant
//! word of the full title, in order ("N Engl J Med" ↔ "New England Journal
//! of Medicine", "Lancet" ↔ "The Lancet"), or the abbreviation is the
//! initials ("NEJM", "JAMA", "BMJ"). Pairs the rule can't see ("Br Med J" ↔
//! "BMJ") are reported as different, so callers treat a journal mismatch as
//! a hint, not proof. Volume and page are numbers and compare exactly.

use crate::id::is_digits;
use crate::matching::normalise;
use crate::work::PLAUSIBLE_YEARS;

/// Words a journal abbreviation leaves out.
const JOURNAL_STOPWORDS: &[&str] = &[
    "the", "of", "and", "for", "in", "on", "de", "der", "des", "du", "la", "le", "und",
];

/// Whether two journal names are the same journal: equal, or one
/// abbreviates the other.
pub fn same_journal(a: &str, b: &str) -> bool {
    let (a, b) = (significant(a), significant(b));
    !a.is_empty() && (a == b || abbreviates(&a, &b) || abbreviates(&b, &a))
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
/// styles: Vancouver ("2017;376(26):2545-55"), APA ("376(26), 2545–2555")
/// and labelled ("vol. 376, pp. 2545–2555").
pub(crate) fn locator(text: &str) -> Locator {
    // "2014; 370 : 1402" → "2014;370:1402"
    let mut compact = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_whitespace() && compact.ends_with([';', ':']) {
            continue;
        }
        // "376 (9734)" → "376(9734)"
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
    let tokens: Vec<&str> = compact.split_whitespace().collect();
    let mut found = Locator::default();
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
                return Locator {
                    volume: Some(volume),
                    page: Some(page),
                };
            }
        }
        // APA: volume(issue), page[–end]
        if tokens[i].ends_with(',')
            && token.contains('(')
            && let Some(volume) = volume_number(token)
            && let Some(page) = tokens.get(i + 1).and_then(|p| page_number(p))
        {
            return Locator {
                volume: Some(volume),
                page: Some(page),
            };
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
            return Locator {
                volume: Some(volume),
                page: Some(page),
            };
        }
        if tokens[i].ends_with(',')
            && let Some(volume) = volume_number(token)
            && let Some((pages, year)) = next_page.and_then(|p| p.split_once('('))
            && bracketed_year(year)
            && ranged(pages)
            && let Some(page) = page_number(pages)
        {
            return Locator {
                volume: Some(volume),
                page: Some(page),
            };
        }
        let label = token.to_ascii_lowercase();
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
    }
}
