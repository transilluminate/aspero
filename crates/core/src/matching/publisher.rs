//! A book's publisher: the one its citation names, and whether the
//! book's editions know it.
//!
//! Imprints and reissues change a book's publisher legitimately (a
//! Palgrave title reissued by Routledge, CRC Press for Taylor & Francis),
//! so the publisher of one record says little. A publisher that none of a
//! book's editions names, where many are known, does (holdout-6's
//! `book-wrong-publisher`). Names compare by their own words, their
//! furniture aside ("Palgrave Macmillan UK" is Palgrave Macmillan, "OUP"
//! Oxford University Press), and imprints with their groups
//! ([`IMPRINTS`]): only names sharing nothing differ.

use std::collections::BTreeMap;

use super::Claim;
use super::text::{Normalised, words};
use crate::publication;
use crate::record::SourceRecord;
use crate::work::Work;

/// Words that mark a name as a publisher's, where free text gives one.
const PUBLISHER_WORDS: &[&str] = &[
    "press",
    "presses",
    "publishing",
    "publishers",
    "publisher",
    "publications",
    "verlag",
    "editions",
    "university",
    "books",
];

/// Words of a publisher's name that don't tell one from another: its
/// kind, its legal form, its country.
const FURNITURE: &[&str] = &[
    "the",
    "a",
    "an",
    "and",
    "of",
    "for",
    "de",
    "des",
    "du",
    "la",
    "le",
    "et",
    "und",
    "press",
    "presses",
    "publishing",
    "publishers",
    "publisher",
    "publications",
    "publication",
    "pub",
    "pubs",
    "publ",
    "verlag",
    "verlags",
    "editions",
    "edition",
    "editorial",
    "editora",
    "editeur",
    "editeurs",
    "university",
    "univ",
    "universitaires",
    "universitaire",
    "up",
    "books",
    "book",
    "ebooks",
    "group",
    "inc",
    "incorporated",
    "ltd",
    "limited",
    "llc",
    "plc",
    "co",
    "company",
    "corp",
    "corporation",
    "gmbh",
    "ag",
    "sa",
    "bv",
    "nv",
    "uk",
    "us",
    "usa",
    "international",
    "sons",
    "son",
    "media",
    "business",
    "science",
    "imprint",
    "academic",
    "new",
    "york",
    "london",
];

/// Publishing groups whose imprints share no word with each other: a
/// book of one is cited, correctly, as of another (Erlbaum's books are
/// Psychology Press's, Taylor & Francis's and Routledge's). Each name's
/// own words, as [`significant`] reads them.
const IMPRINTS: &[&[&str]] = &[
    &[
        "routledge",
        "taylor francis",
        "crc",
        "psychology",
        "garland",
        "focal",
        "earthscan",
        "informa",
        "erlbaum",
        "chapman hall",
        "brunner",
        "frank cass",
        "tavistock",
    ],
    &[
        "macmillan",
        "palgrave",
        "springer",
        "nature",
        "st martin s",
        "apress",
        "kluwer",
        "humana",
        "plenum",
    ],
    &[
        "elsevier",
        "pergamon",
        "north holland",
        "butterworth",
        "butterworths",
        "heinemann",
        "saunders",
        "mosby",
        "churchill livingstone",
        "morgan kaufmann",
        "woodhead",
        "cell",
    ],
    &["wiley", "blackwell", "jossey bass", "interscience", "vch"],
    &["oxford", "oup", "clarendon"],
    &["cambridge", "cup"],
    &["sage", "corwin", "cq"],
    &[
        "penguin",
        "random house",
        "viking",
        "knopf",
        "vintage",
        "doubleday",
        "bantam",
        "allen lane",
        "pantheon",
        "crown",
        "anchor",
        "jonathan cape",
        "chatto windus",
        "bodley head",
        "hamish hamilton",
    ],
    &["harper", "harpercollins", "collins", "william morrow"],
    &[
        "pearson",
        "prentice hall",
        "addison wesley",
        "longman",
        "allyn bacon",
        "merrill",
    ],
    &[
        "cengage",
        "wadsworth",
        "brooks cole",
        "thomson",
        "delmar",
        "south western",
    ],
    // Herder and Herder merged into Seabury, whose line became Continuum.
    &[
        "bloomsbury",
        "continuum",
        "t t clark",
        "berg",
        "hart",
        "methuen",
        "herder",
        "seabury",
    ],
    &["brill", "nijhoff", "rodopi", "vandenhoeck ruprecht"],
    &["de gruyter", "gruyter", "mouton", "saur"],
    &["mcgraw hill", "irwin"],
    &[
        "hachette",
        "little brown",
        "basic",
        "publicaffairs",
        "perseus",
    ],
    &["simon schuster", "free", "scribner", "touchstone"],
];

/// Parts of a reference after its title that are something else than a
/// publisher: an edition, a volume, pages, identifiers, a reprint's or an
/// original's note, a thesis.
const NOT_PUBLISHER: &[&str] = &[
    "ed",
    "edn",
    "eds",
    "edition",
    "editor",
    "editors",
    "edited",
    "vol",
    "vols",
    "volume",
    "volumes",
    "pp",
    "p",
    "isbn",
    "doi",
    "http",
    "https",
    "www",
    "retrieved",
    "accessed",
    "available",
    "published",
    "reprint",
    "reprinted",
    "original",
    "originally",
    "translated",
    "trans",
    "thesis",
    "dissertation",
    "manuscript",
    "unpublished",
    "kindle",
    "ebook",
    "series",
    "chapter",
    "in",
];

/// The words of a name that tell it from another's.
fn significant(words: &[String]) -> Vec<&str> {
    words
        .iter()
        .map(String::as_str)
        .filter(|w| !FURNITURE.contains(w) && !w.chars().all(|c| c.is_ascii_digit()))
        .collect()
}

/// Whether `short` is the initials of `name`'s words, as "OUP" is of
/// Oxford University Press's, or of its words but its kind's and form's
/// ("LSU Press" of Louisiana State University Press); initials written
/// apart ("N.L.B.") are one word.
fn initials_of(short: &[String], name: &[String]) -> bool {
    let joined;
    let short: &str = match short {
        [one] => one,
        letters if letters.len() >= 2 && letters.iter().all(|w| w.chars().count() == 1) => {
            joined = letters.concat();
            &joined
        }
        _ => match significant(short)[..] {
            [one] => one,
            _ => return false,
        },
    };
    let initials = |skip: &[&str]| -> String {
        name.iter()
            .filter(|w| !skip.contains(&w.as_str()))
            .filter_map(|w| w.chars().next())
            .collect()
    };
    let joining = ["the", "of", "and", "for"];
    let kind = [
        "the",
        "of",
        "and",
        "for",
        "press",
        "publishing",
        "publishers",
        "publisher",
        "books",
        "inc",
        "ltd",
        "co",
        "company",
    ];
    short.chars().count() >= 2 && (short == initials(&joining) || short == initials(&kind))
}

/// The publishing group a name is an imprint of, by [`IMPRINTS`]: one
/// whose imprint's words all stand together in the name.
fn group(words: &[String]) -> Option<usize> {
    let name = format!(" {} ", words.join(" "));
    IMPRINTS.iter().position(|imprints| {
        imprints
            .iter()
            .any(|imprint| name.contains(&format!(" {imprint} ")))
    })
}

/// Whether two publishers' names are one publisher's, or its group's: a
/// word of their own shared, one the other's initials, or imprints of one
/// group. Names without words of their own ("University Press") are
/// taken as the same: nothing tells them apart.
pub fn same_publisher(cited: &str, actual: &str) -> bool {
    let (c, a) = (words(cited), words(actual));
    let (cs, as_) = (significant(&c), significant(&a));
    cs.is_empty()
        || as_.is_empty()
        || cs.iter().any(|w| as_.contains(w))
        || initials_of(&c, &a)
        || initials_of(&a, &c)
        || group(&c).is_some_and(|g| group(&a) == Some(g))
}

/// The publisher a book's citation names, read against `record` (the
/// book): a BibTeX entry's, a RIS record's (`PB  -`), a template's
/// (`|publisher=`); else, after the book's title in free text, a part
/// naming a publisher by its words ("Oxford University Press"), one that
/// stands between a place and a year ("London: Routledge, 2013"), or the
/// last part that can be a name ("… Syntax (third volume). Palgrave
/// Macmillan."). `None` when the title can't be placed in the text.
pub fn cited_publisher(claim: &Claim, record: &SourceRecord) -> Option<String> {
    let raw = claim.text.as_deref()?;
    if let Some(publisher) = publication::bibtex_field(raw, "publisher") {
        return Some(publisher);
    }
    if let Some(publisher) = tagged(raw) {
        return Some(publisher);
    }
    let text = Normalised::new(raw);
    let span = text.title_span(record.work.title.as_deref()?)?;
    let end = text.spans.get(span.end.checked_sub(1)?)?.end;
    // The book's own authors and editors named after its title ("eds.
    // John Hope Franklin") are none.
    let families: Vec<String> = record
        .work
        .authors
        .iter()
        .filter_map(|a| a.family.clone().or_else(|| a.literal.clone()))
        .flat_map(|name| words(&name))
        .filter(|w| w.chars().count() > 2)
        .collect();
    let parts: Vec<Part> = parts_after_title(&text.raw[end..])
        .into_iter()
        .filter(|p| !words(&p.name).iter().any(|w| families.contains(w)))
        .collect();
    let by_word = |p: &&Part| {
        words(&p.name)
            .iter()
            .any(|w| PUBLISHER_WORDS.contains(&w.as_str()))
    };
    let by_place = |p: &&Part| p.placed && p.dated;
    parts
        .iter()
        .find(by_word)
        .or_else(|| parts.iter().find(by_place))
        .or_else(|| parts.iter().rev().find(|p| p.dated || p.last))
        .map(|p| p.name.clone())
}

/// A RIS record's `PB` or a wiki template's `publisher`.
fn tagged(raw: &str) -> Option<String> {
    let ris = raw.lines().find_map(|line| {
        let rest = line.trim_start().strip_prefix("PB")?.trim_start();
        Some(rest.strip_prefix('-')?.trim().to_owned())
    });
    let template = || {
        let at = raw.find("publisher")?;
        let before = raw[..at].trim_end();
        if !before.ends_with('|') {
            return None;
        }
        let rest = raw[at + "publisher".len()..]
            .trim_start()
            .strip_prefix('=')?;
        let end = rest.find(['|', '}']).unwrap_or(rest.len());
        Some(rest[..end].trim().to_owned())
    };
    ris.or_else(template).filter(|p| !p.is_empty())
}

/// A part of a reference between its punctuation.
struct Part {
    /// The part, after a place ("London: ").
    name: String,
    /// Whether a place stood before it ("London: Routledge").
    placed: bool,
    /// Whether a year follows it ("Routledge, 2013", "Routledge; 2013").
    dated: bool,
    /// Whether it is the reference's last part with words.
    last: bool,
}

/// The parts of what follows a title that can be a publisher's name,
/// split at its punctuation: words, the first a capital's or a digit's,
/// at most eight of them, none of [`NOT_PUBLISHER`].
fn parts_after_title(tail: &str) -> Vec<Part> {
    // Links and identifiers aside: their dots split them into parts.
    let linked = |t: &&str| {
        let t = t.trim_start_matches(['(', '[', '<']).to_ascii_lowercase();
        t.contains("://") || t.starts_with("www.") || t.starts_with("doi:") || t.starts_with("10.")
    };
    let tail: String = tail
        .split_whitespace()
        .filter(|t| !linked(t))
        .collect::<Vec<_>>()
        .join(" ");
    // A subtitle the book's record lacks, after its title, is none.
    let subtitled = tail.trim_start().starts_with([':', '–', '—']);
    let pieces: Vec<&str> = tail
        .split([
            '.', ',', ';', '(', ')', '[', ']', '*', '"', '“', '”', '_', '|',
        ])
        .map(str::trim)
        .collect();
    let first = pieces
        .iter()
        .position(|p| p.chars().any(char::is_alphabetic));
    let year = |s: &str| {
        let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
        digits.len() == 4
    };
    let worded: Vec<usize> = (0..pieces.len())
        .filter(|&i| pieces[i].chars().any(char::is_alphabetic))
        .collect();
    let last = worded.last().copied();
    worded
        .into_iter()
        .filter_map(|i| {
            let p = pieces[i];
            let (placed, name) = match p.rsplit_once(':') {
                Some((place, name)) if !place.trim().is_empty() => (true, name.trim()),
                Some((_, name)) => (false, name.trim()),
                None => (false, p),
            };
            let named = words(name);
            let nameable = name
                .chars()
                .next()
                .is_some_and(|c| c.is_uppercase() || c.is_ascii_digit())
                && (1..=8).contains(&named.len())
                && !named
                    .iter()
                    .any(|w| NOT_PUBLISHER.contains(&w.as_str()) || ordinal(w))
                && !(subtitled && Some(i) == first);
            nameable.then(|| Part {
                name: name.to_owned(),
                placed,
                dated: pieces[i + 1..]
                    .iter()
                    .find(|next| !next.is_empty())
                    .is_some_and(|next| year(next)),
                last: Some(i) == last,
            })
        })
        .collect()
}

/// "2nd", "5th", "21st": an edition's number.
fn ordinal(word: &str) -> bool {
    let digits = word.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    !digits.is_empty()
        && digits.len() < word.len()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && matches!(&word[digits.len()..], "st" | "nd" | "rd" | "th")
}

/// Editions with a publisher, at least, before a publisher none of them
/// names is held against a citation, when the book's record names none
/// either: fewer may be all the editions a catalogue happens to hold.
pub const EDITIONS_ENOUGH: usize = 3;

/// What a book's records and editions say of the publisher its citation
/// names.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PublisherCheck {
    /// An edition or the book's record names it, or its group
    /// ([`same_publisher`]); or a publisher they name stands anywhere in
    /// the citation.
    Named,
    /// None does, and enough editions are known to tell
    /// ([`EDITIONS_ENOUGH`]): the publisher most of them name (else the
    /// record's).
    Unnamed(String),
    /// None does, but too few editions are known to tell.
    TooFew,
}

/// Checks `cited`, the publisher a book's citation (`text`, citing
/// `years`) names, against the book's own record and its `editions` (Open
/// Library's). It is [`Unnamed`](PublisherCheck::Unnamed) when an edition
/// is of a year cited (or one off it), and at least [`EDITIONS_ENOUGH`]
/// editions name a publisher, or at least one does and the record names
/// another too: two catalogues agreeing. Without an edition of the year
/// cited, the catalogue lacks the one cited: too few to tell.
pub fn check_publisher(
    cited: &str,
    text: Option<&str>,
    years: &[i16],
    record: &Work,
    editions: &[Work],
) -> PublisherCheck {
    let names = |w: &Work| -> Vec<String> {
        w.publisher
            .iter()
            .flat_map(|p| p.split(';'))
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let mut own = names(record);
    // OpenAlex's source for a publisher's books: "Routledge eBooks".
    if let Some(series) = record.container_title.as_deref()
        && let Some(publisher) = series.strip_suffix(" eBooks")
    {
        own.push(publisher.to_owned());
    }
    let printed: Vec<Vec<String>> = editions.iter().map(names).collect();
    let text = text.map(words).unwrap_or_default();
    let in_text = |name: &str| {
        let own = words(name);
        let own = significant(&own);
        !own.is_empty() && own.iter().all(|w| text.iter().any(|t| t == w))
    };
    if own
        .iter()
        .chain(printed.iter().flatten())
        .any(|k| same_publisher(cited, k) || in_text(k))
    {
        return PublisherCheck::Named;
    }
    let with_publisher = printed.iter().filter(|p| !p.is_empty()).count();
    let of_the_year = editions.iter().any(|e| {
        e.issued
            .is_some_and(|d| years.iter().any(|y| y.abs_diff(d.year()) <= 1))
    });
    if !of_the_year || (with_publisher < EDITIONS_ENOUGH && (with_publisher == 0 || own.is_empty()))
    {
        return PublisherCheck::TooFew;
    }
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for name in printed.iter().flatten() {
        *counts.entry(name.as_str()).or_default() += 1;
    }
    // The most named, the first named of those tied.
    let most = counts.values().max().copied().and_then(|top| {
        printed
            .iter()
            .flatten()
            .find(|name| counts.get(name.as_str()) == Some(&top))
            .cloned()
    });
    match most.or_else(|| own.first().cloned()) {
        Some(name) => PublisherCheck::Unnamed(name),
        None => PublisherCheck::TooFew,
    }
}
