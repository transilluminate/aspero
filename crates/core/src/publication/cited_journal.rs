//! The journal a free-text reference cites: read before its locator, or
//! where a style puts it, and only when it can be told.

use crate::id::{is_digits, starts_with_ci};
use crate::matching::normalise;

use super::bibtex::bibtex_field;
use super::conference::proceedings;
use super::journal::JOURNAL_STOPWORDS;
use super::locator::{
    bracketed_year, compact, dated_end, is_label, is_month, is_year, page_number, ranged, styled,
    volume_number,
};

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
    match read_cited_journal(text)? {
        Reading::Sure(journal) => Some(journal),
        Reading::Maybe(_) => None,
    }
}

/// The journals a free-text reference may cite: the one
/// [`cited_journal`] reads, else, where a run of dotted abbreviations
/// follows a title ending in a capitalised word ("… by Nanoparticles. J.
/// Am. Chem. Soc. 113, 3391", "… Sri Lanka. Cad. Saúde Pública.
/// 2005;2:e185"), each reading starting after a stop in it, longest first:
/// one of them is the journal, so it is clearly another only when every
/// one is.
pub(crate) fn cited_journals(text: &str) -> Vec<String> {
    match read_cited_journal(text) {
        Some(Reading::Sure(journal)) => vec![journal],
        Some(Reading::Maybe(readings)) => readings,
        None => Vec::new(),
    }
}

/// A journal read from a reference.
enum Reading {
    /// Told with confidence.
    Sure(String),
    /// One of these, where the title ends unclear.
    Maybe(Vec<String>),
}

fn read_cited_journal(text: &str) -> Option<Reading> {
    if text.trim_start().starts_with('@') {
        // BibTeX: its journal field.
        return bibtex_field(text, "journal").map(Reading::Sure);
    }
    let compact = compact(text);
    let tokens: Vec<&str> = compact.split_whitespace().collect();
    told(&tokens).or_else(|| {
        styled(&tokens)
            .is_none()
            .then(|| unstopped_journal(&tokens).or_else(|| named_after_quote(&tokens)))
            .flatten()
            .map(|journal| Reading::Maybe(vec![journal]))
    })
}

/// [`read_cited_journal`]'s reading where a style tells the journal.
fn told(tokens: &[&str]) -> Option<Reading> {
    if let Some(journal) = prose_journal(tokens) {
        return Some(Reading::Sure(journal));
    }
    let (at, after) = match styled(tokens) {
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
        None => unstyled_end(tokens)?,
    };
    // The date between journal and locator: "N Engl J Med. 2017 Jun 29;376…".
    let mut end = at;
    while end > 0 && is_date_word(tokens[end - 1]) {
        end -= 1;
    }
    if let Some(journal) = quoted_journal(tokens, end) {
        return Some(Reading::Sure(journal));
    }
    let last = without_comma_after_stop(tokens[..end].last()?);
    // The journal's last word, with the stop or comma ending it (always,
    // after a title and before a year, a link or a label).
    let ending = last.strip_suffix(['.', ',']);
    let last_word = ending.unwrap_or(last);
    if !name_word(last_word)
        || is_stopword(last_word)
        || (matches!(after, After::Title | After::Quoted) && ending.is_none())
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
    // Undotted, a short abbreviation after another ("… Creation. Psychol.
    // Sci. 5, 14"; not initials, "JAMA"): the title may have ended
    // before either.
    let undotted = read_journal(tokens, end, false, after).filter(|j| {
        let short = !j.contains(' ')
            && last.ends_with('.')
            && j.chars().count() <= 4
            && j.chars().any(char::is_lowercase);
        !(dotted && short)
    });
    dotted
        .then(|| read_journal(tokens, end, true, after))
        .flatten()
        .or(undotted)
        .map(Reading::Sure)
        .or_else(|| readings(tokens, end, after))
}

/// Words that make a name a publisher's, not a journal's.
const PUBLISHER_WORDS: &[&str] = &[
    "press",
    "publishing",
    "publisher",
    "publishers",
    "publications",
    "verlag",
    "books",
    "edition",
    "editions",
];

/// A journal named without the stop that ends it, the year closing the
/// reference after it: "… Janani Suraksha Yojana in India. Popul Space
/// Place 2018." (wild-5 #375). One reading, not a sure one: back to the
/// title's stop, every word a name's, and no publisher ("Oxford
/// University Press 2018").
fn unstopped_journal(tokens: &[&str]) -> Option<String> {
    let (end, After::Title) = unstyled_end(tokens)? else {
        return None;
    };
    let last = *tokens[..end].last()?;
    if end == tokens.len()
        || !is_date_word(tokens[end])
        || last.ends_with(['.', ','])
        || !name_word(last)
        || is_stopword(last)
    {
        return None;
    }
    read_journal(tokens, end, false, After::Title).filter(|j| !publisher(j) && !proceedings(j))
}

/// The name right after a quoted title, ended by a comma, before a note
/// in lowercase: "\"Bridging the domestic building fabric performance
/// gap,\" BSERT, a follow-up on remedies." (wild-5 #560). One reading, not
/// a sure one. Not a dotted abbreviation ("Invent. math., vol. 154"), a
/// book's "In …", a publisher, or a conference.
fn named_after_quote(tokens: &[&str]) -> Option<String> {
    let quote = tokens.iter().rposition(|t| closes_quote(t))?;
    let mut words: Vec<&str> = Vec::new();
    for (at, t) in tokens.iter().enumerate().skip(quote + 1) {
        let listed = t.strip_suffix(',');
        let word = listed.unwrap_or(t);
        if !name_word(word) || words.len() >= JOURNAL_WORDS {
            return None;
        }
        words.push(word);
        if listed.is_some() {
            let note = tokens
                .get(at + 1)
                .is_some_and(|n| n.starts_with(char::is_lowercase) && !n.contains('.'));
            let first = words[0];
            let name = words.join(" ");
            return (note
                && capitalised(first)
                && !is_stopword(&first.to_lowercase())
                && !publisher(&name)
                && !proceedings(&name))
            .then_some(name);
        }
    }
    None
}

fn publisher(name: &str) -> bool {
    normalise(name)
        .split(' ')
        .any(|w| PUBLISHER_WORDS.contains(&w))
}

/// The journal between a quoted title and `end`, as written, in sentence
/// case too ("“…” Revue historique 35, no. 1 (1974): 130", "'…', Journal
/// of Molecular Biology, 215(3)"): words without digits, stops only
/// abbreviating, back to the closing quote.
fn quoted_journal(tokens: &[&str], end: usize) -> Option<String> {
    let mut k = end;
    while k > 0 && end - k <= JOURNAL_WORDS {
        let t = without_comma_after_stop(tokens[k - 1]);
        if closes_quote(t) {
            let words = &tokens[k..end];
            let first = *words.first()?;
            // The last word's stop kept in a dotted name ("Mach. Intell.").
            let last = without_comma_after_stop(words[words.len() - 1]);
            let dotted = words.len() > 1 && words[words.len() - 2].ends_with('.');
            let last = match last.strip_suffix(['.', ',']) {
                Some(word) if !(dotted && last.ends_with('.')) => word,
                _ => last,
            };
            let mut words = words[..words.len() - 1].to_vec();
            words.push(last);
            return (first != "&" && !is_stopword(first) && capitalised(first))
                .then(|| spaced_colons(&words.join(" ")));
        }
        let word = t.strip_suffix(['.', ',']).unwrap_or(t);
        let inner = k < end && t.ends_with(',');
        let abbreviation = !t.ends_with('.') || capitalised(word);
        if inner
            || !abbreviation
            || word.is_empty()
            || !word
                .chars()
                .all(|c| c.is_alphabetic() || matches!(c, '-' | '\'' | '’' | '&' | ':'))
        {
            return None;
        }
        k -= 1;
    }
    None
}

/// The journal whose last word is `tokens[end - 1]` where the title's end
/// is unclear: back through the name's words, stopped or not, each stop a
/// place the title may have ended and the journal begun. The reading
/// after a closing quote is sure (IEEE's "“…,” IEEE Trans. Pattern Anal.
/// Mach. Intell., vol. 56").
fn readings(tokens: &[&str], end: usize, after: After) -> Option<Reading> {
    let mut words = vec![without_comma_after_stop(tokens[end - 1])];
    let reading = |words: &[&str]| -> Option<String> {
        let first = *words.last()?;
        (first != "&" && !is_stopword(first))
            .then(|| spaced_colons(&words.iter().rev().copied().collect::<Vec<_>>().join(" ")))
    };
    let mut found: Vec<String> = Vec::new();
    let mut k = end - 1;
    while k > 0 && words.len() <= JOURNAL_WORDS {
        let t = tokens[k - 1];
        if closes_quote(t) {
            return reading(&words).map(Reading::Sure);
        }
        if sentence_end(t) {
            found.extend(reading(&words));
        }
        let name = t == "&"
            || is_stopword(t)
            || name_word(t.strip_suffix('.').unwrap_or(t))
            || colon_name(t);
        if !name {
            break;
        }
        words.push(t);
        k -= 1;
    }
    if k == 0 && after == After::Anything {
        found.extend(reading(&words));
    }
    if after == After::Quoted || found.is_empty() {
        return None;
    }
    found.reverse();
    Some(Reading::Maybe(found))
}

/// The journal prose names: "… reported in The Lancet (2020) — …", its
/// capitalised words between a verb's "in" and the bracketed year.
fn prose_journal(tokens: &[&str]) -> Option<String> {
    tokens.iter().enumerate().find_map(|(i, t)| {
        let verb = i > 0
            && tokens[i - 1].len() > 2
            && tokens[i - 1].chars().all(|c| c.is_ascii_lowercase());
        if *t != "in" || !verb {
            return None;
        }
        let rest = &tokens[i + 1..];
        let end = rest.iter().position(|w| !name_word(w))?;
        let year = rest[end]
            .strip_prefix('(')
            .is_some_and(|y| bracketed_year(y.trim_end_matches(['.', ','])));
        // "… the 2007 Bar-Haim paper in Psychol Sci (Threat-related …)":
        // a paper's journal, before what is bracketed after it.
        let paper = matches!(tokens[i - 1], "paper" | "article" | "study" | "report")
            && rest[end].starts_with('(');
        // "… showed in Cancer Epidemiology, 38, 211–214, in …": a journal,
        // its last word's comma, then its volume and pages.
        let placed = rest[end]
            .strip_suffix(',')
            .is_some_and(|w| !w.is_empty() && name_word(w))
            && rest
                .get(end + 1)
                .and_then(|v| v.strip_suffix(','))
                .is_some_and(|v| volume_number(v).is_some())
            && rest.get(end + 2).is_some_and(|p| {
                ranged(p) && page_number(p.trim_end_matches([',', '.'])).is_some()
            });
        if placed {
            let mut words = rest[..end].to_vec();
            words.push(rest[end].trim_end_matches(','));
            return (words.len() <= JOURNAL_WORDS && !is_stopword(words[0]) && words[0] != "&")
                .then(|| words.join(" "));
        }
        let words = &rest[..end];
        ((year || paper)
            && !words.is_empty()
            && words.len() <= JOURNAL_WORDS
            && !is_stopword(words[0])
            && words[0] != "&")
            .then(|| words.join(" "))
    })
}

/// What a journal read from a reference must follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum After {
    /// Anything, or nothing: before a styled locator, a running header
    /// ("Injury 47 (2016) 797–804") is all journal.
    Anything,
    /// The title, ended by a stop or a closing quote.
    Title,
    /// The title, the journal itself ended by a bracketed year (Chicago's
    /// "“…” Journal (2010)", "… Druck. `ApJ` (1939)"), not a stop.
    Dated,
    /// A quoted title (Harvard's "'…', Chest.").
    Quoted,
}

/// Where the journal ends in a reference without a styled locator, and
/// what it must follow ([`cited_journal`]).
fn unstyled_end(tokens: &[&str]) -> Option<(usize, After)> {
    // Chicago: "“Title.” Journal 94, no. 2 (2004).", "… Journal (2010)."
    if let Some((at, _)) = dated_end(tokens) {
        return Some((at, After::Dated));
    }
    // Labelled: "Journal of Clinical Investigation, vol. 8, p. 109"; a
    // label before a number ("J. P. (1982)" has an initial, not a page).
    let label = |i: usize| {
        is_label(tokens[i], &["vol", "volume", "no", "pp", "p"])
            && tokens
                .get(i + 1)
                .is_some_and(|n| n.starts_with(|c: char| c.is_ascii_digit()))
    };
    if let Some(i) = (0..tokens.len()).find(|&i| label(i)) {
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

/// A stop and the comma after it, as IEEE ends a dotted journal ("Mach.
/// Intell., vol. 56"): the stop only.
fn without_comma_after_stop(t: &str) -> &str {
    t.strip_suffix(',')
        .filter(|w| w.ends_with('.'))
        .unwrap_or(t)
}

/// A link or identifier ending a reference: a DOI, URL, PMID or arXiv ID.
pub(super) fn is_link(t: &str) -> bool {
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
    let last = without_comma_after_stop(tokens[end - 1]);
    let mut k = end - 1;
    let mut words = vec![if dotted {
        last
    } else {
        last.strip_suffix(['.', ',']).unwrap_or(last)
    }];
    // What ended the name: a closing quote (true) or a stop (false).
    let mut ended = None;
    // Words with a comma, as a subtitle after a colon lists them ("Journal
    // of Experimental Psychology: Learning, Memory, and Cognition"): kept
    // only if the colon comes.
    let mut listed = false;
    // A list's "and" ("…, Memory, and Cognition"), read already.
    let mut joined = false;
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
        } else if name_word(t) || colon_name(t) {
            if colon_name(t) {
                listed = false;
            }
            joined |= t == "and" || t == "&";
            words.push(t);
        } else if let Some(w) = t
            .strip_suffix(',')
            .filter(|w| joined && ((name_word(w) && capitalised(w)) || colon_name(w)))
        {
            // The colon compacted into the word after it: "Psychology:Learning,".
            listed = !colon_name(w);
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
    if listed {
        return None;
    }
    let follows = match after {
        After::Anything => true,
        After::Title | After::Dated => ended.is_some(),
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
    Some(spaced_colons(&words.join(" ")))
}

/// Name words around a colon inside a name, the space after it dropped
/// ([`compact`]): "JAMA:The" in "JAMA: The Journal of the American Medical
/// Association".
fn colon_name(t: &str) -> bool {
    t.contains(':')
        && t.split(':').all(|w| w.is_empty() || name_word(w))
        && t.split(':').any(|w| !w.is_empty())
}

/// A name with its colons spaced as written: "JAMA: The Journal …".
fn spaced_colons(name: &str) -> String {
    name.split(':')
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(": ")
}

/// A word of a journal's name as references print it: letters (and
/// hyphens, apostrophes), capitalised or with a capital inside ("eLife"),
/// or a joining word ("of", "&").
pub(super) fn name_word(w: &str) -> bool {
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

/// A journal name's joining word, accents aside ("für").
fn is_stopword(w: &str) -> bool {
    JOURNAL_STOPWORDS.contains(&w)
        || (w.starts_with(char::is_lowercase) && JOURNAL_STOPWORDS.contains(&normalise(w).as_str()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publication::{Locator, locator};

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
            // A subtitle after a colon, its words listed with commas
            // (holdout-5 #784).
            (
                "Kulkarni, M., & Hannula, D. E. (2024). Control of Memory Retrieval Alters \
                 Memory-Based Eye Movements. Journal of Experimental Psychology: Learning, \
                 Memory, and Cognition, 50(8), 1199–1219.",
                "Journal of Experimental Psychology: Learning, Memory, and Cognition",
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
            // Prose: a verb's "in", the journal, the bracketed year.
            (
                "Huang et al. described in The Lancet (2020) — \"Clinical features of patients\".",
                "The Lancet",
            ),
            (
                "Wiers et al. described in Genes Brain & Behavior (2008) — \"Relatively strong\".",
                "Genes Brain & Behavior",
            ),
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
    fn journals_and_places_are_read_in_more_styles() {
        // Red team, held-out round (holdout-1): Nature's single page after
        // a dotted name, IEEE's comma after one, Chicago's issue and
        // bracketed year, a colon inside a name, an initial "P." that
        // isn't a page.
        let loc = |v: &str, p: &str| Locator {
            volume: Some(v.into()),
            page: Some(p.into()),
            pages: None,
        };
        for (text, journal, place) in [
            (
                "Mondain-Monval, O. et al. Anisotropic Rayleigh scattering investigations in \
                 smectic A liquid crystal phases. J. Org. Chem. 101, 6301 (1994).",
                "J. Org. Chem.",
                Some(loc("101", "6301")),
            ),
            (
                "R. Khandekar, S. Rao, and U. Vazirani, “Graph partitioning using single \
                 commodity flows,” IEEE Trans. Pattern Anal. Mach. Intell., vol. 56, no. 4, pp. \
                 1-15, 2009.",
                "IEEE Trans. Pattern Anal. Mach. Intell.",
                Some(loc("56", "1")),
            ),
            (
                "V. V. Kiselev, “Genesis of spiral galaxies,” Mon. Not. R. Astron. Soc., vol. \
                 428, no. 5, pp. 674-684, 2013.",
                "Mon. Not. R. Astron. Soc.",
                Some(loc("428", "674")),
            ),
            (
                "Antonovics, Kate, and Robert Town. “Are All the Good Men Married? Uncovering \
                 the Sources of the Marital Wage Premium.” American Economic Review 94, no. 2 \
                 (2004): 317-321.",
                "American Economic Review",
                Some(loc("94", "317")),
            ),
            (
                "v. Koczkás, Julius. “die ultraviolette absorption.” Zeitschrift für Physik 59, \
                 no. 3-4 (1930): 274-288.",
                "Zeitschrift für Physik",
                Some(loc("59", "274")),
            ),
            (
                "Alonso-González, Rafael. “Ventrículo derecho y cardiopatías congénitas en el \
                 adulto.” eClinicalMedicine (2010).",
                "eClinicalMedicine",
                None,
            ),
            (
                "Park, Robin. “Sex-bias in COVID-19-associated illness severity.” American \
                 Journal of Epidemiology (2020). https://doi.org/10.1016/j.eclinm.2020.100519",
                "American Journal of Epidemiology",
                None,
            ),
            // After a quoted title, the journal as written, in sentence case
            // too.
            (
                "Berry, Christopher J. “Adam Smith's Considerations on Language.” Revue \
                 historique 35, no. 1 (1974): 130.",
                "Revue historique",
                Some(loc("35", "130")),
            ),
            (
                "Doe, J. 'Ventrículo derecho', Revista española de cardiología, 63(9), pp. \
                 1070-1080.",
                "Revista española de cardiología",
                Some(loc("63", "1070")),
            ),
            (
                "Ramachandran, V. S. “Perception of Shape from Shading.” Psychological Science 5 \
                 (1994).",
                "Psychological Science",
                Some(Locator {
                    volume: Some("5".into()),
                    ..Locator::default()
                }),
            ),
            (
                "Montgomery, James D. “A Formalization.” American Sociological Review 68, no. 5 \
                 (2003).",
                "American Sociological Review",
                Some(Locator {
                    volume: Some("68".into()),
                    ..Locator::default()
                }),
            ),
            (
                "Füchtbauer & von Heesen, W. Verbreiterung und Verschiebung von Kaliumserienlinien \
                 durch Helium von hohem Druck. ApJ (1939). https://doi.org/10.1007/bf01340112",
                "ApJ",
                None,
            ),
            (
                "Watson ZT, Ru W, et al. Differential selectivity of microglia and astrocytes. \
                 JAMA: The Journal of the American Medical Association. 2026. \
                 doi:10.1093/brain/awaf429",
                "JAMA: The Journal of the American Medical Association",
                None,
            ),
            (
                "Boucher, R. E., & Noonan, J. P. (1982). Adaptive Detection and Removal of \
                 Non-Gaussian Spikes from Gaussian Data. Proceedings of the AAAI Symposium \
                 Series. https://doi.org/10.1109/tpami.1982.4767218",
                "Proceedings of the AAAI Symposium Series",
                None,
            ),
        ] {
            assert_eq!(cited_journal(text).as_deref(), Some(journal), "{text}");
            if let Some(place) = place {
                let found = Locator {
                    pages: None,
                    ..locator(text)
                };
                assert_eq!(found, place, "{text}");
            }
        }
        // Bare numbers and years elsewhere are still no place.
        for text in [
            "Smith J. Title. Publisher, 2010.",
            "Smith J. (2010). Some Title. Oxford University Press.",
            "We saw 12 (2010) patients.",
        ] {
            assert_eq!(locator(text), Locator::default(), "{text}");
        }
    }

    #[test]
    fn a_dotted_journal_after_a_title_in_title_case_is_one_of_its_readings() {
        // Held-out round: the title's last word and the journal's first
        // both end with a stop, so either may begin the journal.
        for (text, journal) in [
            (
                "Perera C. After the Tsunami: Legal Implications of Mass Burials of Unidentified \
                 Victims in Sri Lanka. Cad. Saúde Pública. 2005;2(6):e185.",
                "Cad. Saúde Pública",
            ),
            (
                "D'Aniell, F. 1,3-Stereocontrol with Bromoallenes. Synthesis of N-Boc-ADDA in \
                 Serine/Threonine Phosphatases. Chem. Rev. (1996).",
                "Chem. Rev.",
            ),
            (
                "Rauch, J. Big Signals from Small Particles: Regulation of Cell Signaling \
                 Pathways by Nanoparticles. J. Am. Chem. Soc. 113, 3391 (2013).",
                "J. Am. Chem. Soc.",
            ),
            (
                "Licht, A. L. Gravitational Waves on Conductors. Class. Quantum Grav. 22, 505 \
                 (2005).",
                "Class. Quantum Grav.",
            ),
            (
                "Doe J. Water in New York City. J. Am. Chem. Soc. 2001;123:45-50.",
                "J. Am. Chem. Soc.",
            ),
        ] {
            assert_eq!(cited_journal(text), None, "{text}");
            let readings = cited_journals(text);
            assert!(
                readings
                    .iter()
                    .any(|r| r.trim_end_matches('.') == journal.trim_end_matches('.')),
                "{text}: {readings:?}"
            );
        }
        // A sure reading is the only one.
        assert_eq!(
            cited_journals("Smith J. Is it safe? eLife. 2020;9:e1234."),
            ["eLife"]
        );
        assert!(
            cited_journals("Smith J. Title. Journal of clinical medicine. 2020;9:12").is_empty()
        );
    }

    #[test]
    fn round_three_styles_give_their_locators_and_journals() {
        // holdout-2: MLA, Harvard's page label run into its page, IEEE's
        // article number.
        for (text, volume, page) in [
            (
                "Bergeret, F. S., et al. \"Odd triplet superconductivity.\" Physical Review B \
                 77.4 (2005): 1321-1373.",
                "77",
                "1321",
            ),
            (
                "Barro, R.J., 2008. Consumption Disasters. American Economic Review, 100(2), \
                 pp.58-63.",
                "100",
                "58",
            ),
            (
                "Maier-Flaig, H., 2016. Spin pumping. Nature Physics, 94(5), p.054433.",
                "94",
                "054433",
            ),
            (
                "T. M. Jones, \"Instrumental Stakeholder Theory,\" Acad Management Rev, vol. 20, \
                 no. 2, Art. no. 615, 1995.",
                "20",
                "615",
            ),
        ] {
            let found = locator(text);
            assert_eq!(found.volume.as_deref(), Some(volume), "{text}");
            assert_eq!(found.page.as_deref(), Some(page), "{text}");
        }
        assert_eq!(
            cited_journal(
                "Bergeret, F. S., et al. \"Odd triplet superconductivity.\" Physical Review B \
                 77.4 (2005): 1321-1373."
            )
            .as_deref(),
            Some("Physical Review B")
        );
        assert_eq!(
            cited_journal("the 2007 Bar-Haim paper in Psychol Sci (Threat-related bias.)")
                .as_deref(),
            Some("Psychol Sci")
        );
        // A short abbreviation after another isn't read alone; each reading
        // is weighed.
        let nonaka = "Nonaka, I. A Dynamic Theory of Organizational Knowledge Creation. \
                      Psychol. Sci. 5, 14–37 (1994).";
        assert_eq!(cited_journal(nonaka), None);
        assert!(cited_journals(nonaka).contains(&"Psychol. Sci.".to_owned()));
        assert_eq!(
            cited_journal("Huang C, et al. Clinical features in Wuhan, China. JAMA. 2020.")
                .as_deref(),
            Some("JAMA")
        );
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
            "Smith J. Growth in Cell Cultures (2020 edition).",
            "Ji J, Chen W. Mitigating Deceptive Alignment via Self-Monitoring. 2025. arXiv:2505.18807",
            // Words listed with commas, after no colon: no name's.
            "Smith J. Title. Learning, Memory, and Cognition, 50(8), 1199–1219.",
            // Nor a publisher after its place (wild-4 #841).
            "———. Essence of Decision: Explaining the Cuban Missile Crisis. Boston: Little, Brown, 1971.",
        ] {
            assert_eq!(cited_journal(text), None, "{text}");
        }
    }

    #[test]
    fn a_journal_without_its_stop_or_after_a_quoted_title_is_one_reading() {
        // No stop before the closing year (wild-5 #375); after a quoted
        // title, before a note (#560): one reading, not a sure one.
        for (text, journal) in [
            (
                "Rahman MM, Pallikadavath S. How much do transfers help? New evidence from India. Popul Space Place 2018.",
                "Popul Space Place",
            ),
            (
                "Johnston, Farmer (2016), \"Bridging the building fabric performance gap,\" BSERT, a follow-up on remedies.",
                "BSERT",
            ),
        ] {
            assert_eq!(cited_journals(text), vec![journal.to_owned()], "{text}");
            assert_eq!(cited_journal(text), None, "{text}");
        }
        // Not a publisher, a book's "in", or a name before more names.
        for text in [
            "Smith J. Some title. Oxford University Press 2018.",
            "Smith J (2010), \"A chapter,\" in Handbook of Things, edited by K Jones.",
            "Smith J (2010), \"A chapter,\" Harvard University Press, a classic.",
            "Smith J (2010), \"A title,\" Nature, 2010.",
            "Ren S. Faster R-CNN: towards real-time object detection. NeurIPS 2015.",
            "A. Iovita, \"Derivatives of p-adic L-functions,\" Invent. math., vol. 154, 2003.",
            "Cameron A. \"The Language of Images.\" In The Church and the Arts, edited by D Wood.",
        ] {
            assert!(
                cited_journals(text)
                    .iter()
                    .all(|j| j != "Oxford University Press"
                        && !j.starts_with("In ")
                        && j != "Invent"
                        && j != "NeurIPS"
                        && j != "Harvard University Press"),
                "{text}: {:?}",
                cited_journals(text)
            );
        }
    }
}
