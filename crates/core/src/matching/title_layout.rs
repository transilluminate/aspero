//! Parts of a book's reference around its title that the layout's guess
//! ([`super::probable_title`]) leaves out: a publisher's name, a note of
//! an editor or translator in brackets or a phrase naming them outside
//! ("…, translated by …"), authors named after the title ("… by …"), an
//! edition's statement, a count of volumes, a given name after an
//! initial, a byline before a comma or organisations' before a colon; and
//! the title of a reference listed as "Name, Title, Publisher, City Year",
//! as Italian, Spanish and French references are.

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

/// Words joining two people's names ("Giovanni Falcone e Marcelle
/// Padovani").
const JOINING_NAMES: &[&str] = &["e", "y", "et", "and", "&", "und"];

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

/// People's names written out, as a byline before a comma: "Cesare
/// Mori", "Eric J. Hobsbawm", "Giovanni Falcone e Marcelle Padovani". Each
/// two to four words, capitalised (an initial or a particle within).
fn names_written_out(s: &str) -> bool {
    let mut names = vec![Vec::new()];
    for w in s.split_whitespace() {
        if JOINING_NAMES.contains(&w) {
            names.push(Vec::new());
        } else if let Some(last) = names.last_mut() {
            last.push(w);
        }
    }
    names.iter().all(|name| {
        (2..=4).contains(&name.len())
            && name.first().is_some_and(|w| capitalised(w))
            && name.last().is_some_and(|w| capitalised(w))
            && name
                .iter()
                .all(|w| capitalised(w) || initial(w) || PARTICLES.contains(w))
    })
}

/// A publisher's name between commas, as a listed reference gives it
/// ("Einaudi", "il Mulino", "Manchester University Press"): one to four
/// words, the last capitalised, the others capitalised or particles; not
/// across a sentence's end ("and Colonialism. Oxford UP"), nor Chicago's
/// state and publisher ("NJ: Princeton University Press").
fn publisher_between_commas(s: &str) -> bool {
    let words: Vec<&str> = s.split_whitespace().collect();
    (1..=4).contains(&words.len())
        && !s.contains(". ")
        && !s.contains(':')
        && words.first().is_some_and(|w| !JOINING_NAMES.contains(w))
        && words
            .last()
            .is_some_and(|w| w.starts_with(char::is_uppercase))
        && words.iter().all(|w| {
            (w.starts_with(char::is_uppercase) || PARTICLES.contains(w))
                && !w.chars().any(|c| c.is_ascii_digit())
        })
}

/// The title of a reference listed as "Name, Title, Publisher, City Year"
/// or "Title, Publisher, Year" (Italian, Spanish and French references:
/// "Cesare Mori, Con la mafia ai ferri corti, Mondadori, 1932", "Pino
/// Arlacchi, La mafia imprenditrice, il Mulino, Bologna 1983", "La mafia.
/// Centosessant'anni di storia, Donzelli, 2018"), a note in brackets after
/// it or not. Its last part a year, a city's name or two before it or
/// not; the one before it a publisher's name; the first, where people's
/// names written out, the byline. Not a title that reads as names itself
/// ("Machine Learning, Tom Mitchell, …"), unless after a byline an
/// initial or a second name marks as one ("Eric J. Hobsbawm, Primitive
/// Rebels, …", "Giovanni Falcone e Marcelle Padovani, Cose di Cosa
/// Nostra, …"); nor
/// one naming people as a byline does ("Smith J, Jones K, …"). Without a
/// byline, the title is the one part before the publisher; either way it
/// holds one sentence's end at most (a title and its subtitle, not
/// Chicago's "Herrin, Judith. The Formation of Christendom. Princeton").
pub(super) fn listed_title(text: &str) -> Option<String> {
    let t = super::titles::without_trailing_note(text.trim())
        .trim_end()
        .trim_end_matches('.');
    let parts: Vec<&str> = t.split(", ").collect();
    let [head @ .., publisher, last] = &parts[..] else {
        return None;
    };
    let dated: Vec<&str> = last.split_whitespace().collect();
    let (Some(y), city) = (dated.last(), &dated[..dated.len().saturating_sub(1)]) else {
        return None;
    };
    if !year(y) || city.len() > 2 || !city.iter().all(|w| capitalised(w)) {
        return None;
    }
    if !publisher_between_commas(publisher) {
        return None;
    }
    let (title, marked) = match head {
        [byline, title @ ..] if !title.is_empty() && names_written_out(byline) => {
            let marked = byline
                .split_whitespace()
                .any(|w| initial(w) || JOINING_NAMES.contains(&w));
            (title, marked)
        }
        [_] => (head, false),
        _ => return None,
    };
    let title = title.join(", ");
    let words = title.split_whitespace().count();
    let named = (names_written_out(&title) && !marked) || super::authors::names_given(&title);
    let sentences = title.matches(". ").count();
    (words >= 2 && !named && sentences <= 1 && !title.contains(['(', ')']))
        .then(|| super::titles::title_like(&title))
        .flatten()
}

/// An edition's ordinal or kind before "ed." or "edition": "4th", "2nd",
/// "Fourth", "rev.", "new".
fn edition_word(w: &str) -> bool {
    let w = w
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    let digits = w.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let numbered = !digits.is_empty()
        && digits.len() < w.len()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && matches!(&w[digits.len()..], "st" | "nd" | "rd" | "th");
    numbered
        || matches!(
            w.as_str(),
            "first"
                | "second"
                | "third"
                | "fourth"
                | "fifth"
                | "sixth"
                | "seventh"
                | "eighth"
                | "ninth"
                | "tenth"
                | "rev"
                | "revised"
                | "new"
                | "expanded"
                | "updated"
        )
}

/// Whether a sentence ends in an edition's statement ("…, 4th ed", "2nd
/// edition"): not an editors' "eds" after names.
pub(super) fn ends_in_edition(s: &str) -> bool {
    let words: Vec<&str> = s
        .trim_end()
        .trim_end_matches(['.', ')'])
        .split_whitespace()
        .collect();
    match words[..] {
        [.., before, last] => {
            let last = last
                .trim_matches(|c: char| !c.is_alphabetic())
                .to_lowercase();
            matches!(last.as_str(), "ed" | "edn" | "edition") && edition_word(before)
        }
        _ => false,
    }
}

/// A sentence without the edition's statement that ends it: "Motivational
/// Interviewing: Helping People Change and Grow, 4th ed" (wild-6 #92).
/// Only where two words or more are left.
pub(super) fn without_edition(s: &str) -> &str {
    if !ends_in_edition(s) {
        return s;
    }
    let t = s.trim_end().trim_end_matches(['.', ')']);
    let Some(cut) = t
        .rfind(|c: char| c.is_whitespace())
        .and_then(|last| t[..last].trim_end().rfind(|c: char| c.is_whitespace()))
    else {
        return s;
    };
    let head = t[..cut]
        .trim_end()
        .trim_end_matches([',', '(', ';'])
        .trim_end();
    if head.split_whitespace().count() >= 2 {
        head
    } else {
        s
    }
}

/// The length of the people's names `s` opens with: capitalised words,
/// initials and particles, joined by commas, "and" or "&"; each name two
/// to four words, its last a capitalised word ("J. Lennart Berggren",
/// "Andrew Wilcock").
fn names_run(s: &str) -> usize {
    let mut end = 0;
    for (at, w) in s
        .split_whitespace()
        .map(|w| (w.as_ptr() as usize - s.as_ptr() as usize, w))
    {
        let bare = w.trim_end_matches([',', ';']);
        let name = capitalised(bare.trim_end_matches('.'))
            || initial(bare)
            || PARTICLES.contains(&bare)
            || JOINING_NAMES.contains(&bare);
        if !name {
            break;
        }
        end = at + w.len();
        if w.ends_with([';', ':']) || (w.ends_with('.') && !initial(w)) {
            break;
        }
    }
    let run = s[..end].trim_end_matches(['.', ',', ';', ':']);
    let mut names = vec![Vec::new()];
    for w in run.split_whitespace() {
        let w = w.trim_end_matches(',');
        if JOINING_NAMES.contains(&w) {
            names.push(Vec::new());
        } else if let Some(last) = names.last_mut() {
            last.push(w);
        }
    }
    let people = names.iter().all(|name| {
        (2..=4).contains(&name.len())
            && name.last().is_some_and(|w| capitalised(w))
            && (name.iter().filter(|w| capitalised(w)).count() >= 2
                || name.iter().any(|w| initial(w)))
    });
    if people { run.len() } else { 0 }
}

/// Phrases naming who translated or edited a work, outside brackets: ",
/// translated by J. Lennart Berggren and Alexander Jones" (wild-6 #703).
const ROLE_PHRASES: &[&str] = &[
    ", translated by ",
    ", trans. by ",
    ", transl. by ",
    ", edited by ",
    ", ed. by ",
    ", introduced by ",
    ", with an introduction by ",
    " translated by ",
    " edited by ",
];

/// A reference without a phrase naming its translators or editors,
/// unbracketed ("Ptolemy, Geography, translated by J. Lennart Berggren
/// and Alexander Jones (2000)"), or its authors after its title ("
/// Introducing Palliative Care by Richard Twycross and Andrew Wilcock",
/// wild-6 #247): their initials' full stops would split the title's
/// sentence, or their names run on in it. "by" alone only before names
/// that end the reference (a year or note in brackets aside), after two
/// words or more.
pub(super) fn without_name_phrases(text: &str) -> Cow<'_, str> {
    let lower = text.to_lowercase();
    for phrase in ROLE_PHRASES {
        if let Some(at) = lower.find(phrase) {
            let from = at + phrase.len();
            let run = names_run(&text[from..]);
            if run > 0 {
                let rest = text[from + run..].trim_start_matches(['.', ',']);
                let joint = if rest.trim().is_empty() || rest.starts_with(' ') {
                    ""
                } else {
                    " "
                };
                return Cow::Owned(format!("{}{joint}{rest}", &text[..at]));
            }
        }
    }
    if let Some(at) = lower.rfind(" by ") {
        let from = at + 4;
        let run = names_run(&text[from..]);
        let rest = text[from + run..].trim();
        let bare = rest.trim_end_matches(['.', ',', ';']).trim();
        let ends = bare.is_empty()
            || (bare.starts_with('(') && bare.ends_with(')') && bare.matches('(').count() == 1);
        if run > 0 && ends && text[..at].split_whitespace().count() >= 2 {
            return Cow::Owned(format!("{}{}", &text[..at], rest));
        }
    }
    Cow::Borrowed(text)
}

/// A sentence without a byline of organisations before a colon:
/// "HPA / College of Radiographers / RCR: Protection of Pregnant
/// Patients …" (wild-6 #217). Bodies listed with slashes, or one named
/// as a body ("Royal College of Radiologists:"); not a title's own
/// acronym ("BERT: Pre-training of …").
pub(super) fn without_body_byline(s: &str) -> &str {
    let Some((byline, rest)) = s.split_once(": ") else {
        return s;
    };
    let words: Vec<&str> = byline.split_whitespace().collect();
    let shaped = words.iter().all(|w| {
        *w == "/"
            || w.starts_with(char::is_uppercase)
            || PARTICLES.contains(w)
            || w.chars().all(|c| c.is_uppercase() || c == '/')
    });
    let bodies = byline.contains(" / ")
        || words
            .iter()
            .any(|w| crate::work::is_body_word(&w.to_lowercase()));
    if shaped && bodies && words.len() <= 12 && rest.split_whitespace().count() >= 3 {
        rest
    } else {
        s
    }
}

/// A sentence without a byline of a name written out and a comma before
/// a title that a year in brackets ends: "Einar Arnórsson, Réttarsaga
/// Alþingis (1945)", "Eddie Faye Gates, They Came Searching: … (1997)".
/// Two to four words, each a capitalised word of letters; one before a
/// title opening with an article ("Snowdon, The Spirit Level Delusion
/// (2010)", wild-6 #265). The brackets may give the place and publisher
/// before the year, a page after them, as a note does ("François
/// Blancpain, Un siècle … (Paris: L'Harmattan, 2001), [page]", wild-6
/// #519). An editor's byline needs no year: "James Gairdner, ed., The
/// Paston Letters, 1422–1509" (wild-6 #347).
pub(super) fn without_name_byline(s: &str) -> &str {
    let Some((name, rest)) = s.split_once(", ") else {
        return s;
    };
    let words: Vec<&str> = name.split_whitespace().collect();
    let name_word = |w: &&str| {
        w.starts_with(char::is_uppercase)
            && w.chars().any(char::is_lowercase)
            && w.chars()
                .all(|c| c.is_alphabetic() || matches!(c, '-' | '\'' | '’'))
    };
    let article = ["The ", "A ", "An "].iter().any(|a| rest.starts_with(a));
    let named = ((2..=4).contains(&words.len()) || (words.len() == 1 && article))
        && words.iter().all(name_word);
    if (2..=4).contains(&words.len()) && words.iter().all(name_word) {
        for role in ["ed., ", "eds., ", "(ed.), ", "(eds.), "] {
            if let Some(after) = rest.strip_prefix(role)
                && after.starts_with(char::is_uppercase)
            {
                return after;
            }
        }
    }
    let t = without_page(rest.trim_end().trim_end_matches('.'));
    let dated = t
        .strip_suffix(')')
        .and_then(|t| t.rfind(" (").map(|at| &t[at + 2..]))
        .is_some_and(|inner| {
            let year = inner.rsplit_once(", ").map_or(inner, |(place, year)| {
                if place.contains(": ") { year } else { inner }
            });
            year.get(..4)
                .is_some_and(|y| y.bytes().all(|b| b.is_ascii_digit()))
        });
    if named && dated && rest.starts_with(char::is_uppercase) {
        super::titles::without_trailing_note(&rest[..t.len()])
    } else {
        s
    }
}

/// A reference without the page a note cites after it: ", [page]", ",
/// 45", ", 45–47", ", p. 12".
fn without_page(s: &str) -> &str {
    let Some((head, page)) = s.rsplit_once(", ") else {
        return s;
    };
    let page = page
        .trim_start_matches("pp.")
        .trim_start_matches("p.")
        .trim();
    let bare = page.trim_matches(['[', ']']);
    let numbered = !bare.is_empty() && bare.chars().all(|c| c.is_ascii_digit() || "-–".contains(c));
    if bare == "page" || bare == "pages" || numbered {
        head
    } else {
        s
    }
}
