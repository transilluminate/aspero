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
/// Academy of Sciences", "BMJ" ↔ "BMJ (Clinical research ed.)"), nor
/// abbreviates the other's words in order, leaving some out ("Proc Biol
/// Sci" ↔ "Proceedings of the Royal Society B: Biological Sciences"), nor
/// is either the initials of a run of the other's words ("PNAS"). "Environ
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
        }) || abbreviates_in_order(short, long)
            || short.iter().any(|w| {
                (1..=long.len())
                    .flat_map(|n| long.windows(n))
                    .any(|run| run.len() > 1 && is_initials(w, run))
            })
    };
    !(inside(&c, &a) || inside(&a, &c))
}

/// Words of a full name an abbreviation may leave out, besides stopwords
/// and series letters: its structure, not its subject ("Am J Med" is not
/// the American Journal of Emergency Medicine).
const STRUCTURAL_WORDS: &[&str] = &["section", "part", "series", "royal", "society"];

/// Whether each of `short`'s words abbreviates one of `full`'s, in order,
/// with structural words of `full`'s left out between ("Acta Crystallogr
/// D" for "Acta Crystallographica Section D", "Proc Biol Sci" for
/// "Proceedings of the Royal Society B: Biological Sciences") or two run
/// together ("Cardiothorac" for "Cardio-Thoracic"). A long abbreviation
/// may carry a word the full name doesn't ("Lond" in "Philos Trans R Soc
/// Lond B Biol Sci"): one, among four or more that agree.
fn abbreviates_in_order(short: &[String], full: &[String]) -> bool {
    let structural = |w: &String| w.chars().count() == 1 || STRUCTURAL_WORDS.contains(&w.as_str());
    let mut rest = full;
    let mut missed = 0;
    for s in short {
        // As far as the first word that can't be left out.
        let reach = rest
            .iter()
            .position(|f| !structural(f))
            .map_or(rest.len(), |k| k + 1);
        let next = (0..reach).find_map(|i| {
            let joined = rest.get(i + 1).map(|next| format!("{}{next}", rest[i]));
            if loosely_abbreviates(s, &rest[i]) {
                Some(i + 1)
            } else {
                joined
                    .is_some_and(|j| loosely_abbreviates(s, &j))
                    .then_some(i + 2)
            }
        });
        match next {
            Some(next) => rest = &rest[next..],
            None => missed += 1,
        }
    }
    missed == 0 || (missed == 1 && short.len() > 4)
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
/// normalised reference, split into words). Not as a reference's other
/// words may: a one-word journal is named in full or by a prefix ("Circ"),
/// not any word with its letters ("mice" for "Medicine"), and a name's
/// initials ("SR", "N") are not a journal's.
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
    if let [one] = &full[..] {
        return text.iter().any(|w| w.len() >= 3 && one.starts_with(w));
    }
    text.windows(full.len()).any(|run| {
        run.iter().any(|w| w.len() > 1)
            && run.iter().zip(&full).all(|(s, f)| abbreviates_word(s, f))
    }) || text.iter().any(|w| w.len() > 2 && is_initials(w, &full))
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

/// A field of a BibTeX entry pasted as a reference (`@article{key, journal
/// = {Nature}, …}`), its braces dropped: `None` if the text isn't one or
/// lacks the field.
pub(crate) fn bibtex_field(text: &str, name: &str) -> Option<String> {
    let text = text.trim_start();
    if !text.starts_with('@') {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find(name) {
        let at = from + found;
        from = at + name.len();
        // The whole field name ("title", not "booktitle"), then "=".
        let whole = !lower[..at]
            .chars()
            .next_back()
            .is_some_and(char::is_alphanumeric);
        let Some(rest) = lower[from..].trim_start().strip_prefix('=') else {
            continue;
        };
        if !whole {
            continue;
        }
        let start = text.len() - rest.trim_start().len();
        let close = match text[start..].chars().next()? {
            '{' => '}',
            '"' => '"',
            _ => {
                // A bare number: `year = 2020`.
                let bare: String = text[start..]
                    .chars()
                    .take_while(char::is_ascii_alphanumeric)
                    .collect();
                return (!bare.is_empty()).then_some(bare);
            }
        };
        let mut depth = 0usize;
        let mut value = String::new();
        for c in text[start + 1..].chars() {
            match c {
                c if c == close && depth == 0 => {
                    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
                    return (!value.is_empty()).then_some(value);
                }
                '{' => depth += 1,
                '}' => depth = depth.saturating_sub(1),
                c => value.push(c),
            }
        }
        return None;
    }
    None
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
        // Harvard labels the pages: "Cell, 144(5), pp. 646-674".
        let labelled_page = is_page_label(tokens.get(i + 1).copied().unwrap_or(""));
        let page_at = if labelled_page { i + 2 } else { i + 1 };
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
        // the journal's word.
        if tokens[i].ends_with(',')
            && let Some(volume) = volume_number(token)
            && let Some((pages, year)) = next_page.and_then(|p| p.split_once('('))
            && bracketed_year(year)
            && (ranged(pages) || (i > 0 && name_word(tokens[i - 1])))
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
/// Technology, 146, 106274", Harvard "'Title', Cell, 145(5), pp. 646-674",
/// running headers "Nature 521, 436–444 (2015)"), each capitalised (or a
/// joining word: "of", "&"), back to the end of the sentence before (the
/// title's, or "et al.") or the start of the text. Abbreviations with stops
/// ("J. Am. Chem. Soc.") are read too, back to a sentence ending in a
/// lowercase word; a title ending in a capitalised word ("… in Wuhan,
/// China. Lancet. 2020;395:497") is not one.
///
/// Without a styled locator, only after a title that has clearly ended (a
/// sentence's stop, or a closing quote): before the year ending the
/// reference ("… Wuhan, China. JAMA. 2020."), its links ("… Polya.
/// Journal of Applied Physiology. `https://doi.org/…`"), or a labelled
/// locator (IEEE "\"…,\" Journal of Clinical Investigation, vol. 8, p.
/// 109", "JAMA, no. 10223"); or ending the reference after a quoted title
/// (Harvard "'…', Chest.").
///
/// `None` when anything is unusual: a publisher in the locator ("Springer;
/// 2010:12-30") or before the year ("MIT Press; 2016."), a lowercase or
/// numbered word, punctuation inside the name, or a run of dotted
/// abbreviations that may have begun earlier ("Injury, Int. J. Care
/// Injured 47 (2016)"). A journal misread would be called another journal,
/// so better none.
pub(crate) fn cited_journal(text: &str) -> Option<String> {
    if text.trim_start().starts_with('@') {
        // BibTeX: its journal field.
        return bibtex_field(text, "journal");
    }
    let compact = compact(text);
    let tokens: Vec<&str> = compact.split_whitespace().collect();
    let (at, after) = match styled(&tokens) {
        Some((at, found)) => {
            // "Springer;2010:12-30": a publisher, not a year, before the
            // volume; "Proceedings of X. 2016:770-778": a year, not a volume.
            let head = tokens[at].split([';', ':', '(', ',']).next().unwrap_or("");
            let semicolon = tokens[at].contains(';');
            if (semicolon && !is_digits(head) && !is_month(head))
                || (!semicolon && found.volume.as_deref().is_some_and(is_year))
            {
                return None;
            }
            (at, After::Anything)
        }
        None => unstyled_end(&tokens)?,
    };
    // The date between journal and locator: "N Engl J Med. 2017 Jun 29;376…".
    let mut end = at;
    while end > 0 && is_date_word(tokens[end - 1]) {
        end -= 1;
    }
    let last = *tokens[..end].last()?;
    // The journal's last word, with the stop or comma ending it (always,
    // without a styled locator).
    let ending = last.strip_suffix(['.', ',']);
    let last_word = ending.unwrap_or(last);
    if !name_word(last_word)
        || is_stopword(last_word)
        || (after != After::Anything && ending.is_none())
    {
        return None;
    }
    // Abbreviated with stops: the word before ends with one too. Or it's
    // the end of a title in title case ("… with AlphaFold. Science.
    // 2021;596:583", "… Wuhan, China. Lancet. 2020;395:497"): read
    // undotted, back to it.
    let dotted = last.ends_with('.')
        && end > 1
        && tokens[end - 2]
            .strip_suffix('.')
            .is_some_and(|w| name_word(w) && capitalised(w));
    dotted
        .then(|| read_journal(&tokens, end, true, after))
        .flatten()
        .or_else(|| read_journal(&tokens, end, false, after))
}

/// What a journal read from a reference must follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum After {
    /// Anything, or nothing: before a styled locator, a running header
    /// ("Injury 47 (2016) 797–804") is all journal.
    Anything,
    /// The title, ended by a stop or a closing quote.
    Title,
    /// A quoted title (Harvard's "'…', Chest.").
    Quoted,
}

/// Where the journal ends in a reference without a styled locator, and
/// what it must follow ([`cited_journal`]).
fn unstyled_end(tokens: &[&str]) -> Option<(usize, After)> {
    // Labelled: "Journal of Clinical Investigation, vol. 8, p. 109".
    let label = |t: &&str| {
        matches!(
            t.trim_end_matches(['.', ',']).to_ascii_lowercase().as_str(),
            "vol" | "volume" | "no" | "pp" | "p"
        )
    };
    if let Some(i) = tokens.iter().position(label) {
        return (i > 0 && tokens[i - 1].ends_with(',')).then_some((i, After::Title));
    }
    // The links ending it, then the year: "JAMA. 2020. doi:10.1001/…".
    let mut end = tokens.len();
    while end > 0 && is_link(tokens[end - 1]) {
        end -= 1;
    }
    let mut dated = end;
    while dated > 0 && is_date_word(tokens[dated - 1]) {
        dated -= 1;
    }
    let year = tokens[dated..end]
        .iter()
        .any(|t| is_year(t.trim_matches(|c: char| !c.is_alphanumeric())));
    Some(if year {
        (dated, After::Title)
    } else if end < tokens.len() {
        (end, After::Title)
    } else {
        (end, After::Quoted)
    })
}

/// "pp." or "p.", before a page.
fn is_page_label(t: &str) -> bool {
    matches!(
        t.trim_end_matches(['.', ',']).to_ascii_lowercase().as_str(),
        "pp" | "p"
    )
}

/// A link or identifier ending a reference: a DOI, URL, PMID or arXiv ID.
fn is_link(t: &str) -> bool {
    ["doi", "http", "www.", "pmid", "pmcid", "arxiv", "isbn"]
        .iter()
        .any(|p| starts_with_ci(t, p))
}

/// A word closing a quoted title: `tool',`, `C-fibers,"`, `graphs,”`.
fn closes_quote(t: &str) -> bool {
    t.trim_end_matches([',', '.'])
        .ends_with(['\'', '’', '"', '”'])
}

/// The journal whose last word is `tokens[end - 1]`, read back to the
/// sentence before ([`cited_journal`]), following what `after` says.
/// `dotted`: abbreviated with stops, the name kept as printed, stops and
/// all.
fn read_journal(tokens: &[&str], end: usize, dotted: bool, after: After) -> Option<String> {
    let last = tokens[end - 1];
    let mut k = end - 1;
    let mut words = vec![if dotted {
        last
    } else {
        last.strip_suffix(['.', ',']).unwrap_or(last)
    }];
    // What ended the name: a closing quote (true) or a stop (false).
    let mut ended = None;
    while k > 0 {
        let t = tokens[k - 1];
        let stopped = t.strip_suffix('.');
        if closes_quote(t) {
            ended = Some(true);
            break;
        }
        if dotted {
            let abbreviated = stopped.is_some_and(|w| name_word(w) && capitalised(w));
            if abbreviated || t == "&" || is_stopword(t) {
                words.push(t);
            } else if sentence_end(t) && !capitalised(t) {
                ended = Some(false);
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
            ended = Some(false);
            break;
        } else {
            return None;
        }
        if words.len() > JOURNAL_WORDS {
            return None;
        }
        k -= 1;
    }
    let follows = match after {
        After::Anything => true,
        After::Title => ended.is_some(),
        After::Quoted => ended == Some(true),
    };
    if !follows {
        return None;
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
        assert!(journal_in(&words("Circ 2010;121:23"), "Circulation"));
        assert!(journal_in(&words("Smith J. NEJM 2010"), "N Engl J Med"));
        // Names, initials and title words are not the journal.
        for (text, journal) in [
            ("Ragusa A, Rossi SR. Title. 2021", "Scientific Reports"),
            ("Doe J. Sepsis in mice. 2020", "Medicine"),
            ("Smith N. Title. 2020", "Nature"),
            ("Smith J. A. Title. 2020", "Journal of Anaesthesia"),
        ] {
            assert!(!journal_in(&words(text), journal), "{text}: {journal}");
        }
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
            // A title ending in a capitalised word is not a dotted name.
            (
                "Huang C, Wang Y, Li X, et al. Clinical features of patients infected with 2019 novel coronavirus in Wuhan, China. Lancet. 2020;395(10223):497-506.",
                "Lancet",
            ),
            (
                "Jumper J, et al. Highly accurate protein structure prediction with AlphaFold. Science. 2021;596(7873):583-589.",
                "Science",
            ),
            ("Smith J. Deep Learning. Nature. 2015;521:436.", "Nature"),
            // Harvard: a quoted title, "pp." before the pages; BibTeX.
            (
                "Altschul, S.F. et al. (1990) 'Basic local alignment search tool', Journal of Molecular Biology, 215(3), pp. 403-410.",
                "Journal of Molecular Biology",
            ),
            (
                "@article{p, author = {Pereira, Sueli}, title = {Conectando alunos}, journal = {Revista Foco}, year = {2024}}",
                "Revista Foco",
            ),
            // No styled locator: before the year ending the reference, its
            // links, or a labelled locator, after a title that has ended.
            (
                "Huang C, et al. Clinical features of patients infected with 2019 novel coronavirus in Wuhan, China. JAMA. 2020.",
                "JAMA",
            ),
            (
                "Hochreiter S, Schmidhuber J. Long short-term memory. Neural Netw. 1998.",
                "Neural Netw",
            ),
            (
                "Mahat DB. Use of conditioned media. J Clin Invest. 2016 Mar 3. doi:10.1172/x",
                "J Clin Invest",
            ),
            (
                "Pratiwi, N. Y., & Hidayat, W. (2020). Kesulitan Siswa Madrasah Ibtidaiyah pada Materi Pecahan Berdasarkan Langkah Polya. Journal of Applied Physiology. https://doi.org/10.33603/jnpm.v4i2.3476",
                "Journal of Applied Physiology",
            ),
            (
                "A. Neishabouri, \"Saltatory conduction in unmyelinated axons,\" Journal of Clinical Investigation, vol. 8, p. 109, 2014.",
                "Journal of Clinical Investigation",
            ),
            (
                "C. Huang et al., \"Clinical features of patients in Wuhan, China,\" JAMA, no. 10223, 2020.",
                "JAMA",
            ),
            (
                "Byrd, J.C. et al. (2003) 'A phase II study of cladribine: results from CALGB Study 9211', Chest.",
                "Chest",
            ),
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
            "Doe J. Water in New York City. J. Am. Chem. Soc. 2001;123:45-50.",
            // Lowercase or punctuated words: sentence-case names, notes.
            "Smith J. Title. Journal of clinical medicine. 2020;9:12-20.",
            "Smith J. Title. Lancet (London, England). 2020;395:12-20.",
            "Smith J, Jones K, N Engl J Med 2017;376:2545",
            // Without a styled locator, nothing that may be the title, a
            // publisher or a name.
            "Smith J. Deep learning in medicine. 2020.",
            "Landsman K. I Am a Strange Loop. 2007.",
            "Kahneman D. Thinking, Fast and Slow. New York: Farrar, Straus and Giroux; 2011.",
            "Smith J. Some title. Nature.",
            "Ji J, Chen W. Mitigating Deceptive Alignment via Self-Monitoring. 2025. arXiv:2505.18807",
            "Huang et al. described in The Lancet (2020) — \"Clinical features of patients\".",
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
            ("Ann Intern Med", "Annals of Emergency Medicine"),
            // Sister journals: the full name has a subject word the
            // abbreviation lacks.
            ("Am J Med", "American Journal of Emergency Medicine"),
            ("Ann Surg", "Annals of Thoracic Surgery"),
            (
                "J Am Coll Cardiol",
                "Journal of the American College of Surgeons",
            ),
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
            // NLM abbreviations that leave words out, add one ("Lond") or
            // run two together.
            (
                "Proc Biol Sci",
                "Proceedings of the Royal Society B: Biological Sciences",
            ),
            (
                "Philos Trans R Soc Lond B Biol Sci",
                "Philosophical Transactions of the Royal Society B: Biological Sciences",
            ),
            (
                "Acta Crystallogr D Biol Crystallogr",
                "Acta Crystallographica Section D Biological Crystallography",
            ),
            (
                "Eur J Cardiothorac Surg",
                "European Journal of Cardio-Thoracic Surgery",
            ),
            // Possibly a sister journal, possibly the same: not clearly.
            ("Lancet Infect Dis", "The Lancet"),
            ("", "The Lancet"),
        ] {
            assert!(!other_journal(cited, actual), "{cited} ~ {actual}");
        }
    }
}
