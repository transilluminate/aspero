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
/// The article "A" inside a name ("CA: A Cancer Journal for Clinicians",
/// abbreviated "CA Cancer J Clin") is left out too; a series letter
/// ("Physical Review A", "Acta Crystallographica Section A: …") isn't.
pub(crate) fn significant(name: &str) -> Vec<String> {
    let norm = normalise(name);
    let words: Vec<&str> = norm.split(' ').filter(|w| !w.is_empty()).collect();
    let article = |i: usize| {
        words[i] == "a"
            && i + 1 < words.len()
            && !(i > 0 && STRUCTURAL_WORDS.contains(&words[i - 1]))
    };
    words
        .iter()
        .enumerate()
        .filter(|&(i, w)| !JOURNAL_STOPWORDS.contains(w) && !article(i))
        .map(|(_, w)| (*w).to_owned())
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
fn styled(tokens: &[&str]) -> Option<(usize, Locator)> {
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

/// Conferences whose papers are cited by venue, each by its acronyms and
/// the words of its name that tell it apart (joining words and numbers
/// aside: "Proceedings of the 34th International Conference on Machine
/// Learning"). A proceedings' name is otherwise free ("2017 IEEE
/// Conference on Computer Vision and Pattern Recognition (CVPR)",
/// "Advances in Neural Information Processing Systems 30"), so these are
/// named rather than ruled.
const CONFERENCES: &[(&[&str], &[&str])] = &[
    (
        &["neurips", "nips"],
        &["neural", "information", "processing", "systems"],
    ),
    (
        &["icml"],
        &["international", "conference", "machine", "learning"],
    ),
    (&["iclr"], &["learning", "representations"]),
    (&["cvpr"], &["computer", "vision", "pattern", "recognition"]),
    (
        &["iccv"],
        &["international", "conference", "computer", "vision"],
    ),
    (&["eccv"], &["european", "conference", "computer", "vision"]),
    (
        &["wacv"],
        &["winter", "conference", "applications", "computer", "vision"],
    ),
    (&["bmvc"], &["british", "machine", "vision"]),
    (
        &["acl"],
        &[
            "annual",
            "meeting",
            "association",
            "computational",
            "linguistics",
        ],
    ),
    (
        &["emnlp"],
        &["empirical", "methods", "natural", "language", "processing"],
    ),
    (&["naacl"], &["north", "american", "chapter"]),
    (
        &["coling"],
        &[
            "international",
            "conference",
            "computational",
            "linguistics",
        ],
    ),
    (
        &["kdd", "sigkdd"],
        &["knowledge", "discovery", "data", "mining"],
    ),
    (
        &["aaai"],
        &["aaai", "conference", "artificial", "intelligence"],
    ),
    (
        &["ijcai"],
        &[
            "international",
            "joint",
            "conference",
            "artificial",
            "intelligence",
        ],
    ),
    (&["aistats"], &["artificial", "intelligence", "statistics"]),
    (&["uai"], &["uncertainty", "artificial", "intelligence"]),
    (&["colt"], &["conference", "learning", "theory"]),
    (&["chi"], &["human", "factors", "computing", "systems"]),
    (
        &["icra"],
        &["international", "conference", "robotics", "automation"],
    ),
    (&["iros"], &["intelligent", "robots", "systems"]),
    (&["corl"], &["conference", "robot", "learning"]),
    (
        &["isca"],
        &["international", "symposium", "computer", "architecture"],
    ),
    (
        &["micro"],
        &["international", "symposium", "microarchitecture"],
    ),
    (
        &["sigir"],
        &["research", "development", "information", "retrieval"],
    ),
    (
        &["sigmod"],
        &["international", "conference", "management", "data"],
    ),
    (&["vldb"], &["very", "large", "data", "bases"]),
    (
        &["icse"],
        &["international", "conference", "software", "engineering"],
    ),
    (
        &["osdi"],
        &["operating", "systems", "design", "implementation"],
    ),
    (
        &["sosp"],
        &["symposium", "operating", "systems", "principles"],
    ),
    (
        &["nsdi"],
        &["networked", "systems", "design", "implementation"],
    ),
    (
        &["pldi"],
        &["programming", "language", "design", "implementation"],
    ),
    (&["popl"], &["principles", "programming", "languages"]),
    (
        &["icassp"],
        &["acoustics", "speech", "signal", "processing"],
    ),
    (&["interspeech"], &["interspeech"]),
    (
        &["miccai"],
        &[
            "medical",
            "image",
            "computing",
            "computer",
            "assisted",
            "intervention",
        ],
    ),
    (&["wsdm"], &["web", "search", "data", "mining"]),
    (&["cikm"], &["information", "knowledge", "management"]),
    (&["recsys"], &["recommender", "systems"]),
];

/// Acronyms that are words or names too ("ACL" a ligament, "Chi" a
/// family name): read only in capitals.
const WORDLIKE_ACRONYMS: &[&str] = &["acl", "chi", "micro", "colt", "uai", "isca"];

/// The conferences (indices into [`CONFERENCES`]) a venue's name, or the
/// text that cites one, names: by acronym ("ICCV", "NeurIPS 2017",
/// "CVPR'17") or by its name's words in order.
pub(crate) fn conferences(name: &str) -> Vec<usize> {
    let tokens: Vec<&str> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();
    let acronym = |a: &str| {
        tokens.iter().any(|t| {
            let t = t.trim_end_matches(|c: char| c.is_ascii_digit());
            if WORDLIKE_ACRONYMS.contains(&a) {
                t.eq_ignore_ascii_case(a) && t.chars().all(|c| c.is_ascii_uppercase())
            } else {
                t.eq_ignore_ascii_case(a)
            }
        })
    };
    let words: Vec<String> = normalise(name)
        .split(' ')
        .filter(|w| !w.is_empty() && !JOURNAL_STOPWORDS.contains(w))
        .filter(|w| !w.chars().any(|c| c.is_ascii_digit()))
        .map(str::to_owned)
        .collect();
    let named = |phrase: &[&str]| words.windows(phrase.len()).any(|run| run == phrase);
    CONFERENCES
        .iter()
        .enumerate()
        .filter(|(_, (acronyms, phrase))| acronyms.iter().any(|a| acronym(a)) || named(phrase))
        .map(|(i, _)| i)
        .collect()
}

/// Whether a venue cited is a conference's or a workshop's proceedings:
/// "In Proceedings of …", "… Conference on …", "Advances in Neural …".
pub(crate) fn proceedings(venue: &str) -> bool {
    !conferences(venue).is_empty()
        || normalise(venue).split(' ').any(|w| {
            matches!(
                w,
                "proceedings" | "proc" | "conference" | "symposium" | "workshop" | "congress"
            )
        })
}

/// Words of a proceedings' name that say what kind of meeting, who holds
/// it and when, not what it is about: "Proceedings of the 16th Annual
/// ACM/IEEE International Conference on …", "Companion of the …", and the
/// furniture a reference puts after it ("pp.", "eds.").
const PROCEEDINGS_BOILERPLATE: &[&str] = &[
    "proceedings",
    "proceeding",
    "proc",
    "procs",
    "companion",
    "conference",
    "conferences",
    "conf",
    "symposium",
    "symposia",
    "symp",
    "workshop",
    "workshops",
    "wksp",
    "congress",
    "meeting",
    "colloquium",
    "summit",
    "forum",
    "annual",
    "annu",
    "international",
    "int",
    "intl",
    "world",
    "joint",
    "acm",
    "ieee",
    "cvf",
    "rsj",
    "ifip",
    "siam",
    "spie",
    "usenix",
    "edition",
    "ed",
    "eds",
    "editor",
    "editors",
    "vol",
    "volume",
    "pp",
    "page",
    "pages",
    "part",
    "extended",
    "abstracts",
    "adjunct",
    "papers",
    "poster",
    "posters",
    "demo",
    "demos",
    "track",
    "session",
    "series",
    "held",
    "th",
    "st",
    "nd",
    "rd",
    "a",
    "an",
    "at",
    "to",
    "with",
];

/// Ordinals spelt out, as editions are: "Tenth Annual …".
const ORDINALS: &[&str] = &[
    "first",
    "second",
    "third",
    "fourth",
    "fifth",
    "sixth",
    "seventh",
    "eighth",
    "ninth",
    "tenth",
    "eleventh",
    "twelfth",
    "thirteenth",
    "fourteenth",
    "fifteenth",
    "sixteenth",
    "seventeenth",
    "eighteenth",
    "nineteenth",
    "twentieth",
    "thirtieth",
    "fortieth",
    "fiftieth",
    "twenty",
    "thirty",
    "forty",
    "fifty",
    "sixty",
];

/// Capitalised tokens that name a publisher, place or numeral, not a
/// conference.
const NOT_ACRONYMS: &[&str] = &[
    "acm", "ieee", "cvf", "rsj", "ifip", "siam", "spie", "usenix", "usa", "ny", "uk", "lncs", "pp",
    "ii", "iii", "iv", "vi", "vii", "viii", "ix", "xi", "xii", "eds",
];

/// A proceedings' name as words of its subject and acronyms: "Proceedings
/// of the Tenth Annual ACM/IEEE International Conference on Human-Robot
/// Interaction (HRI '15)" is about "human robot interaction", "HRI". A
/// special interest group (SIGCHI, SIGACCESS) holds a meeting; its name is
/// neither.
struct Subject {
    words: Vec<String>,
    acronyms: Vec<String>,
}

impl Subject {
    fn new(name: &str) -> Subject {
        let tokens: Vec<&str> = name
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| !t.is_empty())
            .collect();
        let alphabetic: Vec<&&str> = tokens
            .iter()
            .filter(|t| t.chars().all(char::is_alphabetic) && t.chars().count() > 3)
            .collect();
        // A name in capitals throughout has words, not acronyms.
        let shouting = alphabetic.len() >= 3
            && alphabetic
                .iter()
                .filter(|t| t.chars().all(char::is_uppercase))
                .count()
                * 2
                > alphabetic.len();
        let mut acronyms: Vec<String> = Vec::new();
        if !shouting {
            for t in &tokens {
                let t = t.trim_end_matches(|c: char| c.is_ascii_digit());
                let upper = t.chars().filter(char::is_ascii_uppercase).count();
                let lower = t.to_ascii_lowercase();
                if upper >= 2
                    && (2..=12).contains(&t.len())
                    && t.chars().all(|c| c.is_ascii_alphabetic())
                    && !NOT_ACRONYMS.contains(&lower.as_str())
                    && !lower.starts_with("sig")
                    && !acronyms.contains(&lower)
                {
                    acronyms.push(lower);
                }
            }
        }
        let words = normalise(name)
            .split(' ')
            .filter(|w| w.chars().count() > 1)
            .filter(|w| !w.chars().any(|c| c.is_ascii_digit()))
            .filter(|w| {
                !JOURNAL_STOPWORDS.contains(w)
                    && !PROCEEDINGS_BOILERPLATE.contains(w)
                    && !ORDINALS.contains(w)
                    && !w.starts_with("sig")
                    && !acronyms.iter().any(|a| a == w)
            })
            .map(str::to_owned)
            .fold(Vec::new(), |mut words, w| {
                if !words.contains(&w) {
                    words.push(w);
                }
                words
            });
        Subject { words, acronyms }
    }
}

/// One subject word for another, or for its abbreviation or plural:
/// "Robot." for "Robotics", "Computers" for "Computer".
fn same_subject_word(a: &str, b: &str) -> bool {
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    short == long || (short.len() >= 3 && long.starts_with(short))
}

/// Whether a proceedings cited is the record's, by what each is about:
/// `Some(true)` when they share an acronym or most of the record's
/// subject, `Some(false)` when they're clearly about different things
/// ("… Workshop on User Interface Test Automation" for "… World Conference
/// on Mobile and Contextual Learning", or "`RecSys '26`" alone for a name
/// whose own acronym is "IUI"), `None` when it can't be told (an acronym
/// against a name without one: "ASSETS" for "… Conference on Computers and
/// Accessibility"). `cited` may run on into what a reference puts after
/// the venue (pages, a publisher): words there only make a mismatch less
/// likely.
pub(crate) fn proceedings_agree(cited: &str, actual: &str) -> Option<bool> {
    let (cited, actual) = (Subject::new(cited), Subject::new(actual));
    let same_acronym =
        |a: &String, b: &String| a == b || a.ends_with(b.as_str()) || b.ends_with(a.as_str());
    if cited
        .acronyms
        .iter()
        .any(|c| actual.acronyms.iter().any(|a| same_acronym(c, a)))
    {
        return Some(true);
    }
    if cited.words.is_empty() {
        return (!cited.acronyms.is_empty() && !actual.acronyms.is_empty()).then_some(false);
    }
    if actual.words.is_empty() {
        return None;
    }
    let shared = actual
        .words
        .iter()
        .filter(|a| cited.words.iter().any(|c| same_subject_word(c, a)))
        .count();
    if shared * 2 >= actual.words.len() {
        Some(true)
    } else if shared * 3 <= actual.words.len() {
        Some(false)
    } else {
        None
    }
}

/// Whether a venue's name is a meeting's, as a record's container: "…
/// Conference on …", "Companion of …", and with `proceedings` "Proceedings
/// of …" too (journals are called that: the Royal Society's).
pub(crate) fn meeting_name(venue: &str, proceedings: bool) -> bool {
    normalise(venue).split(' ').any(|w| {
        matches!(
            w,
            "conference"
                | "symposium"
                | "workshop"
                | "workshops"
                | "congress"
                | "companion"
                | "colloquium"
        ) || (proceedings && w == "proceedings")
    })
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
    if let Some(journal) = prose_journal(&tokens) {
        return Some(Reading::Sure(journal));
    }
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
    if let Some(journal) = quoted_journal(&tokens, end) {
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
    let undotted = read_journal(&tokens, end, false, after).filter(|j| {
        let short = !j.contains(' ')
            && last.ends_with('.')
            && j.chars().count() <= 4
            && j.chars().any(char::is_lowercase);
        !(dotted && short)
    });
    dotted
        .then(|| read_journal(&tokens, end, true, after))
        .flatten()
        .or(undotted)
        .map(Reading::Sure)
        .or_else(|| readings(&tokens, end, after))
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

/// "pp." or "p.", before a page.
fn is_page_label(t: &str) -> bool {
    is_label(t, &["pp", "p"])
}

/// A label, one of `labels`, as written: "no.", "Vol.", "pp.,".
fn is_label(t: &str, labels: &[&str]) -> bool {
    let t = t.trim_end_matches(['.', ',']);
    labels.iter().any(|l| l.eq_ignore_ascii_case(t))
}

/// Chicago's end, a bracketed year after the journal (and its volume and
/// issue), its links aside: "Journal 94, no. 2 (2004).", "Mind 133
/// (2024).", "eClinicalMedicine (2010).". Where the journal ends, and the
/// volume given.
fn dated_end(tokens: &[&str]) -> Option<(usize, Option<String>)> {
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

/// A stop and the comma after it, as IEEE ends a dotted journal ("Mach.
/// Intell., vol. 56"): the stop only.
fn without_comma_after_stop(t: &str) -> &str {
    t.strip_suffix(',')
        .filter(|w| w.ends_with('.'))
        .unwrap_or(t)
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
    let last = without_comma_after_stop(tokens[end - 1]);
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
        } else if name_word(t) || colon_name(t) {
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

/// MLA's volume and issue before the bracketed year: "77.4(2005)" → "77".
fn mla_volume(s: &str) -> Option<String> {
    let (numbers, year) = s.split_once('(')?;
    let (volume, issue) = numbers.split_once('.')?;
    (bracketed_year(year) && is_digits(issue) && issue.len() <= 6)
        .then(|| volume_number(volume))
        .flatten()
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
            // The article "A" inside a name (app pre-test, #102).
            ("CA Cancer J Clin", "CA: A Cancer Journal for Clinicians"),
            ("CA Cancer J Clin", "CA A Cancer Journal for Clinicians"),
            ("Phys Rev A", "Physical Review A"),
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
                    page: None,
                }),
            ),
            (
                "Montgomery, James D. “A Formalization.” American Sociological Review 68, no. 5 \
                 (2003).",
                "American Sociological Review",
                Some(Locator {
                    volume: Some("68".into()),
                    page: None,
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
                assert_eq!(locator(text), place, "{text}");
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
    fn conferences_are_told_by_acronym_or_name() {
        let named = |venue: &str| -> Vec<&str> {
            conferences(venue)
                .into_iter()
                .map(|i| CONFERENCES[i].0[0])
                .collect()
        };
        assert_eq!(
            named("2017 IEEE Conference on Computer Vision and Pattern Recognition (CVPR)"),
            ["cvpr"]
        );
        assert_eq!(
            named("In IEEE International Conference on Computer Vision (ICCV), 5967–5976."),
            ["iccv"]
        );
        assert_eq!(
            named("Advances in Neural Information Processing Systems 30"),
            ["neurips"]
        );
        assert_eq!(named("NeurIPS'17"), ["neurips"]);
        assert_eq!(
            named(
                "Proceedings of the 20th ACM SIGKDD international conference on Knowledge \
                 discovery and data mining"
            ),
            ["kdd"]
        );
        assert_eq!(
            named("Proceedings of the 2019 CHI Conference on Human Factors in Computing Systems"),
            ["chi"]
        );
        // Words that are acronyms too, not in capitals, name none.
        assert!(named("Chi H, Acl reconstruction outcomes. Am J Sports Med.").is_empty());
        assert!(named("Pattern Recognition").is_empty());
        assert!(proceedings("In: Proceedings of the 5th Workshop on Things"));
        assert!(!proceedings("Journal of Machine Learning Research"));
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
            // A series letter is not an article.
            ("Phys Rev A", "Physical Review B"),
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
