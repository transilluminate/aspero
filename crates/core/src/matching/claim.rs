//! Reading a claim: what a citation asserts, its identifiers (DOIs, arXiv
//! IDs, BibTeX's fields), and what its free text says of its year,
//! journal, place, byline, venue, and whether it cites a chapter.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use super::same_work::same_work_id;
use super::text::{Normalised, content_words};
use crate::id::{IdKind, WorkId};
use crate::provider::Place;
use crate::publication::{self, first_page};

/// What a citation asserts. Any part may be missing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Claim {
    /// A free-text reference, in any style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Identifiers asserted for the work.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ids: Vec<WorkId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Names as written: "Ashish Vaswani", "Vaswani, A.", "A. Vaswani".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<i16>,
    /// The journal (or other container), full or abbreviated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<String>,
    /// The first page (or a range, of which the first page counts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    /// The pages as written: a range ("762-774") or a page. Given without
    /// `page`, its first page counts as the page; the claim as read takes
    /// it from free text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pages: Option<String>,
    /// Other readings of claimed identifiers (a DOI without the hyphen at
    /// a line break), to try only if the claimed one doesn't exist.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<Alternative>,
    /// The site the claim was read from (an article page's host:
    /// `jamanetwork.com`), as a hint of the publisher: never compared as a
    /// journal, nor reported as a difference. Without identifiers, its
    /// publisher's works are searched for the title, and preferred between
    /// candidates that match about as well ([`crate::site`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<String>,
}

/// Another reading of a claimed identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Alternative {
    pub claimed: WorkId,
    pub alternative: WorkId,
}

impl Alternative {
    pub fn new(claimed: WorkId, alternative: WorkId) -> Self {
        Alternative {
            claimed,
            alternative,
        }
    }
}

impl Claim {
    /// A claim from a free-text reference, with any DOIs and arXiv IDs it
    /// mentions extracted as asserted identifiers.
    pub fn from_text(text: impl Into<String>) -> Self {
        let text = text.into();
        // A DOI broken across lines may be meant otherwise (#39).
        let mut alternatives: Vec<Alternative> = Vec::new();
        for m in crate::id::scan(&text) {
            if let Some(other) = m.alternative(&text) {
                let a = Alternative::new(m.id, other);
                if !alternatives.contains(&a) {
                    alternatives.push(a);
                }
            }
        }
        let mut ids = find_ids(&text);
        for id in bibtex_ids(&text) {
            if !ids.iter().any(|f| same_work_id(f, &id)) {
                ids.push(id);
            }
        }
        if !ids.iter().any(|id| id.kind() == IdKind::Doi) {
            ids.extend(nber_paper(&text));
        }
        if ids.is_empty() {
            ids.extend(bare_arxiv(&text));
        }
        ids.extend(bare_pmid(&text, &ids));
        for id in bare_isbns(&text) {
            if !ids.iter().any(|f| same_work_id(f, &id)) {
                ids.push(id);
            }
        }
        Claim {
            ids,
            text: Some(text),
            alternatives,
            ..Claim::default()
        }
    }

    /// A claim from whatever a user typed: if the whole input is an
    /// identifier, a claim of just that identifier (an existence check);
    /// a Wikipedia citation template, RIS or EndNote record, its fields
    /// ([`crate::tagged`]); otherwise a free-text reference (see
    /// [`Claim::from_text`]).
    pub fn from_input(input: &str) -> Self {
        let input = input.trim();
        if let Some(claim) = crate::tagged::claim(input) {
            return claim;
        }
        let whole = || match crate::id::scan(input).as_slice() {
            // As a copy spaced it out: `https:// doi. org/ 10. 1183/ …`.
            [m] if m.span == (0..input.len()) => Some(m.id.clone()),
            _ => None,
        };
        match input.parse::<WorkId>().ok().or_else(whole) {
            Some(id) => Claim {
                ids: vec![id],
                ..Claim::default()
            },
            None => Claim::from_text(input),
        }
    }

    /// The volume and first page the claim cites: its fields, each one not
    /// given read from its text ("Injury 47 (2016) 797–804").
    pub(crate) fn locator(&self) -> publication::Locator {
        let page = self.page.as_ref().or(self.pages.as_ref());
        let given = publication::Locator {
            volume: self.volume.clone(),
            page: page.map(|p| first_page(p).to_owned()),
            pages: self.pages.clone().or_else(|| self.page.clone()),
        };
        match &self.text {
            Some(text) if given.volume.is_none() || given.page.is_none() => {
                let found = publication::locator(text);
                let pages = if given.page.is_some() {
                    given.pages
                } else {
                    found.pages
                };
                publication::Locator {
                    volume: given.volume.or(found.volume),
                    page: given.page.or(found.page),
                    pages,
                }
            }
            _ => given,
        }
    }

    /// Where the claim says the work appeared: volume and first page.
    pub fn place(&self) -> Option<Place> {
        let located = self.locator();
        Some(Place::new(located.volume?, located.page?))
    }

    /// The journal the claim cites: its `container`, else the journal its
    /// text names right before the volume and page, when that can be told
    /// with confidence ("… placenta. Environ Sci Technol. 2021;146:106274"
    /// names "Environ Sci Technol").
    pub fn cited_container(&self) -> Option<String> {
        self.container
            .clone()
            .or_else(|| publication::cited_journal(self.text.as_deref()?))
    }

    /// The journals the claim may cite: its `container`, else those its
    /// text may name where the title's end is unclear, one of which it
    /// does ([`publication::cited_journals`]).
    pub(super) fn cited_containers(&self) -> Vec<String> {
        match (&self.container, &self.text) {
            (Some(container), _) => vec![container.clone()],
            (None, Some(text)) => publication::cited_journals(text),
            (None, None) => Vec::new(),
        }
    }

    /// The claim as read: as given, with what its text says of the year
    /// (when it gives one), journal, volume and first page filled in where
    /// not given, and its title and authors as written where its layout
    /// tells them (Vancouver, APA, Harvard, a quoted title); nothing where
    /// it doesn't. The journal is read only where it can be told
    /// ([`Claim::cited_container`]). Matching doesn't use these readings:
    /// it finds a work's own title and authors in the text.
    pub fn as_read(&self) -> Claim {
        self.as_read_beside(None)
    }

    /// [`Claim::as_read`], with free text's title and authors read where
    /// `work`'s title stands in it (the work found for it): that stretch of
    /// text, and the names before it, as written ("Seymour CW", "Liu VX").
    /// Its layout's reading where the work's title isn't there.
    pub fn as_read_beside(&self, work: Option<&crate::work::Work>) -> Claim {
        let mut read = self.clone();
        if let Some(text) = &self.text
            && (read.title.is_none() || read.authors.is_empty())
        {
            let written = super::written::written(text, work);
            read.title = read.title.or(written.title);
            if read.authors.is_empty() {
                read.authors = written.authors;
            }
        }
        if read.year.is_none()
            && let [year] = dedup_years(self.cited_years())[..]
        {
            read.year = Some(year);
        }
        read.container = self.cited_container();
        let located = self.locator();
        read.volume = read.volume.or(located.volume);
        read.page = read.page.or(located.page);
        read.pages = located.pages;
        read
    }

    /// The years the claim cites: its year, else those in its text.
    pub fn cited_years(&self) -> Vec<i16> {
        self.cited_years_beside(None)
    }

    /// [`Claim::cited_years`], but for years inside `title` (a work's,
    /// compared with the claim) where the text gives it: "Siegel RL.
    /// Cancer statistics, 2020. 2018." cites 2018, not 2020.
    pub fn cited_years_beside(&self, title: Option<&str>) -> Vec<i16> {
        match (self.year, &self.text) {
            (Some(year), _) => vec![year],
            (None, Some(text)) => Normalised::new(text).years_beside(title),
            _ => Vec::new(),
        }
    }

    /// The DOI prefixes of the claim's site's publisher, when the claim
    /// names no identifier: what the site hints ([`crate::site`]). With
    /// identifiers, the site says nothing they don't.
    pub fn site_prefixes(&self) -> &'static [&'static str] {
        match &self.site {
            Some(site) if self.ids.is_empty() => crate::site::doi_prefixes(site),
            _ => &[],
        }
    }

    /// Nothing to search with: no identifiers, and no text, title or
    /// authors that aren't blank.
    pub fn is_empty(&self) -> bool {
        let blank = |s: &Option<String>| s.as_deref().is_none_or(|s| s.trim().is_empty());
        blank(&self.text)
            && self.ids.is_empty()
            && blank(&self.title)
            && self.authors.iter().all(|a| a.trim().is_empty())
    }

    /// A reference string for citation matchers: the text, or one composed
    /// from the fields.
    pub fn reference(&self) -> Option<String> {
        if let Some(text) = &self.text {
            return Some(text.clone());
        }
        // Vancouver-like: "Authors. Title. Journal. 2017;376:2545".
        let location = match (&self.volume, &self.page) {
            (Some(v), Some(p)) => Some(format!("{v}:{p}")),
            (Some(v), None) => Some(v.clone()),
            (None, p) => p.clone(),
        };
        let when = [self.year.map(|y| y.to_string()), location]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(";");
        let parts: Vec<String> = [
            (!self.authors.is_empty()).then(|| self.authors.join(", ")),
            self.title.clone(),
            self.container.clone(),
            (!when.is_empty()).then_some(when),
        ]
        .into_iter()
        .flatten()
        .collect();
        (!parts.is_empty()).then(|| parts.join(". "))
    }
}

/// Whether a reference cites a chapter in a book: "In: Jones K, ed. …",
/// "In K. Jones (Ed.), …", "In Handbook of Sepsis, edited by …".
pub(super) fn cites_chapter(text: &str) -> bool {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    tokens.iter().enumerate().any(|(i, t)| {
        *t == "In:"
            || (*t == "In"
                && tokens[i + 1..]
                    .iter()
                    .any(|t| t.starts_with("(Ed") || *t == "edited"))
    })
}

/// Whether a reference cites its work in an edited book: "In" (or "in"
/// after a comma or quote, Harvard's) and then its editors ("In: Jones K,
/// Smith P, editors. …", "in Jones, K. (eds) …", "In K. Jones (Ed.), …",
/// "edited by", "Hrsg."); an edition ("2nd ed.") is none.
pub(super) fn cites_edited_book(text: &str) -> bool {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let editors = |i: usize| {
        let t = tokens[i]
            .trim_matches(|c: char| !c.is_alphabetic())
            .to_lowercase();
        // "2nd ed.", "Revised ed.": an edition.
        let edition = i > 0 && {
            let before = tokens[i - 1].trim_end_matches(['.', ',']).to_lowercase();
            let ordinal = before.trim_start_matches(|c: char| c.is_ascii_digit());
            (ordinal.len() < before.len() && matches!(ordinal, "st" | "nd" | "rd" | "th" | "d"))
                || matches!(
                    before.as_str(),
                    "first" | "second" | "third" | "fourth" | "fifth" | "revised" | "rev" | "new"
                )
        };
        matches!(
            t.as_str(),
            "eds" | "editor" | "editors" | "edited" | "hrsg" | "éd" | "éds" | "coord"
        ) || (t == "ed" && !edition)
    };
    tokens.iter().enumerate().any(|(i, t)| {
        let opens = *t == "In:"
            || *t == "In"
            || (*t == "in" && i > 0 && tokens[i - 1].ends_with([',', '\'', '’', '"', '”']));
        opens && (i + 1..tokens.len().min(i + 16)).any(editors)
    })
}

/// A free-text reference's byline: a BibTeX entry's author field, else
/// its text before the record's title (at `span`). When the title is cited
/// otherwise, up to the first of the title's words or the first sentence's
/// end ("Rivers E, et al." before another title), whichever comes first.
pub(super) fn byline(
    text: &Normalised,
    title: Option<&str>,
    span: Option<&Range<usize>>,
) -> Option<String> {
    if let Some(authors) = publication::bibtex_field(&text.raw, "author") {
        return Some(authors);
    }
    let start = if let Some(span) = span {
        span.start
    } else {
        let title = content_words(title?);
        let titled = text.words.iter().position(|w| title.contains(w))?;
        // A word ending a sentence, not an initial ("RS.", "J.").
        let ended = (0..titled).find(|&i| {
            let raw = &text.raw[text.spans[i].clone()];
            let initial = raw.chars().count() <= 3 && raw.chars().all(char::is_uppercase);
            let after = &text.raw[text.spans[i].end..];
            !initial
                && (after.starts_with(". ") || after.starts_with(").") || after.starts_with("). "))
        });
        ended.map_or(titled, |i| i + 1)
    };
    let byline = &text.raw[..text.spans.get(start).map_or(text.raw.len(), |s| s.start)];
    Some(unlabelled(byline).to_owned())
}

/// The byline of a reference that gives no title ("Laughlin, Baskin, Agard
/// & Bertozzi. Science 2008, 320, 664"): its text up to the first
/// sentence's end (not an initial's), "et al.", the first word of the
/// `journal`'s (its significant words: "Am" of "J Am Coll Cardiol") or its
/// first number.
pub(super) fn untitled_byline(text: &Normalised, journal: Option<&[String]>) -> Option<String> {
    let end = (0..text.words.len()).find_map(|i| {
        let raw = &text.raw[text.spans[i].clone()];
        let journal_word = raw.chars().count() >= 2
            && raw.starts_with(char::is_uppercase)
            && journal.is_some_and(|j| publication::journal_word(&text.words[i], j));
        if raw.starts_with(|c: char| c.is_ascii_digit()) || journal_word {
            return Some(i);
        }
        if text.words[i] == "al" && i > 0 && text.words[i - 1] == "et" {
            return Some(i + 1);
        }
        let initial = raw.chars().count() <= 3 && raw.chars().all(char::is_uppercase);
        let after = &text.raw[text.spans[i].end..];
        (!initial && after.starts_with(". ")).then_some(i + 1)
    })?;
    let byline = &text.raw[..text.spans.get(end).map_or(text.raw.len(), |s| s.start)];
    Some(unlabelled(byline).to_owned())
}

/// A byline without the label a reference may open with, a trial's or a
/// guideline's ("EMPEROR-Reduced (empagliflozin, n=3,730): Packer M, …",
/// "HOPE-3: Yusuf S, …", "`BaSICS` (Finfer S et al. …", "Aktueller
/// Überblick: S. Pirandola et al. …"): see [`label_end`].
fn unlabelled(byline: &str) -> &str {
    &byline[label_end(byline, false)..]
}

/// Where a reference proper starts after a label it opens with (and a
/// list's number before it: "2. `BaSICS` (…"): a few words before a colon,
/// or one before a bracket, no comma among them outside brackets, and a
/// name with its initials after it ("`McMurray` JJV", "S. Pirandola"), not
/// a title's words ("NICE (2010) Delirium: prevention, …"). `acronym`
/// asks the label to start as a trial's or a model's name does, with two
/// capitals or a digit ("DAPA-HF", "`BaSICS`"), not as a title's words do
/// ("Deep learning: LeCun Y, …"). 0 without one.
fn label_end(text: &str, acronym: bool) -> usize {
    // A list's number: "2. ", "12) ".
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    let start = match text[digits..].strip_prefix(['.', ')']) {
        Some(rest) if (1..=3).contains(&digits) && rest.starts_with(' ') => {
            text.len() - rest.trim_start().len()
        }
        _ => 0,
    };
    let rest = &text[start..];
    // The label's end and where the names start: a colon's, else a
    // bracket's after one word.
    let (label, names) = match rest.find(": ") {
        Some(colon) => (&rest[..colon], colon + 2),
        None => match rest.find(" (") {
            Some(open) if !rest[..open].contains(char::is_whitespace) => (&rest[..open], open + 2),
            _ => return 0,
        },
    };
    let mut after = rest[names..].split_whitespace();
    let initials = |t: &str| {
        let t = t.trim_end_matches([',', '.', ';']);
        (1..=3).contains(&t.chars().filter(|c| *c != '.' && *c != '-').count())
            && t.chars().all(|c| c.is_uppercase() || c == '.' || c == '-')
    };
    let name = |t: &str| {
        t.starts_with(char::is_uppercase)
            && t.chars().filter(|c| c.is_alphabetic()).count() > 1
            && t.chars().any(char::is_lowercase)
    };
    let named = match (after.next(), after.next()) {
        (Some(first), Some(next)) => {
            (name(first) && initials(next))
                || (initials(first) && first.ends_with('.') && name(next))
        }
        _ => false,
    };
    if !named {
        return 0;
    }
    let mut depth = 0usize;
    let bare: String = label
        .chars()
        .filter(|&c| {
            match c {
                '(' | '[' => depth += 1,
                ')' | ']' => depth = depth.saturating_sub(1),
                _ => return depth == 0,
            }
            false
        })
        .collect();
    let words: Vec<&str> = bare.split_whitespace().collect();
    let shaped = words.first().is_some_and(|w| {
        w.chars().filter(|c| c.is_uppercase()).count() >= 2
            || w.contains(|c: char| c.is_ascii_digit())
    });
    if bare.contains(',') || words.is_empty() || words.len() > 4 || (acronym && !shaped) {
        return 0;
    }
    start + names
}

/// The stretch of a free-text reference where a title it gives stands:
/// after a label it opens with ([`label_end`], a trial's or a model's
/// name: "DAPA-HF: `McMurray` JJV, …"), and before what follows the volume
/// and page it cites, in its sentence or table's cell (a model's
/// commentary: "… Am J Sports Med 39(11):2296-303 | Cluster RCT in Danish
/// players. …"). All of it where it cites no place.
pub(super) fn title_region(text: &str) -> &str {
    let from = label_end(text, true);
    let text = &text[from..];
    let whole = publication::locator(text);
    let (Some(volume), Some(page)) = (&whole.volume, &whole.page) else {
        return text;
    };
    let stops = text
        .match_indices(" | ")
        .chain(text.match_indices(". "))
        .chain(text.match_indices('\n'))
        .map(|(at, _)| at);
    let mut stops: Vec<usize> = stops.collect();
    stops.sort_unstable();
    stops
        .into_iter()
        .find(|&at| {
            let before = publication::locator(&text[..at]);
            before.volume.as_ref() == Some(volume) && before.page.as_ref() == Some(page)
        })
        .map_or(text, |at| &text[..at])
}

/// The venue a claim cites, to tell a conference by: its container, a
/// BibTeX entry's `booktitle` (else `journal`), or a reference's text
/// after the work's title (at `span`; all of it when the title isn't found
/// there), its links aside ("10.1109/cvpr.2017.632" names no venue cited).
pub(super) fn cited_venue(
    claim: &Claim,
    text: Option<&Normalised>,
    span: Option<&Range<usize>>,
) -> Option<String> {
    if let Some(container) = &claim.container {
        return Some(container.clone());
    }
    let text = text?;
    if text.raw.trim_start().starts_with('@') {
        return publication::bibtex_field(&text.raw, "booktitle")
            .or_else(|| publication::bibtex_field(&text.raw, "journal"));
    }
    let after = span
        .and_then(|s| text.spans.get(s.end.checked_sub(1)?))
        .map_or(0, |w| w.end);
    let venue: Vec<&str> = text.raw[after..]
        .split_whitespace()
        .filter(|t| {
            let t = t.trim_start_matches(['(', '[', '<']).to_ascii_lowercase();
            !(["doi", "http", "www.", "arxiv", "10."]
                .iter()
                .any(|p| t.starts_with(p)))
        })
        .collect();
    Some(venue.join(" "))
}

fn dedup_years(mut years: Vec<i16>) -> Vec<i16> {
    years.sort_unstable();
    years.dedup();
    years
}

/// Identifiers a BibTeX entry pasted as a reference gives in fields that
/// name them bare: its arXiv `eprint` (`eprint = {1710.01128},
/// archivePrefix = {arXiv}`) and `pmid`.
fn bibtex_ids(text: &str) -> Vec<WorkId> {
    let field = |name: &str| publication::bibtex_field(text, name);
    let mut ids = Vec::new();
    let archive = field("archiveprefix").or_else(|| field("eprinttype"));
    if let Some(eprint) = field("eprint")
        && archive
            .as_deref()
            .is_none_or(|a| a.eq_ignore_ascii_case("arxiv"))
    {
        ids.extend(format!("arxiv:{eprint}").parse::<WorkId>());
    }
    if let Some(pmid) = field("pmid") {
        ids.extend(format!("pmid:{pmid}").parse::<WorkId>());
    }
    ids
}

/// The arXiv number a reference gives bare as its identifier, as models
/// write them: the only number of that shape in it, after a colon
/// ("`NegotiationToM`: 2404.13627"), as a table's cell ("… | 1706.03741 |
/// NeurIPS 2017") or last ("… (2210.13312)"). [`crate::id::scan`] reads
/// bare numbers in prose as none; a reference holding one there and no
/// other identifier labels it by where it stands. Not after a colon
/// without a space ("2016;533:1706.03762" is a locator), nor alone.
fn bare_arxiv(text: &str) -> Option<WorkId> {
    let bytes = text.as_bytes();
    let digits = |from: usize| {
        bytes[from.min(bytes.len())..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };
    let mut shaped: Vec<std::ops::Range<usize>> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let run = digits(i);
        if run == 0 {
            i += 1;
            continue;
        }
        let edge =
            i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || b"./".contains(&bytes[i - 1]));
        let tail = if run == 4 && bytes.get(i + 4) == Some(&b'.') {
            digits(i + 5)
        } else {
            0
        };
        if edge && (4..=5).contains(&tail) {
            let mut end = i + 5 + tail;
            if bytes.get(end) == Some(&b'v') && digits(end + 1) > 0 {
                end += 1 + digits(end + 1);
            }
            // Not run on into letters, a path or more digits after a dot.
            let run_on = bytes
                .get(end)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'/')
                || (bytes.get(end) == Some(&b'.')
                    && bytes.get(end + 1).is_some_and(u8::is_ascii_digit));
            if !run_on {
                shaped.push(i..end);
            }
            i = end;
        } else {
            i += run;
        }
    }
    let [span] = &shaped[..] else {
        return None;
    };
    let before = &text[..span.start];
    let after = &text[span.end..];
    let spaced =
        before.is_empty() || before.ends_with(char::is_whitespace) || before.ends_with(['(', '[']);
    let last = after.chars().all(|c| !c.is_alphanumeric());
    let colon = before.trim_end().ends_with(':') && before.ends_with(char::is_whitespace);
    let cell = before.trim_end().ends_with('|')
        && (after.trim_start().starts_with('|') || after.trim().is_empty());
    // A number alone is no reference ("1706.03762" typed is ambiguous).
    let referenced = before.chars().filter(|c| c.is_alphabetic()).count() >= 3;
    if !(referenced && spaced && (last || colon || cell)) {
        return None;
    }
    format!("arxiv:{}", &text[span.clone()]).parse().ok()
}

/// The PMID a Markdown table's row gives bare in a cell of its own, beside
/// a DOI, as models write them ("… JAMA Cardiol. 2016;1(4):397-404. |
/// 10.1001/jamacardio.2016.0695 | 27438392 (verify)"): the only cell of 7
/// or 8 digits, alone or with a bracketed note. [`crate::id::scan`] reads
/// bare numbers as none; a row citing a DOI and no PMID labels this one by
/// where it stands. Not in prose, nor as a cell's part ("2016;1(4)"), nor
/// in a row without a DOI.
fn bare_pmid(text: &str, ids: &[WorkId]) -> Option<WorkId> {
    let has = |kind| ids.iter().any(|id| id.kind() == kind);
    if !has(IdKind::Doi) || has(IdKind::Pmid) {
        return None;
    }
    let numbered: Vec<&str> = text
        .split('|')
        .skip(1)
        .filter_map(|cell| {
            let cell = cell.trim();
            let digits = cell
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(cell.len());
            let (number, note) = cell.split_at(digits);
            let note = note.trim();
            let noted = note.is_empty()
                || (note.starts_with('(')
                    && note.ends_with(')')
                    && note[1..note.len() - 1]
                        .chars()
                        .all(|c| c.is_alphabetic() || c == ' ' || c == '?'));
            ((7..=8).contains(&number.len()) && !number.starts_with('0') && noted).then_some(number)
        })
        .collect();
    let [pmid] = numbered[..] else {
        return None;
    };
    format!("pmid:{pmid}").parse().ok()
}

/// The DOI of the NBER working paper a reference gives by its number, as
/// economics cites them ("NBER Working Paper w31113", "NBER Working Paper
/// No. 16449", "NBER WP 9804", "Working Paper 24871 … National Bureau of
/// Economic Research"): NBER registers each as `10.3386/w<number>`, four
/// digits at least. Only with NBER named beside it (a working paper's
/// number is any series' otherwise), and read only when the reference
/// gives no DOI (one given names the version it cites).
fn nber_paper(text: &str) -> Option<WorkId> {
    let tokens: Vec<String> = text
        .split_whitespace()
        .map(|t| {
            t.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .collect();
    let t = |i: usize| tokens.get(i).map_or("", String::as_str);
    let nber_at = |i: usize| {
        t(i) == "nber"
            || (i >= 4
                && (i - 4..=i)
                    .map(t)
                    .eq(["national", "bureau", "of", "economic", "research"]))
    };
    for (i, token) in tokens.iter().enumerate() {
        let digits = token.strip_prefix('w').unwrap_or(token);
        if !(1..=6).contains(&digits.len()) || !digits.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        // Back past "No." or "#" to the series: "Working Paper (Series)",
        // "Working Papers" or "WP".
        let mut at = i;
        if at > 0 && matches!(t(at - 1), "no" | "number" | "nr" | "") {
            at -= 1;
        }
        if at > 0 && t(at - 1) == "series" {
            at -= 1;
        }
        let series = if at > 0 && t(at - 1) == "wp" {
            at - 1
        } else if at > 1 && matches!(t(at - 1), "paper" | "papers") && t(at - 2) == "working" {
            at - 2
        } else {
            continue;
        };
        // NBER right before the series, or the bureau named after it
        // ("Working Paper No. 24871. Cambridge, MA: National Bureau …").
        let named = (series > 0 && nber_at(series - 1)) || (i + 1..i + 12).any(nber_at);
        let number: u32 = digits.parse().ok()?;
        if named && number > 0 {
            return format!("doi:10.3386/w{number:04}").parse().ok();
        }
    }
    None
}

/// The ISBN-13s a reference gives bare, as models write them in a table's
/// cell ("… | 15th (2020) | 978-1260452310") or after a note ("ISBN-13
/// (Dover): 978-0486445687"): a word of its own of 13 digits, hyphens
/// allowed, with a book prefix (978 or 979) and a valid check digit.
/// [`crate::id::scan`] reads ISBNs only when labelled, and its mentions
/// are unchanged; in a reference this shape is a book's number (a phone
/// number has fewer digits, and another number that starts 978 or 979
/// passes the check one time in ten). Not an ISBN-10, which looks like
/// other numbers, nor digits inside a DOI or link
/// ("10.1017/cbo9781107049963").
fn bare_isbns(text: &str) -> Vec<WorkId> {
    let hyphen = |c: char| matches!(c, '-' | '\u{2010}' | '\u{2011}');
    let mut found: Vec<WorkId> = Vec::new();
    for word in text.split(|c: char| c.is_whitespace() || c == '|') {
        let word = word.trim_matches(|c: char| "()[]{}.,;:\"'“”‘’*_".contains(c));
        let digits = word.chars().filter(char::is_ascii_digit).count();
        let shaped = digits == 13
            && word.chars().all(|c| c.is_ascii_digit() || hyphen(c))
            && word.starts_with(|c: char| c.is_ascii_digit())
            && word.ends_with(|c: char| c.is_ascii_digit())
            && !word.contains("--");
        if shaped
            && let Ok(id) = WorkId::parse_as(IdKind::Isbn, word)
            && !found.contains(&id)
        {
            found.push(id);
        }
    }
    found
}

/// The distinct identifiers mentioned in free text (see [`crate::id::scan`]).
pub fn find_ids(text: &str) -> Vec<WorkId> {
    let mut found: Vec<WorkId> = Vec::new();
    for mention in crate::id::scan(text) {
        if !found.iter().any(|f| same_work_id(f, &mention.id)) {
            found.push(mention.id);
        }
    }
    found
}
