//! Parts of a book's reference around its title that the layout's guess
//! ([`super::probable_title`]) leaves out: a publisher's name, a note of
//! an editor or translator in brackets, a count of volumes, a given name
//! after an initial.

use std::borrow::Cow;

/// Last words that make a name a publisher's ("Harvard Business School
/// Press", "Oxford UP", "Penguin Books").
const PRESS_LAST: &[&str] = &[
    "press",
    "presses",
    "up",
    "publishing",
    "publishers",
    "publisher",
    "verlag",
    "books",
];

/// First words that do ("Éditions du Seuil", "Edizioni Laterza").
const PRESS_FIRST: &[&str] = &[
    "éditions",
    "editions",
    "edizioni",
    "ediciones",
    "editorial",
    "editora",
];

/// Lower-case words a publisher's or a person's name may hold ("Routledge
/// & Kegan Paul", "il Mulino", "Éditions du Seuil", "Marcel de Vries").
const PARTICLES: &[&str] = &[
    "&", "and", "of", "the", "for", "il", "la", "le", "les", "de", "di", "del", "della", "du",
    "des", "von", "van", "der", "y",
];

/// A four-digit year.
fn year(w: &str) -> bool {
    w.len() == 4 && w.bytes().all(|b| b.is_ascii_digit())
}

/// Whether a sentence is a publisher's name, a year after it or not:
/// "Harvard Business School Press", "Princeton University Press, 1984",
/// "Oxford UP, 2002", "Éditions du Seuil, 1969". Its words capitalised
/// (particles aside), six at most, the last a publisher's kind or the
/// first an imprint's ("Éditions"). A title ending in "Press" has words
/// in lower case ("Freedom of the press in wartime").
pub(super) fn publisher_name(s: &str) -> bool {
    let t = s.trim().trim_end_matches('.');
    let t = match t.rsplit_once([',', ' ']) {
        Some((head, y)) if year(y) => head.trim_end_matches(',').trim_end(),
        _ => t,
    };
    let words: Vec<&str> = t.split_whitespace().collect();
    let (Some(first), Some(last)) = (words.first(), words.last()) else {
        return false;
    };
    let bare = |w: &str| {
        w.trim_matches(|c: char| !c.is_alphanumeric() && c != '&')
            .to_lowercase()
    };
    words.len() <= 6
        && words
            .iter()
            .all(|w| w.starts_with(char::is_uppercase) || PARTICLES.contains(&bare(w).as_str()))
        && (PRESS_LAST.contains(&bare(last).as_str())
            || PRESS_FIRST.contains(&bare(first).as_str()))
}

/// Whether a sentence gives identifiers or a link and little else:
/// "ISBN-13: 978-0521864497", "doi: 10.1007/…", "Available from:
/// https://…". Not a sentence with words enough besides (a citation
/// template's "|title=Markov Processes: … |isbn=…").
pub(super) fn identifiers(s: &str) -> bool {
    let lower = s.to_lowercase();
    let marked = [
        "isbn", "issn", "doi", "pmid", "arxiv", "http://", "https://", "www.",
    ]
    .iter()
    .any(|m| lower.contains(m));
    let labels = [
        "isbn",
        "issn",
        "doi",
        "pmid",
        "arxiv",
        "available",
        "from",
        "retrieved",
        "accessed",
        "disponible",
        "verfügbar",
        "online",
    ];
    let words = lower
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphabetic()))
        .filter(|w| w.chars().count() >= 3 && w.chars().all(char::is_alphabetic))
        .filter(|w| !labels.contains(w))
        .count();
    marked && words < 3
}

/// A sentence without the publisher a comma sets after its title, a year
/// after it or not: "Truth, Errors and Lies: Politics and Economics in a
/// Volatile World, Columbia UP". Only where two words or more are left.
pub(super) fn without_trailing_publisher(s: &str) -> &str {
    let t = s.trim_end().trim_end_matches('.');
    let t = match t.rsplit_once(", ") {
        Some((head, y)) if year(y) => head,
        _ => t,
    };
    match t.rsplit_once(", ") {
        Some((head, last)) if publisher_name(last) && head.split_whitespace().count() >= 2 => head,
        _ => s,
    }
}

/// A sentence without the count of volumes after its title: "Bristol,
/// Africa and the Eighteenth-Century Slave Trade to America, 4 vols".
pub(super) fn without_volumes(s: &str) -> &str {
    let t = s.trim_end().trim_end_matches('.');
    let Some((head, count)) = t.rsplit_once(", ") else {
        return s;
    };
    match count.split_whitespace().collect::<Vec<_>>()[..] {
        [n, "vols" | "volumes" | "Vols" | "voll"]
            if n.bytes().all(|b| b.is_ascii_digit()) && head.split_whitespace().count() >= 2 =>
        {
            head
        }
        _ => s,
    }
}

/// Words that open a note in brackets naming an editor, a translator, a
/// revision or another edition: "(ed. G. S. Koby)", "(ed. Tiedemann,
/// trans. Jephcott, 1998)", "(ed. it. I ribelli, Einaudi, 1966)".
const ROLES: &[&str] = &[
    "ed",
    "eds",
    "edd",
    "edited",
    "trans",
    "transl",
    "translated",
    "tr",
    "rev",
    "repr",
    "reprinted",
    "orig",
    "originally",
];

/// A reference without its notes in brackets naming an editor, a
/// translator or another edition, where the note names more than the role
/// ("(ed. Leppert, 2002)", not APA's "(Ed.)"): their full stops would
/// split the title's sentence ("Repairing Texts: … Processes (ed. G. S.
/// Koby). Kent State UP.").
pub(super) fn without_role_notes(text: &str) -> Cow<'_, str> {
    let mut out = String::new();
    let mut rest = text;
    let mut cut = false;
    while let Some(open) = rest.find(" (") {
        let inner_start = open + 2;
        let Some(close) = rest[inner_start..].find(')') else {
            break;
        };
        let inner = &rest[inner_start..inner_start + close];
        let role: String = inner
            .chars()
            .take_while(|c| c.is_alphabetic())
            .collect::<String>()
            .to_lowercase();
        let after = &inner[role.len().min(inner.len())..];
        let more = after
            .trim_start_matches(['.', ' '])
            .chars()
            .any(char::is_alphanumeric);
        let marked = after.starts_with(['.', ' ']);
        if ROLES.contains(&role.as_str()) && marked && more && !inner.contains('(') {
            out.push_str(&rest[..open]);
            rest = &rest[inner_start + close + 1..];
            cut = true;
        } else {
            out.push_str(&rest[..inner_start]);
            rest = &rest[inner_start..];
        }
    }
    if !cut {
        return Cow::Borrowed(text);
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// A capitalised word of letters, as a name's ("Cesare", "Roma-Bari").
fn capitalised(w: &str) -> bool {
    w.starts_with(char::is_uppercase)
        && w.chars().any(char::is_lowercase)
        && w.chars()
            .all(|c| c.is_alphabetic() || matches!(c, '-' | '\'' | '’'))
}

/// An initial: "J.", "J".
fn initial(w: &str) -> bool {
    let w = w.strip_suffix('.').unwrap_or(w);
    w.chars().count() == 1 && w.chars().all(char::is_uppercase)
}

/// A byline of one opening sentence, its family name and its initials
/// ("Christensen, C", "Connor, W"), as splitting a reference at its full
/// stops leaves it.
pub(super) fn family_initials(s: &str) -> bool {
    let Some((family, given)) = s.split_once(", ") else {
        return false;
    };
    let family: Vec<&str> = family.split_whitespace().collect();
    let given: Vec<&str> = given.split_whitespace().collect();
    (1..=2).contains(&family.len())
        && family.iter().all(|w| capitalised(w))
        && (1..=3).contains(&given.len())
        && given.iter().all(|w| initial(w))
}

/// Whether `s`, a sentence after `before`, is the given name written out
/// after the initial that ends `before`: "Robert" of "Connor, W. Robert.
/// Thucydides".
pub(super) fn given_after_initial(before: &str, s: &str) -> bool {
    family_initials(before) && {
        let words: Vec<&str> = s.split_whitespace().collect();
        (1..=2).contains(&words.len()) && words.iter().all(|w| capitalised(w))
    }
}
