//! A book's publisher: the one a claim cites, and whether it is clearly
//! another than a record's.
//!
//! Imprints and reissues change a book's publisher legitimately (a
//! Palgrave title reissued by Routledge, CRC Press for Taylor & Francis),
//! so another publisher alone says nothing; with another year too, the
//! citation may be another edition ([`super::assess()`]). Names compare by
//! their own words, their furniture aside ("Palgrave Macmillan UK" is
//! Palgrave Macmillan, "OUP" Oxford University Press): only names sharing
//! nothing differ. No list of publishers is kept.

use std::ops::Range;

use super::Claim;
use super::assess::{claim_text, own_title};
use super::text::{Normalised, words};
use crate::publication;
use crate::record::SourceRecord;

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
/// Oxford University Press's.
fn initials_of(short: &[String], name: &[String]) -> bool {
    let [short] = short else { return false };
    let initials: String = name
        .iter()
        .filter(|w| !matches!(w.as_str(), "the" | "of" | "and" | "for"))
        .filter_map(|w| w.chars().next())
        .collect();
    short.len() >= 2 && *short == initials
}

/// Whether a cited publisher is clearly another than a record's: each
/// names itself by words the other's lacks, and neither is the other's
/// initials. Names without words of their own ("University Press") are
/// none to tell.
pub fn publishers_differ(cited: &str, actual: &str) -> bool {
    let (c, a) = (words(cited), words(actual));
    !significant(&c).is_empty() && !significant(&a).is_empty() && !same_names(&c, &a)
}

/// Whether two names share a word of their own, or one is the other's
/// initials.
fn same_names(c: &[String], a: &[String]) -> bool {
    let (cs, as_) = (significant(c), significant(a));
    cs.iter().any(|w| as_.contains(w)) || initials_of(c, a) || initials_of(a, c)
}

/// The publisher a claim cites, as read against `record`'s
/// ([`cited_publisher`]); `None` when the record names none.
pub fn publisher_cited(claim: &Claim, record: &SourceRecord) -> Option<String> {
    let actual = record.work.publisher.as_deref()?;
    let text = claim_text(claim, &record.work);
    let span = match (&claim.title, own_title(&record.work), &text) {
        (None, Some(title), Some(text)) => text.title_span(&title),
        _ => None,
    };
    cited_publisher(claim, text.as_ref(), span.as_ref(), actual)
}

/// The publisher a claim cites, compared with a book's (`actual`): its
/// own, a BibTeX entry's, else a part of its text after the work's title
/// (at `span`) that names `actual`, or names a publisher by its words
/// ("Oxford University Press", "Springer-Verlag") or by where it stands
/// ("London: Routledge, 2013").
pub(super) fn cited_publisher(
    claim: &Claim,
    text: Option<&Normalised>,
    span: Option<&Range<usize>>,
    actual: &str,
) -> Option<String> {
    if let Some(publisher) = &claim.publisher {
        return Some(publisher.clone());
    }
    let text = text?;
    if let Some(publisher) = publication::bibtex_field(&text.raw, "publisher") {
        return Some(publisher);
    }
    let end = text.spans.get(span?.end.checked_sub(1)?)?.end;
    let parts = parts_after_title(&text.raw[end..]);
    let actual = words(actual);
    let named = |p: &&Part| same_names(&words(&p.name), &actual);
    let by_word = |p: &&Part| {
        words(&p.name)
            .iter()
            .any(|w| PUBLISHER_WORDS.contains(&w.as_str()))
    };
    let by_place = |p: &&Part| p.placed && p.dated;
    parts
        .iter()
        .find(named)
        .or_else(|| parts.iter().find(by_word))
        .or_else(|| parts.iter().find(by_place))
        .map(|p| p.name.clone())
}

/// A part of a reference between its punctuation.
struct Part {
    /// The part, after a place ("London: ").
    name: String,
    /// Whether a place stood before it ("London: Routledge").
    placed: bool,
    /// Whether a year follows it ("Routledge, 2013", "Routledge; 2013").
    dated: bool,
}

/// The parts of what follows a title, split at its punctuation.
fn parts_after_title(tail: &str) -> Vec<Part> {
    let pieces: Vec<&str> = tail
        .split(['.', ',', ';', '(', ')', '[', ']', '*', '"', '“', '”'])
        .map(str::trim)
        .collect();
    let year = |s: &str| {
        let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
        digits.len() == 4
    };
    pieces
        .iter()
        .enumerate()
        .filter(|(_, p)| p.chars().any(char::is_alphabetic))
        .map(|(i, p)| {
            let (placed, name) = match p.rsplit_once(':') {
                Some((place, name)) if !place.trim().is_empty() => (true, name.trim()),
                Some((_, name)) => (false, name.trim()),
                None => (false, *p),
            };
            let dated = pieces.get(i + 1).is_some_and(|next| year(next));
            Part {
                name: name.to_owned(),
                placed,
                dated,
            }
        })
        .filter(|p| !p.name.is_empty())
        .collect()
}
