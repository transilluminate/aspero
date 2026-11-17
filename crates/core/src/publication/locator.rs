//! Where a reference says a work is: its volume and first page, read from
//! the citation styles that give them (Vancouver, APA, Harvard, IEEE, MLA, …).

use crate::id::{is_digits, starts_with_ci};
use crate::work::PLAUSIBLE_YEARS;

use super::bibtex::bibtex_field;
use super::cited_journal::{is_link, name_word};

/// The first page of a page or range: "2545" from "2545-55" or "2545–2555".
pub fn first_page(pages: &str) -> &str {
    // Up to whatever separates the last page, a dash however a record
    // garbles it ("205???210").
    let pages = pages.trim();
    let end = pages
        .find(|c: char| !(c.is_alphanumeric() || c == '.'))
        .unwrap_or(pages.len());
    &pages[..end]
}

/// Volume and first page as a reference gives them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Locator {
    pub volume: Option<String>,
    pub page: Option<String>,
}

/// The volume and first page in a free-text reference, from the common
/// styles: Vancouver ("2017;376(26):2545-55"), APA ("376(26), 2545–2555",
/// or without an issue "Journal Name, 146, 106274."), Harvard ("376(26),
/// pp. 2545–2555"), Nature ("Nature 530, 484 (2016)"), labelled ("vol.
/// 376, pp. 2545–2555") and a BibTeX entry's fields.
pub(crate) fn locator(text: &str) -> Locator {
    if text.trim_start().starts_with('@') {
        // BibTeX: its fields.
        return Locator {
            volume: bibtex_field(text, "volume").and_then(|v| volume_number(&v)),
            page: bibtex_field(text, "pages").and_then(|p| page_number(&p)),
        };
    }
    let compact = compact(text);
    let tokens: Vec<&str> = compact.split_whitespace().collect();
    if let Some((_, found)) = styled(&tokens) {
        return found;
    }
    // Chicago without pages: "Journal 94, no. 2 (2004)."
    let found = labelled(&tokens);
    Locator {
        volume: found
            .volume
            .or_else(|| dated_end(&tokens).and_then(|(_, volume)| volume)),
        ..found
    }
}

/// Whether a free-text reference gives an issue with its volume, as a
/// journal's article is cited: Vancouver's "2019;23(5):159-175", APA's
/// "22(2), 3–16", Chicago's "94, no. 2". A book series' volume
/// ("Lecture Notes in Computer Science, vol. 9351") has none.
pub(crate) fn cites_issue(text: &str) -> bool {
    if text.trim_start().starts_with('@') {
        return bibtex_field(text, "number").is_some() && bibtex_field(text, "volume").is_some();
    }
    let compact = compact(text);
    let tokens: Vec<&str> = compact.split_whitespace().collect();
    let Some((i, _)) = styled(&tokens) else {
        return false;
    };
    let token = tokens[i];
    let head = token.split(':').next().unwrap_or(token);
    let head = head.rsplit(';').next().unwrap_or(head);
    let bracketed = head.split_once('(').is_some_and(|(_, rest)| {
        let issue = rest.split(')').next().unwrap_or("");
        !issue.is_empty() && issue.len() <= 8 && !bracketed_year(rest.trim_end_matches([',', '.']))
    });
    bracketed || tokens.get(i + 1).is_some_and(|t| is_label(t, &["no"]))
}

/// The reference with the spaces that styles put around their separators
/// dropped, so each locator is one token: "2014; 370 : 1402" →
/// "2014;370:1402", "376 (9734)" → "376(9734)".
pub(super) fn compact(text: &str) -> String {
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
        // A page label run into its page: "pp.58-63", "p.054433".
        if c.is_ascii_digit() {
            let label = compact
                .rsplit(char::is_whitespace)
                .next()
                .is_some_and(|t| t == "pp." || t == "p.");
            if label {
                compact.push(' ');
            }
        }
        compact.push(c);
    }
    compact
}

/// The first styled locator among a compacted reference's tokens, with the
/// index of the token it starts at: what comes before it is the journal.
pub(super) fn styled(tokens: &[&str]) -> Option<(usize, Locator)> {
    let at = |volume: String, page: String| Locator {
        volume: Some(volume),
        page: Some(page),
    };
    for (i, token) in tokens.iter().enumerate() {
        let token = token.trim_end_matches(['.', ',']);
        // Vancouver: [year;]volume[(issue)]:page[-end], with a year, issue
        // or page range to tell it from a time of day ("at 10:30").
        // MLA: volume.issue (year): page[-end], "77.4 (2005): 1321-1373".
        if let Some((head, pages)) = token.split_once(':') {
            let volume = head.rsplit(';').next().unwrap_or(head);
            let styled = head.contains([';', '(']) || pages.contains(['-', '–', '—']);
            if styled
                && let (Some(volume), Some(page)) = (
                    volume_number(volume).or_else(|| mla_volume(volume)),
                    page_number(pages),
                )
            {
                return Some((i, at(volume, page)));
            }
        }
        // Harvard labels the pages: "Cell, 144(5), pp. 646-674".
        let labelled_page = is_page_label(tokens.get(i + 1).copied().unwrap_or(""));
        let page_at = if labelled_page { i + 2 } else { i + 1 };
        // Chicago: "Journal 94, no. 2 (2004): 317-321", the issue labelled,
        // the year bracketed before the pages.
        if tokens[i].ends_with(',')
            && let Some(volume) = volume_number(token)
            && tokens.get(i + 1).is_some_and(|t| is_label(t, &["no"]))
            && let Some((head, pages)) = tokens.get(i + 2).and_then(|t| t.split_once(':'))
            && head.split_once('(').is_some_and(|(_, y)| bracketed_year(y))
            && let Some(page) = page_number(pages)
        {
            return Some((i, at(volume, page)));
        }
        // APA: volume(issue), page[–end]
        if tokens[i].ends_with(',')
            && token.contains('(')
            && let Some(volume) = volume_number(token)
            && let Some(page) = tokens.get(page_at).and_then(|p| page_number(p))
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
        // Nature's single page, "Nature 530, 484 (2016)", only right after
        // the journal's word, abbreviated or not ("J. Org. Chem. 101, 6301
        // (1994)").
        if tokens[i].ends_with(',')
            && let Some(volume) = volume_number(token)
            && let Some((pages, year)) = next_page.and_then(|p| p.split_once('('))
            && bracketed_year(year)
            && (ranged(pages)
                || (i > 0 && name_word(tokens[i - 1].strip_suffix('.').unwrap_or(tokens[i - 1]))))
            && let Some(page) = page_number(pages)
        {
            return Some((i, at(volume, page)));
        }
        // APA without an issue: "Journal Name, 146, 106274." Bare numbers
        // after commas are common, so only right after a word and its
        // comma (the journal), not a year, and with the page ending the
        // reference or followed by its link, or labelled ("pp.").
        if tokens[i].ends_with(',')
            && let Some(volume) = volume_number(token).filter(|v| !is_year(v))
            && i > 0
            && tokens[i - 1]
                .strip_suffix(',')
                .is_some_and(|w| !w.is_empty() && w.chars().all(char::is_alphabetic))
            && let Some(page) = tokens.get(page_at).and_then(|p| page_number(p))
            && (labelled_page
                || tokens[page_at].ends_with('.')
                || tokens
                    .get(page_at + 1)
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
            // IEEE's article number, where the page would be: "Art. no. 615".
            "art" if found.page.is_none() && next().is_some_and(|n| is_label(n, &["no"])) => {
                found.page = tokens.get(i + 2).and_then(|p| page_number(p));
            }
            _ => {}
        }
    }
    found
}

/// "pp." or "p.", before a page.
fn is_page_label(t: &str) -> bool {
    is_label(t, &["pp", "p"])
}

/// A label, one of `labels`, as written: "no.", "Vol.", "pp.,".
pub(super) fn is_label(t: &str, labels: &[&str]) -> bool {
    let t = t.trim_end_matches(['.', ',']);
    labels.iter().any(|l| l.eq_ignore_ascii_case(t))
}

/// Chicago's end, a bracketed year after the journal (and its volume and
/// issue), its links aside: "Journal 94, no. 2 (2004).", "Mind 133
/// (2024).", "eClinicalMedicine (2010).". Where the journal ends, and the
/// volume given.
pub(super) fn dated_end(tokens: &[&str]) -> Option<(usize, Option<String>)> {
    let mut end = tokens.len();
    while end > 0 && is_link(tokens[end - 1]) {
        end -= 1;
    }
    let (number, year) = tokens[..end]
        .last()?
        .trim_end_matches(['.', ','])
        .split_once('(')?;
    if !bracketed_year(year) {
        return None;
    }
    if number.is_empty() {
        return Some((end - 1, None));
    }
    // The volume before a labelled issue: "94, no. 2 (2004)".
    if end >= 3 && is_label(tokens[end - 2], &["no"]) {
        let volume = tokens[end - 3].strip_suffix(',').and_then(volume_number)?;
        return Some((end - 3, Some(volume)));
    }
    // Right after the journal's word: "Psychological Science 5 (1994)".
    let journal = end >= 2 && name_word(tokens[end - 2].trim_end_matches('.'));
    journal.then(|| (end - 1, volume_number(number)))
}

pub(super) fn is_year(w: &str) -> bool {
    w.len() == 4 && w.parse().is_ok_and(|y| PLAUSIBLE_YEARS.contains(&y))
}

pub(crate) fn is_month(w: &str) -> bool {
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
pub(super) fn bracketed_year(s: &str) -> bool {
    s.strip_suffix(')')
        .and_then(|y| y.parse().ok())
        .is_some_and(|y| PLAUSIBLE_YEARS.contains(&y))
}

/// A page range: "797–804".
pub(super) fn ranged(pages: &str) -> bool {
    pages.contains(['-', '–', '—'])
}

/// MLA's volume and issue before the bracketed year: "77.4(2005)" → "77".
fn mla_volume(s: &str) -> Option<String> {
    let (numbers, year) = s.split_once('(')?;
    let (volume, issue) = numbers.split_once('.')?;
    (bracketed_year(year) && is_digits(issue) && issue.len() <= 6)
        .then(|| volume_number(volume))
        .flatten()
}

/// "376" or "376(26)": the volume number.
pub(super) fn volume_number(s: &str) -> Option<String> {
    let volume = s.split('(').next().unwrap_or(s);
    (volume.len() <= 6 && is_digits(volume)).then(|| volume.to_owned())
}

/// "2545-55", "e1234", "S12" or "R3" (a letter for a section or
/// supplement: Nucleic Acids Research's "D1", a review's "R3"), or a
/// supplement's roman number ("ii5", Annals of Oncology's): the first page.
pub(super) fn page_number(s: &str) -> Option<String> {
    let page = first_page(s.trim_end_matches(['.', ',', ';']));
    let roman = page
        .find(|c: char| !matches!(c, 'i' | 'v' | 'x'))
        .filter(|&n| (2..=4).contains(&n))
        .map(|n| &page[n..]);
    let digits = roman.unwrap_or_else(|| {
        page.strip_prefix(|c: char| c.is_ascii_alphabetic())
            .unwrap_or(page)
    });
    (digits.len() <= 8 && is_digits(digits)).then(|| page.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // Red team (#102): Nature's single page, Harvard's "pp.", BibTeX.
        assert_eq!(
            locator("Silver, D. et al. Mastering the game of Go. Nature 530, 484 (2016)."),
            loc("530", "484")
        );
        assert_eq!(
            locator("Hanahan, D. (2011) 'Hallmarks of cancer', Cell, 144(5), pp. 646-674."),
            loc("144", "646")
        );
        assert_eq!(
            locator("LeCun, Y. (2015) 'Deep learning', Nature, 521, pp. 436-444."),
            loc("521", "436")
        );
        assert_eq!(
            locator(
                "@article{j, author = {Jumper, John}, title = {Highly accurate {AlphaFold}}, \
                 journal = {Nature}, year = {2021}, volume = {597}, pages = {583--589}}"
            ),
            loc("597", "583")
        );
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
}
