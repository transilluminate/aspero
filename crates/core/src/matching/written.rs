//! What a free-text reference writes of its title and authors, as
//! written, for [`Claim::as_read`](super::Claim::as_read): where a work's
//! title stands in it, the title there and the byline before it; else what
//! its layout says (Vancouver's "Authors. Title. Journal. Year;…", APA's
//! "Authors (Year). Title. Journal, …", Harvard's, a quoted title). Nothing
//! where it can't be told: a wrong title is worse than none.

use std::ops::Range;

use super::claim::byline;
use super::text::{Normalised, normalise};
use super::titles::given_title;
use crate::publication;
use crate::work::Work;

/// A reference's title and authors, as written.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Written {
    pub(super) title: Option<String>,
    pub(super) authors: Vec<String>,
}

/// What `text` writes of its title and authors: a BibTeX entry's fields;
/// else where `work`'s title stands in it, that stretch and the names
/// before it; else its layout's ([`laid_out`]).
pub(super) fn written(text: &str, work: Option<&Work>) -> Written {
    if text.trim_start().starts_with('@') {
        let field = |name: &str| publication::bibtex_field(text, name);
        return Written {
            title: field("title").filter(|t| !t.trim().is_empty()),
            authors: field("author").map_or_else(Vec::new, |a| {
                a.split(" and ")
                    .map(str::trim)
                    .filter(|a| !a.is_empty())
                    .map(str::to_owned)
                    .collect()
            }),
        };
    }
    let found = work
        .and_then(|w| w.title.as_deref())
        .filter(|t| !t.starts_with("http"))
        .and_then(|title| {
            let text = Normalised::new(text);
            let span = text.title_span(title).filter(|s| !s.is_empty())?;
            Some((text, title, span))
        });
    let laid = laid_out(text);
    let Some((normalised, title, span)) = found else {
        return laid;
    };
    let stretch = stretch(&normalised, &span);
    let authors = byline(&normalised, Some(title), Some(&span))
        .and_then(|b| names_before_title(&b))
        .unwrap_or_default();
    let (cited, read) = (normalise(&stretch), laid.title.as_deref().map(normalise));
    let title = match read {
        // The layout's title cut short of the work's, at a stop inside it
        // ("N-Acylenamines from oxazolines. New route to …").
        Some(read) if cited.starts_with(&read) => Some(stretch),
        // The work's title inside the layout's: cut short of the one cited
        // (a subtitle the record lacks), or words of the title that are
        // the work's own ("Data from: …"). Elsewhere: another work's title
        // standing where the cited one's journal does ("JAMA Network
        // Open"). The layout's either way.
        Some(_) => laid.title,
        // No layout: the work's title, unless it is the journal cited.
        None => {
            let journal = publication::cited_journal(text).map(|j| normalise(&j));
            (journal.is_none_or(|j| !j.contains(&cited))).then_some(stretch)
        }
    };
    Written {
        title,
        authors: if authors.is_empty() {
            laid.authors
        } else {
            authors
        },
    }
}

/// The words at `span`, as written, with the mark ending a question and
/// the bracket closing one opened inside.
fn stretch(text: &Normalised, span: &Range<usize>) -> String {
    let (from, mut to) = (text.spans[span.start].start, text.spans[span.end - 1].end);
    let opened = |s: &str| s.matches('(').count() > s.matches(')').count();
    loop {
        let next = text.raw[to..].chars().next();
        match next {
            Some(c @ ('?' | '!')) => to += c.len_utf8(),
            Some(')') if opened(&text.raw[from..to]) => to += 1,
            _ => break,
        }
    }
    text.raw[from..to].to_owned()
}

/// What a reference's layout says of its title and authors: a quoted
/// title (Harvard's 'An image is worth …', IEEE's "…,") right after the
/// names; else, after a byline that reads as names throughout (and the
/// year after it, APA's and Harvard's), the sentence that follows, when a
/// journal, year or place follows that. Nothing otherwise. Emphasis
/// marks (`*…*`) aren't the title's.
pub(super) fn laid_out(text: &str) -> Written {
    let plain = text.replace(['*', '_'], "");
    let text = unnumbered(plain.trim());
    if let Some(quoted) = quoted(text) {
        return quoted;
    }
    // The byline ends at the last of its stops that still reads as names
    // ("Seymour, C. W., Liu, V. X. (2016).", stopping after each initial).
    let mut byline: Option<(usize, Vec<String>)> = None;
    for (at, _) in text.match_indices(' ') {
        let before = &text[..at];
        if !before.ends_with(['.', ')']) {
            continue;
        }
        match names(before) {
            Some(found) => byline = Some((at, found)),
            None if byline.is_some() => break,
            None => {}
        }
    }
    let Some((at, authors)) = byline else {
        return Written::default();
    };
    let rest = after_year(text[at..].trim_start());
    // Names going on past the stop ("Lip, G. Y. H., & …", "Stephanie E.
    // Smallwood, Saltwater …"): the byline isn't where it was taken to end.
    let first = rest.split_whitespace().next().unwrap_or_default();
    // (A bare capital is a title's "A", not an initial.) A family name
    // ending its sentence ("Lionel P. Robert. 2013.") is no title.
    let more_names = first.ends_with(',')
        || dotted_initials(first)
        || first.strip_suffix('.').is_some_and(name_word)
        || matches!(first, "&" | "and" | "et" | "..." | "…");
    if more_names || !rest.starts_with(|c: char| c.is_uppercase() || c.is_ascii_digit()) {
        return Written {
            title: None,
            authors: Vec::new(),
        };
    }
    let title = sentence(rest)
        .map(|(title, after)| (without_label(title), after))
        .filter(|(title, after)| {
            let words = title.split_whitespace().count();
            let placed = after.chars().any(|c| c.is_ascii_digit());
            let balanced = |open: char, close: char| {
                title.matches(open).count() == title.matches(close).count()
            };
            words >= 2
                && placed
                && balanced('(', ')')
                && balanced('[', ']')
                && !title.contains(';')
                && !title
                    .split(", ")
                    .skip(1)
                    .any(|p| p.starts_with(|c: char| c.is_ascii_digit()))
                && !title.starts_with(['"', '“', '‘', '\''])
                && !title.starts_with("In ")
                && !title.starts_with("In:")
        });
    Written {
        title: title.map(|(t, _)| t.to_owned()),
        authors,
    }
}

/// A title in quotes right after the byline (Harvard's 'The effectiveness
/// of …', Journal; IEEE's "Attention is all you need," in …), and the names
/// before it. A phrase quoted inside a title ("The \"obesity paradox\" in
/// sepsis") isn't one, nor a quote's apostrophe ("Nurses' Characteristics").
fn quoted(text: &str) -> Option<Written> {
    let title = given_title(text)?;
    let at = text.find(title.as_str())?;
    let opening = text[..at].chars().next_back()?;
    let before = text[..at - opening.len_utf8()].trim_end();
    let after = text[at + title.len()..].trim_start_matches([',', '.', '?', '!']);
    let close = after.chars().next()?;
    // A quote closed on a word and a lowercase word after it: words quoted
    // inside a title ("\"Light\" on Phototherapy", "\"Care left undone\"
    // during …").
    let inside = after.len() == text.len() - at - title.len()
        && after[close.len_utf8()..]
            .trim_start()
            .starts_with(char::is_lowercase);
    if inside {
        return None;
    }
    let closes = match opening {
        '"' => close == '"',
        '“' => close == '”',
        '‘' | '\'' => {
            matches!(close, '’' | '\'')
                && after[close.len_utf8()..]
                    .chars()
                    .next()
                    .is_none_or(|c| ",.;:".contains(c))
        }
        _ => false,
    };
    if !closes {
        return None;
    }
    let authors = names(before);
    (authors.is_some() || before.is_empty()).then(|| Written {
        title: Some(title),
        authors: authors.unwrap_or_default(),
    })
}

/// What follows a year opening a stretch of a reference ("2011: Hearing
/// loss …", "(2016). Assessment …", "2016. Assessment …"): the year's,
/// not the title's.
fn after_year(rest: &str) -> &str {
    let bare = rest.strip_prefix('(').unwrap_or(rest);
    let digits = bare.chars().take_while(char::is_ascii_digit).count();
    if digits != 4 {
        return rest;
    }
    let after = bare[4..]
        .strip_prefix(|c: char| c.is_ascii_lowercase())
        .unwrap_or(&bare[4..]);
    let after = after.strip_prefix(')').unwrap_or(after);
    match after.strip_prefix(['.', ':', ',']) {
        Some(after) if after.starts_with(' ') => after.trim_start(),
        _ => rest,
    }
}

/// A title without the label after it: "[Data set]", "[dataset]".
fn without_label(title: &str) -> &str {
    match title
        .trim_end()
        .strip_suffix(']')
        .and_then(|t| t.rfind(" ["))
    {
        Some(at) => title[..at].trim_end(),
        None => title,
    }
}

/// The first sentence of `rest` and what follows it: up to a stop after a
/// word (not an initial or an abbreviation: "vs.", "U.S.", "ca."), or a
/// question's or exclamation's mark; on through a subtitle after either
/// ("Is ego depletion real? An analysis of arguments. Personality …,
/// 23(2)", "Social prescribing: less rhetoric and more reality. A
/// systematic review of the evidence. BMJ Open. 2017").
fn sentence(rest: &str) -> Option<(&str, &str)> {
    let mut from = 0;
    let mut previous = "";
    while let Some(i) = rest[from..].find(' ').map(|i| from + i) {
        let before = &rest[..i];
        let last = before.rsplit(' ').next().unwrap_or_default();
        let ended = if before.ends_with(['?', '!']) {
            true
        } else if let Some(word) = last.strip_suffix('.') {
            let word = word.trim_end_matches('.');
            let initial = word.chars().count() == 1
                && word.chars().all(char::is_uppercase)
                // "troponin T.", "vitamin D.": a title's last word.
                && !previous.starts_with(char::is_lowercase);
            let abbreviation = matches!(
                word.to_lowercase().as_str(),
                "vs" | "al"
                    | "st"
                    | "dr"
                    | "no"
                    | "vol"
                    | "ed"
                    | "eds"
                    | "ca"
                    | "c"
                    | "pp"
                    | "approx"
            ) || (word.contains('.')
                && word.chars().all(|c| c.is_alphabetic() || c == '.'));
            !word.is_empty() && !initial && !abbreviation
        } else {
            false
        };
        if ended && (before.ends_with(']') || !subtitled(&rest[i..])) {
            let title = before.trim_end_matches('.').trim_end();
            return Some((title, &rest[i..]));
        }
        previous = last;
        from = i + 1;
    }
    None
}

/// Whether what follows a title's stop or question's mark is its
/// subtitle: a sentence without numbers (not a journal and its volume,
/// APA's), not a reference's furniture ("In: …", "Available from") nor a
/// venue's name ("Journal of …", "… University Press"), with more than a
/// year or link after it (not the journal, Vancouver's), and with a
/// lowercase word of four letters or more (sentence case: not "JAMA",
/// "Expert Rev Anti Infect Ther"), or of four words or more with a
/// title's small words among them ("An Individual Patient Data
/// Meta-Analysis", "A Test of …").
fn subtitled(after: &str) -> bool {
    const VENUES: &[&str] = &[
        "Journal",
        "Press",
        "University",
        "Proceedings",
        "Annals",
        "Reviews",
        "Bulletin",
        "Archives",
        "Letters",
        "Society",
        "Database",
        "Institute",
        "Bureau",
        "Publishing",
        "Publishers",
        "company",
        "Company",
        "Library",
        "Documents",
        "Networks",
        "Conference",
    ];
    let next = after.trim_start();
    let (next, beyond) = next.split_once(". ").unwrap_or((next, ""));
    let beyond = beyond.trim_start().to_lowercase();
    let ends = beyond.is_empty()
        || beyond.starts_with(|c: char| c.is_ascii_digit() || c == '(')
        || ["doi", "http", "www", "available", "retrieved", "accessed"]
            .iter()
            .any(|p| beyond.starts_with(p));
    let words: Vec<&str> = next.split_whitespace().collect();
    if ends
        || words
            .iter()
            .any(|w| VENUES.contains(&w.trim_matches(|c: char| !c.is_alphabetic())))
    {
        return false;
    }
    let furniture = matches!(
        words.first().map(|w| w.trim_end_matches(':')),
        Some("In" | "Available" | "Accessed" | "Retrieved" | "Epub" | "Published" | "Unpublished")
    );
    let lowercase = words
        .iter()
        .any(|w| w.chars().count() >= 4 && w.starts_with(char::is_lowercase));
    let small = words.iter().any(|w| {
        matches!(
            *w,
            "a" | "an" | "the" | "of" | "in" | "for" | "and" | "with" | "on" | "to" | "from"
        )
    });
    !furniture
        && !next.chars().any(|c| c.is_ascii_digit())
        && next.starts_with(char::is_uppercase)
        && (lowercase || (words.len() >= 4 && small))
}

/// A reference without the number a list gives it: "1. ", "12) ", "[3] ".
fn unnumbered(text: &str) -> &str {
    let digits = text
        .trim_start_matches('[')
        .chars()
        .take_while(char::is_ascii_digit)
        .count();
    if digits == 0 || digits > 3 {
        return text;
    }
    let skip = text.len() - text.trim_start_matches('[').len() + digits;
    let after = &text[skip..];
    for mark in ["] ", ". ", ") ", "\t"] {
        if let Some(rest) = after.strip_prefix(mark) {
            return rest.trim_start();
        }
    }
    text
}

/// A byline's names as written, in its order, when every part of it reads
/// as a name in one style: Vancouver's "Seymour CW, Liu VX, et al.",
/// APA's and Harvard's "Seymour, C. W., & Liu, V. X. (2016)", or initials
/// first, "C. W. Seymour and V. X. Liu". `None` for anything else (an
/// organisation, a label, a title's words): no guess.
pub(super) fn names(byline: &str) -> Option<Vec<String>> {
    let s = without_tail(unnumbered(byline.trim()));
    if s.is_empty() {
        return None;
    }
    // The names joined by "and" or "&", each of them its parts between
    // commas: a family name and its initials (APA's) are never apart
    // across an "and".
    let groups: Vec<Vec<&str>> = s
        .split(" and ")
        .flat_map(|g| g.split(" & "))
        .map(|g| {
            g.split([',', ';'])
                .map(|p| {
                    let p = p.trim();
                    p.strip_prefix("and ")
                        .or_else(|| p.strip_prefix("& "))
                        .unwrap_or(p)
                        .trim()
                })
                // APA's ellipsis before the last of many authors.
                .filter(|p| !p.is_empty() && !matches!(*p, "..." | "…"))
                .collect::<Vec<&str>>()
        })
        .filter(|g| !g.is_empty())
        .collect();
    let pieces: Vec<&str> = groups.iter().flatten().copied().collect();
    if pieces.is_empty() {
        return None;
    }
    if pieces.iter().all(|p| family_initials(p)) {
        return Some(
            pieces
                .iter()
                .map(|p| {
                    // "Seymour CW." → "Seymour CW"; "Seymour C.W." stays.
                    let initials = p.rsplit(' ').next().unwrap_or_default();
                    if initials.trim_end_matches('.').contains('.') {
                        (*p).to_owned()
                    } else {
                        p.trim_end_matches('.').to_owned()
                    }
                })
                .collect(),
        );
    }
    let paired = |g: &Vec<&str>| {
        g.len().is_multiple_of(2)
            && g.chunks(2)
                .all(|pair| family(pair[0]) && dotted_initials(pair[1]))
    };
    if groups.iter().all(paired) {
        return Some(
            groups
                .iter()
                .flat_map(|g| g.chunks(2))
                .map(|pair| format!("{}, {}", pair[0], pair[1]))
                .collect(),
        );
    }
    if pieces.iter().all(|p| initials_family(p)) {
        return Some(pieces.iter().map(|p| (*p).to_owned()).collect());
    }
    // Chicago's and MLA's: the first name inverted, the rest in order
    // ("Deodato, Manuela, Caterina Qualizza, and Paolo Manganotti";
    // "Klein, Ira").
    if let [first, given, rest @ ..] = pieces.as_slice()
        && groups[0].len() >= 2
        && family(first)
        && given_names(given)
        && rest.iter().all(|p| in_order(p))
    {
        let mut found = vec![format!("{first}, {given}")];
        found.extend(rest.iter().map(|p| (*p).to_owned()));
        return Some(found);
    }
    // One family name before "et al." ("Gilmer et al.").
    if let [only] = pieces.as_slice()
        && family(only)
        && byline.to_lowercase().contains("et al")
    {
        return Some(vec![(*only).to_owned()]);
    }
    None
}

/// [`names`], and names written in order ("Javier Schloemann and R.
/// Michael Buehrer", "Philippe Dollinger"): only before a title found,
/// where the byline's end is known. Read from a layout, such names can't
/// be told from a title's capitalised words ("Saltwater Slavery").
fn names_before_title(byline: &str) -> Option<Vec<String>> {
    names(byline).or_else(|| {
        let s = without_tail(unnumbered(byline.trim()));
        let pieces: Vec<&str> = s
            .split(" and ")
            .flat_map(|g| g.split(" & "))
            .flat_map(|g| g.split([',', ';']))
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect();
        (!pieces.is_empty() && pieces.iter().all(|p| in_order(p)))
            .then(|| pieces.iter().map(|p| (*p).to_owned()).collect())
    })
}

/// Given names, written out, then any initials: "Ira", "Cindi K",
/// "Thomas S." (not "H. R. Triterpenoids").
fn given_names(p: &str) -> bool {
    let words: Vec<&str> = p.split_whitespace().collect();
    let initials = words.iter().position(|w| initial(w)).unwrap_or(words.len());
    (1..=3).contains(&words.len())
        && words[initials..].iter().all(|w| initial(w))
        && words
            .iter()
            .any(|w| name_word(w) && w.starts_with(char::is_uppercase))
        && words
            .iter()
            .all(|w| (name_word(w) && w.starts_with(char::is_uppercase)) || initial(w))
}

/// A name in order: given names, then the family name ("Caterina
/// Qualizza", "Quentin J. Holloway-Jessop", "R. Michael Buehrer").
fn in_order(p: &str) -> bool {
    let words: Vec<&str> = p.split_whitespace().collect();
    let Some((last, given)) = words.split_last() else {
        return false;
    };
    (1..=3).contains(&given.len())
        && name_word(last)
        && last.starts_with(char::is_uppercase)
        && given
            .iter()
            .all(|w| (name_word(w) && w.starts_with(char::is_uppercase)) || initial(w))
}

/// One initial: "K", "J.", "J.-L.".
fn initial(w: &str) -> bool {
    dotted_initials(w) || (w.chars().count() == 1 && w.chars().all(char::is_uppercase))
}

/// A byline without what may end it: "et al.", the year (APA's "(2016).",
/// Harvard's "(2016)", "2016."), and the stops and commas between.
fn without_tail(s: &str) -> &str {
    let mut s = s;
    loop {
        let before = s;
        s = s.trim_end_matches([' ', ',', ':', ';']);
        if let Some(t) = s.strip_suffix(").") {
            s = &s[..=t.len()];
        }
        if let Some(open) = s.strip_suffix(')').and_then(|t| t.rfind('(')) {
            let inner = &s[open + 1..s.len() - 1];
            if dated(inner) {
                s = &s[..open];
            }
        }
        let lower = s.to_lowercase();
        for tail in ["et al.", "et al", "and others", "et al.."] {
            if lower.ends_with(tail) {
                s = &s[..s.len() - tail.len()];
                break;
            }
        }
        if let Some((head, year)) = s.trim_end_matches('.').rsplit_once(' ')
            && dated(year)
        {
            s = head;
        }
        // The stop after a name written out ("… and Paolo Manganotti."),
        // not an initial's.
        if let Some(t) = s.strip_suffix('.')
            && t.ends_with(char::is_lowercase)
        {
            s = t;
        }
        if s == before {
            return s.trim();
        }
    }
}

/// A year as a byline gives it: "2016", "2016a", "n.d.", "2016, March 3".
fn dated(s: &str) -> bool {
    let s = s.trim();
    if s == "n.d." || s == "n.d" {
        return true;
    }
    let digits = s.chars().take_while(char::is_ascii_digit).count();
    let rest = &s[digits..];
    digits == 4 && (rest.is_empty() || rest.len() == 1 || rest.starts_with(", "))
}

/// A word of a name: capitalised letters ("O'Brien", "Jean-Louis",
/// "Øvretveit"), or a particle ("van", "de", "von", "da").
fn name_word(w: &str) -> bool {
    const PARTICLES: &[&str] = &[
        "van", "von", "de", "der", "den", "da", "di", "del", "della", "dos", "du", "la", "le",
        "ten", "ter", "bin", "al", "el",
    ];
    if PARTICLES.contains(&w) {
        return true;
    }
    let letters = w.chars().filter(|c| c.is_alphabetic()).count();
    w.starts_with(char::is_uppercase)
        && letters >= 2
        && w.chars().any(char::is_lowercase)
        && w.chars()
            .all(|c| c.is_alphabetic() || matches!(c, '-' | '\'' | '’'))
}

/// A family name: name words, one of them capitalised, at most four.
fn family(p: &str) -> bool {
    let words: Vec<&str> = p.split_whitespace().collect();
    (1..=4).contains(&words.len())
        && words.iter().all(|w| name_word(w))
        && words.iter().any(|w| w.starts_with(char::is_uppercase))
}

/// Initials undotted, Vancouver's: "CW", "C", "J-L", optionally a stop.
fn bare_initials(w: &str) -> bool {
    let w = w.strip_suffix('.').unwrap_or(w);
    let letters = w.chars().filter(|c| c.is_alphabetic()).count();
    (1..=4).contains(&letters)
        && w.chars().all(|c| c.is_uppercase() || c == '-')
        && w.starts_with(char::is_uppercase)
}

/// Initials dotted, APA's: "C. W.", "C.W.", "J.-L.", "C.".
fn dotted_initials(p: &str) -> bool {
    let joined: String = p.split_whitespace().collect();
    let letters = joined.chars().filter(|c| c.is_alphabetic()).count();
    (1..=6).contains(&letters)
        && joined.ends_with('.')
        && joined
            .chars()
            .all(|c| c.is_uppercase() || c == '.' || c == '-')
        && joined
            .split('.')
            .filter(|s| !s.is_empty())
            .all(|s| s.trim_start_matches('-').chars().count() == 1)
}

/// Vancouver's "Family Initials": "Seymour CW", "van der Poll T",
/// "Seymour C.W.", "Longstreth WT Jr".
fn family_initials(p: &str) -> bool {
    let p = ["Jr.", "Jr", "Sr.", "Sr", "II", "III", "IV", "2nd", "3rd"]
        .iter()
        .find_map(|s| p.strip_suffix(s).and_then(|p| p.strip_suffix(' ')))
        .unwrap_or(p);
    let Some((name, initials)) = p.rsplit_once(' ') else {
        return false;
    };
    family(name) && (bare_initials(initials) || dotted_initials(initials))
}

/// Initials, then the family name: "C. W. Seymour", "C.W. Seymour".
fn initials_family(p: &str) -> bool {
    let words: Vec<&str> = p.split_whitespace().collect();
    let given = words.iter().take_while(|w| dotted_initials(w)).count();
    given > 0 && given < words.len() && family(&words[given..].join(" "))
}
