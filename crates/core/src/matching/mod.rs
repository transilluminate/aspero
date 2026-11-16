//! Does a record match what a citation claims? Pure scoring, no I/O.
//!
//! A [`Claim`] is what someone (often an agent) asserted: a free-text
//! reference, structured fields, identifiers, or a mix. [`assess`] compares
//! one candidate record against it, field by field, and combines the fields
//! into a confidence in `0.0..=1.0`.
//!
//! Lessons from real data shape the rules (see DESIGN "Provider notes and
//! gotchas"):
//! - **Lookalikes exist**: re-deposits with the real title, authors and
//!   abstract under a different year and publisher. A wrong year is a strong
//!   penalty, never a detail.
//! - **One field is not proof**: a title alone ("Deep learning") matches
//!   many works, so confidence is capped unless at least two fields agree.
//! - Free-text references are not parsed into fields. Instead the record's
//!   fields are looked for *in* the text: its title as a phrase, its authors'
//!   family names, its year. This is robust to citation styles.

mod authors;
mod derivative;
mod text;

use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::id::{IdKind, WorkId, starts_with_ci};
use crate::provider::Place;
use crate::publication::{self, first_page, journal_in, other_journal, same_journal};
use crate::record::SourceRecord;
use crate::work::WorkType;

use authors::{
    author_overlap, byline_strangers, claimed_strangers, complete_byline, family_name, name_words,
    same_family,
};
pub use derivative::{
    WorkKind, claim_mentions_derivative, derivative_kind, derivative_work_kind, is_derivative,
};
use text::{Normalised, content_words, words};
pub use text::{
    normalise, search_terms, shared_title, title_similarity, unmarked, whole_title_similarity,
};

/// Field weights in the confidence (before penalties).
const W_TITLE: f64 = 0.5;
const W_AUTHORS: f64 = 0.25;
const W_YEAR: f64 = 0.25;

/// Multiplier when the year is off by two or more.
const YEAR_MISMATCH_PENALTY: f64 = 0.5;
/// Multiplier when the title clearly differs.
const TITLE_MISMATCH_PENALTY: f64 = 0.5;
/// Multiplier when the authors clearly differ.
const AUTHORS_MISMATCH_PENALTY: f64 = 0.5;
/// Confidence ceiling when fewer than two fields could be compared.
const SINGLE_FIELD_CAP: f64 = 0.7;
/// Confidence ceiling without corroborating authors or identifiers.
const UNCORROBORATED_CAP: f64 = 0.8;
/// Author score that corroborates a match: the first author present
/// (0.75 for structured claims, 0.9 in free text).
const CORROBORATING_AUTHORS: f64 = 0.75;
/// Confidence when the journal, volume and first page all agree and
/// nothing contradicts them: verified.
const PLACED: f64 = 0.9;
/// The same with the year off by one (online first, then in print):
/// plausible.
const PLACED_YEAR_OFF: f64 = 0.8;
/// Confidence ceiling, below plausible, without evidence of which work is
/// cited: only a year or journal agreeing, or a short title found inside
/// free text.
const NO_EVIDENCE_CAP: f64 = 0.5;
/// Another edition of a cited book: plausible, with the year a difference.
const EDITION: f64 = PLAUSIBLE;
/// A dataset's or software's version, its identifier claimed, cited with
/// another year: plausible, with the year a difference.
const VERSION: f64 = PLAUSIBLE;
/// Content words below which a title found inside free text is short: one
/// or two words turn up in any text ("Aspero", "Deep learning").
const SHORT_TITLE_WORDS: usize = 3;
/// Multiplier for replies, corrections, errata and commentaries.
const DERIVATIVE_PENALTY: f64 = 0.5;
/// Confidence ceiling when the claimed volume or first page differs.
const LOCATION_MISMATCH_CAP: f64 = 0.8;
/// Multiplier when the claim names a journal clearly another
/// ([`other_journal`]): lowers the ranking.
const JOURNAL_MISMATCH_PENALTY: f64 = 0.9;
/// Confidence ceiling when a journal article is cited in a journal that is
/// clearly another ([`other_journal`]): a citation error, as a wrong volume
/// is, and a tell of invented references.
const JOURNAL_MISMATCH_CAP: f64 = LOCATION_MISMATCH_CAP;
/// Confidence ceiling when the claim names an author the record's complete
/// byline lacks: invented co-authors beside a real first author.
const STRANGERS_CAP: f64 = LOCATION_MISMATCH_CAP;
/// Title score, partial, when the claim's title is the record's with words
/// added ([`text::extends`]).
const EXTENDED_TITLE: f64 = 0.75;
/// Confidence ceiling then: the claim may cite another work whose title
/// contains this one's, or this one with a subtitle its record lacks.
const EXTENDED_TITLE_CAP: f64 = LOCATION_MISMATCH_CAP;
/// Confidence ceiling for a work with no journal, volume or page when the
/// claim cites a journal: a lookalike copy can't confirm it.
const UNPLACED_CAP: f64 = LOCATION_MISMATCH_CAP;

/// Verdict thresholds on confidence.
pub const VERIFIED: f64 = 0.85;
pub const PLAUSIBLE: f64 = 0.6;

/// Authors compared from the start of a byline.
const AUTHORS_COMPARED: usize = 3;

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
        Claim {
            ids: find_ids(&text),
            text: Some(text),
            alternatives,
            ..Claim::default()
        }
    }

    /// A claim from whatever a user typed: if the whole input is an
    /// identifier, a claim of just that identifier (an existence check);
    /// otherwise a free-text reference (see [`Claim::from_text`]).
    pub fn from_input(input: &str) -> Self {
        let input = input.trim();
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Agreement {
    Match,
    Partial,
    Mismatch,
    /// The claim or the record doesn't say.
    Unknown,
}

/// One field's comparison: a score in `0.0..=1.0` when comparable.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FieldCheck {
    pub agreement: Agreement,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
}

impl FieldCheck {
    const UNKNOWN: FieldCheck = FieldCheck {
        agreement: Agreement::Unknown,
        score: None,
    };

    fn exact(agrees: bool) -> Self {
        FieldCheck::scored(if agrees { 1.0 } else { 0.0 })
    }

    fn scored(score: f64) -> Self {
        let agreement = match score {
            s if s >= 0.9 => Agreement::Match,
            s if s >= 0.5 => Agreement::Partial,
            _ => Agreement::Mismatch,
        };
        FieldCheck {
            agreement,
            score: Some(score),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Assessment {
    pub confidence: f64,
    pub title: FieldCheck,
    pub authors: FieldCheck,
    pub year: FieldCheck,
    /// The journal: `Match` when the claim names it (or an abbreviation),
    /// `Mismatch` when a structured claim names one clearly another, or a
    /// journal article's reference cites one
    /// ([`Claim::cited_container`], [`other_journal`]).
    pub container: FieldCheck,
    pub volume: FieldCheck,
    /// The first page.
    pub page: FieldCheck,
    /// Whether the claim's identifiers are the record's: `Match` if any
    /// claimed ID is the record's, `Mismatch` if the record has a different
    /// ID of the same kind, else `Unknown`.
    pub ids: Agreement,
}

/// A title that is a URL.
fn is_url(title: &str) -> bool {
    let t = title.trim();
    ["http://", "https://", "www."]
        .iter()
        .any(|p| starts_with_ci(t, p))
        || t.contains("://")
}

/// Whether a reference cites a chapter in a book: "In: Jones K, ed. …",
/// "In K. Jones (Ed.), …", "In Handbook of Sepsis, edited by …".
fn cites_chapter(text: &str) -> bool {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    tokens.iter().enumerate().any(|(i, t)| {
        *t == "In:"
            || (*t == "In"
                && tokens[i + 1..]
                    .iter()
                    .any(|t| t.starts_with("(Ed") || *t == "edited"))
    })
}

/// A free-text reference's byline: a BibTeX entry's author field, else
/// its text before the record's title (at `span`). When the title is cited
/// otherwise, up to the first of the title's words or the first sentence's
/// end ("Rivers E, et al." before another title), whichever comes first.
fn byline(text: &Normalised, title: Option<&str>, span: Option<&Range<usize>>) -> Option<String> {
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
    Some(text.raw[..text.spans.get(start).map_or(text.raw.len(), |s| s.start)].to_owned())
}

fn dedup_years(mut years: Vec<i16>) -> Vec<i16> {
    years.sort_unstable();
    years.dedup();
    years
}

/// Words naming a preprint server, or a preprint.
const PREPRINT_WORDS: &[&str] = &[
    "arxiv",
    "biorxiv",
    "medrxiv",
    "chemrxiv",
    "psyarxiv",
    "socarxiv",
    "techrxiv",
    "ssrn",
    "preprint",
    "preprints",
];

/// Whether a venue is a preprint server's: "arXiv (Cornell University)",
/// "medRxiv", "Research Square", "SSRN Electronic Journal".
fn preprint_server(name: &str) -> bool {
    let words = words(name);
    words.iter().any(|w| PREPRINT_WORDS.contains(&w.as_str()))
        || words.windows(2).any(|w| w == ["research", "square"])
}

/// Scores how well `record` matches `claim`.
pub fn assess(claim: &Claim, record: &SourceRecord) -> Assessment {
    let work = &record.work;
    let text = claim.text.as_deref().map(Normalised::new);
    // A placeholder record's "title" may be a URL (a registration agency's
    // notice page): no title at all.
    let work_title = work.title.clone().filter(|t| !is_url(t));

    let families: Vec<String> = work.authors.iter().filter_map(family_name).collect();
    // A reference that gives no title ("Wittermans E, et al. ERJ Open Res.
    // 2022") doesn't give a different one.
    let untitled = claim.title.is_none()
        && text
            .as_ref()
            .is_some_and(|t| t.untitled(&families, work.container_title.as_deref()));

    let title = match (&claim.title, &work_title, &text) {
        (Some(claimed), Some(actual), _) => FieldCheck::scored(title_similarity(claimed, actual)),
        (None, Some(actual), Some(text)) if !untitled => {
            FieldCheck::scored(text.title_coverage(actual))
        }
        _ => FieldCheck::UNKNOWN,
    };

    let authors = if families.is_empty() {
        FieldCheck::UNKNOWN
    } else if !claim.authors.is_empty() {
        FieldCheck::scored(author_overlap(&claim.authors, &families))
    } else if let Some(text) = &text {
        // A reference that names no one ("Injury 47 (2016) 797–804", a
        // pasted title) doesn't name others.
        match text.author_coverage(&families) {
            0.0 if !claim.text.as_deref().is_some_and(authors::names_given) => FieldCheck::UNKNOWN,
            coverage => FieldCheck::scored(coverage),
        }
    } else {
        FieldCheck::UNKNOWN
    };
    // Where the record's title stands in free text: what precedes it is the
    // byline, what follows the journal and place.
    let title_span = match (&claim.title, &work_title, &text) {
        (None, Some(actual), Some(text)) => text.title_span(actual),
        _ => None,
    };
    // Names the claim gives that the record's byline lacks: invented
    // co-authors beside a real first author. Not a match, whoever is first.
    // Not for books, whose records often list only the first author (Open
    // Library's Sutton without Barto).
    let strangers = authors.agreement != Agreement::Unknown
        && work.work_type != Some(crate::work::WorkType::Book)
        && complete_byline(&work.authors)
        && if claim.authors.is_empty() {
            text.as_ref()
                .and_then(|text| byline(text, work_title.as_deref(), title_span.as_ref()))
                .is_some_and(|b| {
                    byline_strangers(&b, &work.authors, work.container_title.as_deref())
                })
        } else {
            claimed_strangers(&claim.authors, &work.authors)
        };
    let authors = match authors.score {
        Some(s) if strangers => FieldCheck::scored(s.min(CORROBORATING_AUTHORS)),
        _ => authors,
    };
    // The record's title with words added: another work's title that
    // contains this one's ("Deep learning for medical image analysis"
    // isn't "Deep learning"), or a subtitle the record lacks. Partly the
    // same, never the same. Books' records often leave their subtitles out
    // ("Imagined Communities", cited with "Reflections on the Origin …"):
    // there, a subtitle after the whole title is no more words.
    let subtitle_ok = work.work_type == Some(crate::work::WorkType::Book)
        && work_title
            .as_deref()
            .is_some_and(|t| text::main_title(unmarked(t)) == unmarked(t));
    let extended = match (&claim.title, &work_title, &text, &title_span) {
        (Some(claimed), Some(actual), _, _) => text::extends(claimed, actual, subtitle_ok),
        (None, Some(_), Some(text), Some(span)) => {
            let journal = work
                .container_title
                .as_deref()
                .map(publication::significant);
            let names = name_words(&work.authors);
            text.extends_title(span, &names, journal.as_deref(), subtitle_ok)
        }
        _ => false,
    };
    let title = match title.score {
        Some(s) if extended => FieldCheck::scored(s.min(EXTENDED_TITLE)),
        _ => title,
    };

    let actual_years: Vec<i16> = work
        .issued
        .map(|d| d.year())
        .into_iter()
        .chain(work.other_years.iter().copied())
        .collect();
    // A year inside the title ("Cancer statistics, 2020") is not the year
    // cited.
    let claimed_years = claim.cited_years_beside(work_title.as_deref());
    let year = if actual_years.is_empty() || claimed_years.is_empty() {
        FieldCheck::UNKNOWN
    } else {
        // Nearest pairing: any edition's year may be the one cited.
        let delta = claimed_years
            .iter()
            .flat_map(|c| actual_years.iter().map(move |a| c.abs_diff(*a)))
            .min()
            .unwrap_or(u16::MAX);
        // Halves per year off: off by one is common (online-first vs
        // print, preprint vs published) and counts as partial; two or
        // more is a mismatch, but a nearer year still ranks higher.
        FieldCheck::scored(0.5f64.powi(i32::from(delta.min(30))))
    };

    // Only a journal article's journal is held against a citation: books'
    // series, proceedings and repositories go by many names.
    let article = work.work_type == Some(crate::work::WorkType::JournalArticle);
    // A journal read from text that is the work's own title, in title case
    // ("… Hooper D. Hot Gravitons From Kerr Black Holes. 2020."), is none.
    let title_words = work_title.as_deref().map(words).unwrap_or_default();
    let cited_journal = claim.cited_container().filter(|cited| {
        claim.container.is_some()
            || !publication::significant(cited)
                .iter()
                .all(|w| title_words.contains(w))
    });
    let cited_place = claim.locator();
    // A reference giving a journal, volume and page cites the published
    // article; a preprint of it (in no journal, at no volume) is another
    // version, not the one cited.
    // So is one cited in a journal by name alone ("Science Robotics.
    // 2024."), unless its identifier is the one claimed.
    let preprint = work.work_type == Some(WorkType::Preprint)
        || work.container_title.as_deref().is_some_and(preprint_server);
    let preprint_of_cited = preprint
        && work.volume.is_none()
        && cited_journal
            .as_deref()
            .is_some_and(|j| !preprint_server(j))
        && ((cited_place.volume.is_some() && cited_place.page.is_some())
            || id_agreement(&claim.ids, &record.ids) != Agreement::Match);
    // A journal's article cited as a preprint ("medRxiv preprint, 2019"),
    // without an identifier to say which version: another version.
    let article_as_preprint = article
        && !preprint
        && claim.ids.is_empty()
        && claim.container.is_none()
        && text
            .as_ref()
            .is_some_and(|t| t.words.iter().any(|w| PREPRINT_WORDS.contains(&w.as_str())));
    // The journal the claim names, or reads where its reference gives one,
    // decides, not words elsewhere in a reference (an author's initials
    // "SR" for Scientific Reports). Names compare by rule, which misses
    // some pairs, so only one clearly another is a mismatch: any work's
    // when claimed, a journal article's when read from text (where a
    // book's publisher may stand).
    let container = match (cited_journal.as_deref(), &work.container_title, &text) {
        (Some(cited), Some(actual), _)
            if same_journal(cited, actual) || journal_in(&words(cited), actual) =>
        {
            FieldCheck::exact(true)
        }
        (Some(cited), Some(actual), _)
            if (article || claim.container.is_some()) && other_journal(cited, actual) =>
        {
            FieldCheck::exact(false)
        }
        (None, Some(actual), Some(text)) if journal_in(&text.words, actual) => {
            FieldCheck::exact(true)
        }
        _ => FieldCheck::UNKNOWN,
    };
    let container =
        if (preprint_of_cited || article_as_preprint) && container.agreement != Agreement::Match {
            FieldCheck::exact(false)
        } else {
            container
        };
    // A journal article cited in a journal clearly not its own, or a
    // preprint cited as a journal's article.
    let wrong_journal =
        container.agreement == Agreement::Mismatch && (preprint_of_cited || article);
    let located = &cited_place;
    let same = |claimed: Option<&str>, actual: Option<&str>| match (claimed, actual) {
        (Some(c), Some(a)) if !c.is_empty() && !a.is_empty() => {
            FieldCheck::exact(c.trim().eq_ignore_ascii_case(a.trim()))
        }
        _ => FieldCheck::UNKNOWN,
    };
    let volume = same(located.volume.as_deref(), work.volume.as_deref());
    let page = same(
        located.page.as_deref(),
        work.page.as_deref().map(first_page),
    );

    // Citing the exact place: volume and first page.
    let located = volume.agreement == Agreement::Match && page.agreement == Agreement::Match;
    // Short Vancouver references often leave the title out ("Daum RS et
    // al. N Engl J Med 2017;376:2545"); with the journal and volume
    // agreeing, a title missing from the text is not a different one (the
    // page then decides between verified and a wrong page).
    // Unless it gives one: a few words that are none of the work's authors,
    // journal, numbers or furniture ("Niwa T. Transfection of plasmid DNA
    // by nanocarriers. J Microbiol Methods. 2005;61:251" isn't titleless).
    let placed = container.agreement == Agreement::Match && volume.agreement == Agreement::Match;
    let titled = text.as_ref().is_some_and(|t| {
        t.title_run(&families, work.container_title.as_deref()) >= SHORT_TITLE_WORDS
    });
    let title =
        if placed && claim.title.is_none() && title.agreement == Agreement::Mismatch && !titled {
            FieldCheck::UNKNOWN
        } else {
            title
        };

    let ids = id_agreement(&claim.ids, &record.ids);
    // A claim of identifiers alone is fully confirmed by a matching ID.
    let id_only = [title, authors, year].iter().all(|c| c.score.is_none());
    let mut confidence = if id_only && ids == Agreement::Match {
        1.0
    } else {
        combine(title, authors, year)
    };
    // Title and year alone can't verify: a short or generic title ("Health
    // technology assessment") turns up inside many references, and letters
    // and corrections often carry the original's title without authors.
    // Corroboration means the first author matches, or a claimed ID does.
    // So does citing the exact place.
    let corroborated = authors.score.is_some_and(|s| s >= CORROBORATING_AUTHORS)
        || ids == Agreement::Match
        || located;
    if !id_only && !corroborated {
        confidence = confidence.min(UNCORROBORATED_CAP);
    }
    // A title of a word or two inside free text ("Aspero" in "aspero 0.6.5
    // is in") is no evidence by itself, nor with a year: it takes the
    // authors, the journal, the place or an identifier.
    let short_title_in_text = claim.title.is_none()
        && text.is_some()
        && work_title
            .as_deref()
            .is_some_and(|t| content_words(t).len() < SHORT_TITLE_WORDS);
    if short_title_in_text && !corroborated && container.agreement != Agreement::Match {
        confidence = confidence.min(NO_EVIDENCE_CAP);
    }
    // A year, or a year and journal, identify nothing: some work matches
    // them by chance. It takes the title, authors, an identifier or the
    // place.
    let identified =
        matches!(title.agreement, Agreement::Match | Agreement::Partial) || corroborated;
    if !id_only && !identified {
        confidence = confidence.min(NO_EVIDENCE_CAP);
    }
    // Without a title, authors, a year and a journal fit many works: it
    // takes an identifier, the place, or the journal, volume and authors.
    let pinned = ids == Agreement::Match
        || located
        || (placed && authors.score.is_some_and(|s| s >= CORROBORATING_AUTHORS));
    if untitled && !pinned {
        confidence = confidence.min(NO_EVIDENCE_CAP);
    }
    // A title that only partly agrees, with nothing else agreeing, is a
    // few shared words: "Journal of the American College of Surgeons"
    // inside a reference whose author is the College.
    let agrees = |c: &FieldCheck| matches!(c.agreement, Agreement::Match | Agreement::Partial);
    if title.agreement == Agreement::Partial
        && ![authors, year, container].iter().any(agrees)
        && ids != Agreement::Match
        && !located
    {
        confidence = confidence.min(NO_EVIDENCE_CAP);
    }
    // Journal, volume and first page together name one work, title or no
    // title ("Injury 47 (2016) 797–804"), unless something contradicts
    // it.
    // Another title at this work's place, by its authors: a title
    // translated or paraphrased, or one work's title cited at another's
    // place. Plausible, its title to check; never verified.
    if located
        && container.agreement == Agreement::Match
        && title.agreement == Agreement::Mismatch
        && authors.agreement == Agreement::Match
        && year.agreement != Agreement::Mismatch
    {
        confidence = confidence.max(PLAUSIBLE);
    }
    if located
        && container.agreement == Agreement::Match
        && title.agreement != Agreement::Mismatch
        && authors.agreement != Agreement::Mismatch
    {
        let floor = match year.agreement {
            Agreement::Match | Agreement::Unknown => PLACED,
            Agreement::Partial => PLACED_YEAR_OFF,
            Agreement::Mismatch => 0.0,
        };
        confidence = confidence.max(floor);
    }
    // A wrong volume or page is an error in the citation, and tells an
    // original from the letters and corrections in the same journal.
    if [volume, page]
        .iter()
        .any(|c| c.agreement == Agreement::Mismatch)
    {
        confidence = confidence.min(LOCATION_MISMATCH_CAP);
    }
    // Another journal lowers the ranking; for a journal article (the wrong
    // journal, a common error in real and invented references) it is an
    // error in the citation, as a wrong volume is.
    if container.agreement == Agreement::Mismatch {
        confidence *= JOURNAL_MISMATCH_PENALTY;
    }
    if wrong_journal {
        confidence = confidence.min(JOURNAL_MISMATCH_CAP);
    }
    // A copy deposited elsewhere (Zenodo, a repository, a technical
    // report) has the work's title, authors and year but no journal,
    // volume or page: it can't confirm the journal cited, and it isn't the
    // article cited there, unless the claim's identifier says it is.
    // Preprints, books, chapters, data and software have none to give,
    // but nor do they confirm a journal, volume and page all cited
    // (PsycEXTRA's record of a paper Science printed).
    let unconfirmable = cited_place.volume.is_some()
        && cited_place.page.is_some()
        && work.volume.is_none()
        && work.page.is_none()
        && container.agreement != Agreement::Match;
    let journal_less = !matches!(
        work.work_type,
        Some(
            WorkType::Preprint
                | WorkType::Book
                | WorkType::Chapter
                | WorkType::Dataset
                | WorkType::Software
        )
    ) && work.container_title.is_none()
        && work.volume.is_none()
        && work.page.is_none();
    if (journal_less || unconfirmable) && cited_journal.is_some() && ids != Agreement::Match {
        confidence = confidence.min(UNPLACED_CAP);
    }
    // Another edition of a book: the same title and authors, another year
    // (a 9th edition cited, a later one found). The book is right; the
    // year, or a publisher given as its container, is a difference. A
    // chapter cited in the book is not the book: its authors found may be
    // the book's editors. A book cited with its publisher, found reprinted
    // whole as a chapter of a reader (Bourdieu's "Distinction" in a 2018
    // collection), is another edition too.
    let cites_book = text.as_ref().is_some_and(|t| {
        t.words.iter().any(|w| {
            matches!(
                w.as_str(),
                "press" | "publisher" | "publishers" | "publishing" | "books" | "verlag"
            )
        })
    });
    if (work.work_type == Some(WorkType::Book)
        || (work.work_type == Some(WorkType::Chapter) && cites_book))
        && (title.agreement == Agreement::Match || extended)
        && authors.score.is_some_and(|s| s >= CORROBORATING_AUTHORS)
        && year.agreement == Agreement::Mismatch
        && !claim.text.as_deref().is_some_and(cites_chapter)
    {
        confidence = confidence.max(EDITION);
    }
    // A dataset's or software's record carries its latest version's date:
    // the claimed identifier and title name it, whatever year is cited
    // (MIMIC-III's DOI, cited as of 2016, registered in 2023).
    if matches!(work.work_type, Some(WorkType::Dataset | WorkType::Software))
        && ids == Agreement::Match
        && title.agreement == Agreement::Match
        && year.agreement == Agreement::Mismatch
    {
        confidence = confidence.max(VERSION);
    }
    // Authors the work doesn't have are an error in the citation, and a
    // tell of invented references, whatever else agrees.
    if strangers {
        confidence = confidence.min(STRANGERS_CAP);
    }
    // So is a title with words the work's lacks, whatever else agrees.
    if extended {
        confidence = confidence.min(EXTENDED_TITLE_CAP);
    }
    // A reply, correction or commentary is not the work it responds to,
    // unless the citation says it is citing one.
    if let Some(actual) = &work_title
        && is_derivative(actual)
        && !claim_mentions_derivative(claim)
    {
        confidence *= DERIVATIVE_PENALTY;
    }

    Assessment {
        confidence,
        title,
        authors,
        year,
        container,
        volume,
        page,
        ids,
    }
}

fn combine(title: FieldCheck, authors: FieldCheck, year: FieldCheck) -> f64 {
    let parts = [(title, W_TITLE), (authors, W_AUTHORS), (year, W_YEAR)];
    let known: Vec<(f64, f64)> = parts
        .iter()
        .filter_map(|(check, w)| check.score.map(|s| (s, *w)))
        .collect();
    let weight: f64 = known.iter().map(|(_, w)| w).sum();
    if weight == 0.0 {
        return 0.0;
    }
    let mut confidence = known.iter().map(|(s, w)| s * w).sum::<f64>() / weight;
    if year.agreement == Agreement::Mismatch {
        confidence *= YEAR_MISMATCH_PENALTY;
    }
    if title.agreement == Agreement::Mismatch {
        confidence *= TITLE_MISMATCH_PENALTY;
    }
    // Different authors usually mean a different work (a similarly titled
    // book or survey), even when title and year agree.
    if authors.agreement == Agreement::Mismatch {
        confidence *= AUTHORS_MISMATCH_PENALTY;
    }
    if known.len() < 2 {
        confidence = confidence.min(SINGLE_FIELD_CAP);
    }
    confidence
}

/// Whether claimed identifiers are among a work's identifiers (see
/// [`Assessment::ids`]).
pub fn id_agreement(claimed: &[WorkId], actual: &[WorkId]) -> Agreement {
    if claimed.is_empty() {
        return Agreement::Unknown;
    }
    if claimed
        .iter()
        .any(|c| actual.iter().any(|a| same_work_id(c, a)))
    {
        return Agreement::Match;
    }
    let shares_kind = claimed
        .iter()
        .any(|c| actual.iter().any(|a| a.kind() == c.kind()));
    if shares_kind {
        Agreement::Mismatch
    } else {
        Agreement::Unknown
    }
}

/// Identifier equality at the level of works: arXiv versions are the same work.
pub fn same_work_id(a: &WorkId, b: &WorkId) -> bool {
    match (a, b) {
        (WorkId::Arxiv(x), WorkId::Arxiv(y)) => x.unversioned() == y.unversioned(),
        _ => a == b,
    }
}

/// Title similarity at which two records' titles count as one title when
/// telling whether they are the same work.
const SAME_WORK_TITLE: f64 = 0.9;

/// Whether two records are one work, as a preprint and its published
/// version are: a shared DOI, PMID or arXiv ID (published records carry
/// their preprint's arXiv ID); else the same title, the same first author
/// and years at most one apart. A reply or erratum repeating a title is
/// not the work it answers.
pub fn same_work(a: &SourceRecord, b: &SourceRecord) -> bool {
    let shared = a.ids.iter().any(|x| {
        matches!(x.kind(), IdKind::Doi | IdKind::Pmid | IdKind::Arxiv)
            && b.ids.iter().any(|y| same_work_id(x, y))
    });
    if shared {
        return true;
    }
    let (wa, wb) = (&a.work, &b.work);
    let (Some(ta), Some(tb)) = (&wa.title, &wb.title) else {
        return false;
    };
    if title_similarity(ta, tb) < SAME_WORK_TITLE
        || derivative_work_kind(ta) != derivative_work_kind(tb)
    {
        return false;
    }
    let first = |w: &crate::work::Work| w.authors.first().and_then(family_name);
    let same_first = match (first(wa), first(wb)) {
        (Some(x), Some(y)) => same_family(&x, &y),
        _ => false,
    };
    let near_years = match (wa.issued, wb.issued) {
        (Some(x), Some(y)) => (x.year() - y.year()).abs() <= 1,
        _ => false,
    };
    same_first && near_years
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

#[cfg(test)]
// Exact constants: scores and spends rounded or summed from binary-exact values.
#[allow(clippy::float_cmp)]
mod tests;
