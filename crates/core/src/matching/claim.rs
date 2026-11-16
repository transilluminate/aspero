//! Reading a claim: what a citation asserts, its identifiers (DOIs, arXiv
//! IDs, BibTeX's fields), and what its free text says of its year,
//! journal, place, byline, venue, and whether it cites a chapter.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use super::same_work::same_work_id;
use super::text::{Normalised, content_words};
use crate::id::WorkId;
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
        let given = publication::Locator {
            volume: self.volume.clone(),
            page: self.page.as_deref().map(|p| first_page(p).to_owned()),
        };
        match &self.text {
            Some(text) if given.volume.is_none() || given.page.is_none() => {
                let found = publication::locator(text);
                publication::Locator {
                    volume: given.volume.or(found.volume),
                    page: given.page.or(found.page),
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
    /// not given. Free text's title and authors aren't parsed out: they're
    /// scored by finding a work's own in the text; the journal is read only
    /// where it can be told ([`Claim::cited_container`]).
    pub fn as_read(&self) -> Claim {
        let mut read = self.clone();
        if read.year.is_none()
            && let [year] = dedup_years(self.cited_years())[..]
        {
            read.year = Some(year);
        }
        read.container = self.cited_container();
        let located = self.locator();
        read.volume = read.volume.or(located.volume);
        read.page = read.page.or(located.page);
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

/// A byline without the label a reference may open with, a trial's or a
/// guideline's ("EMPEROR-Reduced (empagliflozin, n=3,730): Packer M, …",
/// "HOPE-3: Yusuf S, …"): a few words before a colon, no comma among
/// them outside brackets, and a name with its initials after it (not a
/// title's words: "NICE (2010) Delirium: prevention, …").
fn unlabelled(byline: &str) -> &str {
    let Some(colon) = byline.find(": ") else {
        return byline;
    };
    let mut after = byline[colon + 2..].split_whitespace();
    let initials = |t: &str| {
        let t = t.trim_end_matches([',', '.', ';']);
        (1..=3).contains(&t.chars().count())
            && t.chars().all(|c| c.is_uppercase() || c == '.' || c == '-')
    };
    let named = match (after.next(), after.next()) {
        (Some(name), Some(next)) => {
            name.starts_with(char::is_uppercase) && name.chars().count() > 1 && initials(next)
        }
        _ => false,
    };
    if !named {
        return byline;
    }
    let label = &byline[..colon];
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
    if bare.contains(',') || bare.split_whitespace().count() > 4 {
        return byline;
    }
    &byline[colon + 2..]
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
