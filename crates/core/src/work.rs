//! The minimal, provider-neutral description of a scholarly work.
//!
//! Shaped after CSL-JSON so export and citation formatting stay cheap, but it
//! is not CSL-JSON itself: identifiers live on [`SourceRecord`](crate::SourceRecord),
//! and anything source-specific stays in the provider's raw response.
//!
//! Every field is optional, because no provider fills everything. The structs
//! are `#[non_exhaustive]`: build them from `Default` and assign fields, so
//! adding a field later is not a breaking change.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::id::is_digits;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Work {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// In byline order. Empty means "not provided", not "anonymous".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<Person>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issued: Option<PartialDate>,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub work_type: Option<WorkType>,
    /// Journal, proceedings or book title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    #[serde(default, rename = "abstract", skip_serializing_if = "Option::is_none")]
    pub abstract_text: Option<String>,
    /// Landing page for the work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// As counted by the providing source; counts differ between sources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cited_by_count: Option<u64>,
    /// Retractions, corrections and other editorial notices about the work.
    ///
    /// Stored as `updates`, its name before 0.35, so caches written by
    /// either read in both.
    #[serde(default, rename = "updates", skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<Notice>,
    /// When the work is itself a notice, the works it updates: what an
    /// erratum corrects or a retraction notice retracts.
    /// Stored as `update_to`, as `notices` is as `updates`.
    #[serde(default, rename = "update_to", skip_serializing_if = "Vec::is_empty")]
    pub notice_for: Vec<NoticeFor>,
    /// Whether, and where, the work can be read free and legally.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_access: Option<OpenAccess>,
    /// Years of other editions or versions of the same work, when the
    /// provider knows them (a book's later editions). A citation of any of
    /// them is a citation of this work.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub other_years: Vec<i16>,
    /// When the work was read at its `url`: a web page's access date, as
    /// the caller gives it (CSL `accessed`, BibTeX `urldate`, RIS `Y2`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accessed: Option<PartialDate>,
    /// The work's language, as a language tag (`en`, `en-GB`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

/// An editorial notice about a work: retraction, correction, expression of
/// concern, withdrawal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Notice {
    pub kind: NoticeKind,
    /// The notice's own identifier (usually a DOI), if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice: Option<crate::id::WorkId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<PartialDate>,
    /// Who reported it: "retraction-watch", "publisher", "openalex", ….
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// The date the source gives for the update (Crossref's `updated`,
    /// often the issue's), when the notice appeared earlier: `date` is then
    /// the earliest known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deposited: Option<PartialDate>,
}

impl Notice {
    pub fn new(kind: NoticeKind) -> Self {
        Notice {
            kind,
            notice: None,
            date: None,
            source: None,
            deposited: None,
        }
    }
}

/// Whether, and where, a work can be read free and legally (as with
/// Unpaywall, whose data OpenAlex carries).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct OpenAccess {
    /// Free to read somewhere.
    pub is_oa: bool,
    /// Unpaywall's classes: `gold`, `diamond`, `hybrid`, `bronze` (free on
    /// the publisher's site without a licence), `green` (a repository
    /// copy), `closed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// The best free copy as a PDF.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdf_url: Option<String>,
    /// The best free copy's page (HTML full text or a landing page).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// That copy's licence, e.g. `cc-by`; none for bronze.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// `publishedVersion`, `acceptedVersion` or `submittedVersion`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Who said so: "openalex", "europepmc", ….
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// A notice's link to the work it updates (Crossref's `update-to`): the
/// other direction of an [`Notice`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct NoticeFor {
    pub kind: NoticeKind,
    /// The updated work.
    pub work: crate::id::WorkId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<PartialDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl NoticeFor {
    pub fn new(kind: NoticeKind, work: crate::id::WorkId) -> Self {
        NoticeFor {
            kind,
            work,
            date: None,
            source: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NoticeKind {
    Retraction,
    Withdrawal,
    ExpressionOfConcern,
    Correction,
    /// A newer version or edition supersedes the work (Cochrane's
    /// `.pub7` after `.pub6`), or a preprint's published version; the
    /// update's `notice` is the newer work.
    NewVersion,
    Other(String),
}

impl NoticeKind {
    pub fn as_str(&self) -> &str {
        match self {
            NoticeKind::Retraction => "retraction",
            NoticeKind::Withdrawal => "withdrawal",
            NoticeKind::ExpressionOfConcern => "expression_of_concern",
            NoticeKind::Correction => "correction",
            NoticeKind::NewVersion => "new_version",
            NoticeKind::Other(s) => s,
        }
    }

    /// From Crossref/Retraction Watch type names (`retraction`,
    /// `expression_of_concern`, `erratum`, …); never fails.
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().replace('-', "_").as_str() {
            "retraction" | "partial_retraction" => NoticeKind::Retraction,
            "withdrawal" | "removal" => NoticeKind::Withdrawal,
            "expression_of_concern" => NoticeKind::ExpressionOfConcern,
            "correction" | "erratum" | "corrigendum" | "addendum" => NoticeKind::Correction,
            "new_version" | "new_edition" => NoticeKind::NewVersion,
            _ => NoticeKind::Other(s.to_owned()),
        }
    }

    /// Whether citing the work is unsafe: retracted or withdrawn.
    pub fn is_retraction(&self) -> bool {
        matches!(self, NoticeKind::Retraction | NoticeKind::Withdrawal)
    }
}

impl Serialize for NoticeKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for NoticeKind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(NoticeKind::parse(&String::deserialize(d)?))
    }
}

impl Work {
    /// The first retraction or withdrawal notice, if any.
    pub fn retraction(&self) -> Option<&Notice> {
        self.notices.iter().find(|u| u.kind.is_retraction())
    }

    /// Types a work that is itself a notice (it has `notice_for`) as one:
    /// `retraction` for retractions and withdrawals, `erratum` for other
    /// notices, as OpenAlex names them. Registries type notices as plain
    /// articles. New versions are works in their own right and keep theirs.
    pub fn type_as_notice(&mut self) {
        let kinds = || self.notice_for.iter().map(|t| &t.kind);
        let name = if kinds().any(NoticeKind::is_retraction) {
            "retraction"
        } else if kinds().any(|k| *k != NoticeKind::NewVersion) {
            "erratum"
        } else {
            return;
        };
        self.work_type = Some(WorkType::Other(name.into()));
    }

    /// The newer version superseding this work, if one is known.
    pub fn newer_version(&self) -> Option<&crate::id::WorkId> {
        self.notices
            .iter()
            .filter(|u| u.kind == NoticeKind::NewVersion)
            .find_map(|u| u.notice.as_ref())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Person {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub given: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// A name that was not split into given/family, or an organisation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub literal: Option<String>,
    /// Bare ORCID iD, e.g. `0000-0002-1825-0097`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orcid: Option<String>,
    /// The author's OpenAlex id, bare: `A5023888391`. Only OpenAlex's
    /// records give it (and records of the same work merged with one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openalex_id: Option<String>,
}

impl Person {
    pub fn new(given: impl Into<String>, family: impl Into<String>) -> Self {
        Person {
            given: Some(given.into()),
            family: Some(family.into()),
            ..Person::default()
        }
    }

    pub fn literal(name: impl Into<String>) -> Self {
        Person {
            literal: Some(name.into()),
            ..Person::default()
        }
    }

    /// Family and given names: as given, or split from a name written
    /// whole ("Vincent van Gogh" → "van Gogh", "Vincent"). `None` for an
    /// organisation, or a name that may be one ("WHO", "GBD 2019
    /// Collaborators").
    pub fn parts(&self) -> Option<(String, Option<String>)> {
        match (&self.family, &self.literal) {
            (Some(family), _) => Some((family.clone(), self.given.clone())),
            (None, Some(literal)) => split_literal(literal),
            (None, None) => None,
        }
    }

    /// The given names' initials: "Dong Vu" → "DV", "Jean-Pierre" → "JP",
    /// "T. D. N." → "TDN".
    pub fn initials(&self) -> Option<String> {
        let (_, given) = self.parts()?;
        let initials: String = given?
            .split(|c: char| c.is_whitespace() || c == '-' || c == '.')
            .filter_map(|part| part.chars().find(|c| c.is_alphabetic()))
            .flat_map(char::to_uppercase)
            .collect();
        (!initials.is_empty()).then_some(initials)
    }

    /// The name as Vancouver writes it: "Laan DV"; an organisation as
    /// written.
    pub fn short(&self) -> Option<String> {
        match self.parts() {
            Some((family, _)) => Some(match self.initials() {
                Some(initials) => format!("{family} {initials}"),
                None => family,
            }),
            None => self.display_name(),
        }
    }

    /// The name as BibTeX and RIS write it: "LeCun, Yann"; an organisation
    /// as written.
    pub fn inverted(&self) -> Option<String> {
        match self.parts() {
            Some((family, Some(given))) => Some(format!("{family}, {given}")),
            Some((family, None)) => Some(family),
            None => self.display_name(),
        }
    }

    /// The best single-string name: "Given Family", or the literal.
    pub fn display_name(&self) -> Option<String> {
        match (&self.given, &self.family, &self.literal) {
            (Some(g), Some(f), _) => Some(format!("{g} {f}")),
            (None, Some(f), _) => Some(f.clone()),
            (_, _, Some(l)) => Some(l.clone()),
            (Some(g), None, None) => Some(g.clone()),
            (None, None, None) => None,
        }
    }
}

/// Lowercase words that join a family name: "van Gogh", "de la Cruz".
const PARTICLES: [&str; 14] = [
    "van", "von", "de", "der", "den", "da", "di", "du", "la", "le", "del", "dos", "ten", "ter",
];

/// Words that make a name an organisation's.
const BODIES: [&str; 55] = [
    "university",
    "institute",
    "institution",
    "group",
    "team",
    "organization",
    "organisation",
    "consortium",
    "committee",
    "association",
    "society",
    "collaboration",
    "network",
    "centre",
    "center",
    "foundation",
    "agency",
    "council",
    "department",
    "ministry",
    "office",
    "board",
    "federation",
    "alliance",
    "project",
    "initiative",
    "commission",
    "programme",
    "program",
    "inc",
    "ltd",
    "corporation",
    "company",
    "trust",
    "investigators",
    "study",
    "panel",
    "laboratory",
    "hospital",
    "college",
    "subcommittee",
    "taskforce",
    "workgroup",
    "secretariat",
    "trialists",
    "collaborators",
    "collaborative",
    "force",
    "bank",
    "service",
    "services",
    "authority",
    "unit",
    "health",
    "research",
];

/// Whether a (lowercase) word joins a family name: "van", "de".
pub(crate) fn is_particle(word: &str) -> bool {
    PARTICLES.contains(&word)
}

/// Whether a (lowercase) word makes a name an organisation's: "group",
/// "consortium", "collaboration".
pub(crate) fn is_body_word(word: &str) -> bool {
    BODIES.contains(&word)
}

/// A name written family name first, as DataCite's creators often are
/// ("Lubrano, Emanuele", "Carvalho, Schubert Ribeiro de"): its family and
/// given names. Not a name with a suffix or degree after the comma ("Smith,
/// Jr.", "Julio A., M.D. Ramirez"), nor initials before it.
pub(crate) fn inverted(literal: &str) -> Option<(&str, &str)> {
    let (family, given) = literal.split_once(',')?;
    let (family, given) = (family.trim(), given.trim());
    let named = !family.is_empty()
        && family.split_whitespace().count() <= 3
        && family
            .split_whitespace()
            .all(|w| !w.ends_with('.') && w.chars().filter(|c| c.is_alphabetic()).count() >= 2);
    let suffix = given.split_whitespace().any(|w| {
        matches!(
            w.trim_end_matches(['.', ','])
                .replace('.', "")
                .to_lowercase()
                .as_str(),
            "jr" | "sr" | "ii" | "iii" | "iv" | "md" | "phd" | "dr" | "prof" | "msc" | "mph"
        )
    });
    let whole = format!("{given} {family}");
    (named
        && !given.is_empty()
        && !given.contains(',')
        && !suffix
        && split_literal(&whole).is_some())
    .then_some((family, given))
}

/// Suffixes, degrees and honours written after a name, as written (case
/// matters: "MA" is a degree, "Ma" a family name), dots dropped.
const POST_NOMINALS: &[&str] = &[
    "Jr", "Sr", "II", "III", "IV", "PhD", "DPhil", "MD", "MB", "BS", "BCh", "ChB", "MBBS", "MBChB",
    "BM", "BSc", "MSc", "DSc", "MA", "BA", "MS", "MPH", "MBA", "RN", "DO", "PharmD", "EdD", "PsyD",
    "DDS", "DMD", "DVM", "JD", "LLB", "LLM", "FRS", "FRSE", "FRCP", "FRCS", "FRCPC", "FACP",
    "FACS", "FMedSci", "MRCP", "OBE", "CBE", "MBE", "Hon",
];

/// Post-nominals that are family names too ("Wei MA", printed in
/// capitals): taken as post-nominal only before another.
const NAMES_TOO: &[&str] = &["MA", "BA", "DO", "MS"];

/// A name without what follows it, as some records print one (`James M.
/// Ritter DPhil FRCP FBPharmacolS FMedSci`, `Arthur J. Atkinson Jr.`): the
/// words before its first suffix or degree, once a given name and a family
/// name stand before it. Everything after that word goes too, however
/// written (a fellowship's letters: `FBPharmacolS`).
pub(crate) fn without_post_nominals(literal: &str) -> &str {
    let words: Vec<(usize, &str)> = literal
        .split_whitespace()
        .map(|w| (w.as_ptr() as usize - literal.as_ptr() as usize, w))
        .collect();
    let bare = |w: &str| w.trim_end_matches([',', ';']).replace('.', "");
    let letters = |w: &str| w.chars().filter(|c| c.is_alphabetic()).count();
    let cut = (2..words.len()).find(|&i| {
        let w = bare(words[i].1);
        // After a family name, not an initial: "Julio A., M.D. Ramirez"
        // (Open Library's) has its family name after the degree.
        letters(words[i - 1].1) >= 2
            && POST_NOMINALS.contains(&w.as_str())
            && (!NAMES_TOO.contains(&w.as_str())
                || words
                    .get(i + 1)
                    .is_some_and(|(_, next)| POST_NOMINALS.contains(&bare(next).as_str())))
    });
    match cut {
        Some(i) => literal[..words[i].0].trim_end().trim_end_matches(','),
        None => literal,
    }
}

/// A personal name written whole ("Yann LeCun", "Vincent van Gogh"), as
/// family and given names; `None` for anything that may be an
/// organisation's.
fn split_literal(literal: &str) -> Option<(String, Option<String>)> {
    let literal = without_post_nominals(literal);
    if literal.contains(',') {
        return inverted(literal)
            .map(|(family, given)| (family.to_owned(), Some(given.to_owned())));
    }
    let words: Vec<&str> = literal.split_whitespace().collect();
    let acronym = |w: &str| w.chars().count() > 2 && w.chars().all(|c| !c.is_lowercase());
    // A family name in capitals after given names written otherwise
    // ("Seok-Bae LEE", "A. H. LINSSEN"), as some sources print it; not
    // an acronym among other words ("NHS England", "WHO Europe").
    let capitals_last = |i: usize| {
        i + 1 == words.len() && words[..i].iter().all(|w| !acronym(w.trim_end_matches('.')))
    };
    let personal = (2..=5).contains(&words.len())
        && words.iter().enumerate().all(|(i, w)| {
            let lower = w.trim_end_matches(['.', ',']).to_lowercase();
            let capital = w.chars().next().is_some_and(char::is_uppercase);
            (capital || PARTICLES.contains(&lower.as_str()))
                && (!acronym(w) || capitals_last(i))
                && !BODIES.contains(&lower.as_str())
                && !w.chars().any(|c| c.is_ascii_digit())
        });
    if !personal {
        return None;
    }
    let mut start = words.len() - 1;
    while start > 1 && PARTICLES.contains(&words[start - 1]) {
        start -= 1;
    }
    // A family name of one letter is a name mangled upstream, not one to
    // cite by: OpenAlex lists a PASTIES trial author (Siobhan Creanor) as
    // "S. C. S" (A5004132779, seen 2026-11-14). It stays as written. Two
    // letters are a family name often enough (Li, Wu, Ng: "J. Li").
    if words[start..].join("").trim_matches('.').chars().count() == 1 {
        return None;
    }
    Some((words[start..].join(" "), Some(words[..start].join(" "))))
}

/// Removes an "Abstract" heading that some sources leave at the start of
/// abstract text (JATS `<title>Abstract</title>`, kept as a word by
/// OpenAlex). Only a standalone heading: "Abstraction is…" is untouched.
pub fn tidy_abstract(text: &str) -> String {
    let text = text.trim();
    for heading in ["Abstract", "ABSTRACT"] {
        if let Some(rest) = text.strip_prefix(heading)
            && rest.starts_with(|c: char| c.is_whitespace() || matches!(c, ':' | '.' | '-' | '—'))
        {
            return rest
                .trim_start_matches(|c: char| {
                    c.is_whitespace() || matches!(c, ':' | '.' | '-' | '—')
                })
                .to_owned();
        }
    }
    text.to_owned()
}

/// A bare ORCID iD, as [`Person::orcid`] holds it, from one written bare
/// or as an `orcid.org` URL. `None` if nothing is left.
pub fn orcid(raw: &str) -> Option<String> {
    let bare = ["https://orcid.org/", "http://orcid.org/"]
        .iter()
        .find_map(|p| raw.strip_prefix(p))
        .unwrap_or(raw);
    (!bare.is_empty()).then(|| bare.to_owned())
}

/// Years a work could have been published in: a four-digit number in
/// text is read as a year only within these.
pub(crate) const PLAUSIBLE_YEARS: std::ops::RangeInclusive<i16> = 1600..=2100;

/// A date known to year, month or day precision. Serialises as `2017`,
/// `2017-06` or `2017-06-12`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PartialDate {
    year: i16,
    month: Option<u8>,
    day: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid date {0:?}: expected YYYY, YYYY-MM or YYYY-MM-DD")]
pub struct DateError(String);

impl PartialDate {
    pub fn from_year(year: i16) -> Self {
        PartialDate {
            year,
            month: None,
            day: None,
        }
    }

    /// `None` if the month or day is out of range for the calendar.
    pub fn new(year: i16, month: Option<u8>, day: Option<u8>) -> Option<Self> {
        match (month, day) {
            (None, Some(_)) => return None,
            (Some(m), None) if !(1..=12).contains(&m) => return None,
            (Some(m), Some(d)) => {
                jiff::civil::Date::new(year, i8::try_from(m).ok()?, i8::try_from(d).ok()?).ok()?;
            }
            _ => {}
        }
        Some(PartialDate { year, month, day })
    }

    pub fn year(&self) -> i16 {
        self.year
    }

    pub fn month(&self) -> Option<u8> {
        self.month
    }

    pub fn day(&self) -> Option<u8> {
        self.day
    }
}

impl FromStr for PartialDate {
    type Err = DateError;

    fn from_str(s: &str) -> Result<Self, DateError> {
        let err = || DateError(s.to_owned());
        let mut parts = s.trim().splitn(3, '-');
        // The next part, if it is `len` digits.
        let mut num = |len: usize| match parts.next() {
            None => Ok(None),
            Some(p) if p.len() == len && is_digits(p) => Ok(Some(p)),
            Some(_) => Err(err()),
        };
        let year = num(4)?.ok_or_else(err)?;
        let month = num(2)?;
        let day = num(2)?;
        // Four digits fit an `i16`, two a `u8`.
        let two = |d: &str| d.parse().expect("two digits");
        PartialDate::new(
            year.parse().expect("four digits"),
            month.map(two),
            day.map(two),
        )
        .ok_or_else(err)
    }
}

impl fmt::Display for PartialDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}", self.year)?;
        if let Some(m) = self.month {
            write!(f, "-{m:02}")?;
        }
        if let Some(d) = self.day {
            write!(f, "-{d:02}")?;
        }
        Ok(())
    }
}

/// Kind of work, as CSL types. Unknown upstream types are kept verbatim in
/// `Other` rather than dropped.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WorkType {
    JournalArticle,
    ConferencePaper,
    /// CSL `article`: preprints and other unreviewed articles.
    Preprint,
    Book,
    Chapter,
    Thesis,
    Report,
    Dataset,
    Software,
    Review,
    /// CSL `webpage`: a page cited as itself, not as a scholarly work (see
    /// [`crate::webpage::WebPage`]).
    WebPage,
    Other(String),
}

impl WorkType {
    /// The CSL type name.
    pub fn as_csl(&self) -> &str {
        match self {
            WorkType::JournalArticle => "article-journal",
            WorkType::ConferencePaper => "paper-conference",
            WorkType::Preprint => "article",
            WorkType::Book => "book",
            WorkType::Chapter => "chapter",
            WorkType::Thesis => "thesis",
            WorkType::Report => "report",
            WorkType::Dataset => "dataset",
            WorkType::Software => "software",
            WorkType::Review => "review",
            WorkType::WebPage => "webpage",
            WorkType::Other(s) => s,
        }
    }

    /// The type in words, for people: "journal article", "book chapter",
    /// "preprint". Other CSL types as named, without their hyphens.
    pub fn label(&self) -> String {
        let named = match self {
            WorkType::JournalArticle => "journal article",
            WorkType::ConferencePaper => "conference paper",
            WorkType::Preprint => "preprint",
            WorkType::Book => "book",
            WorkType::Chapter => "book chapter",
            WorkType::Thesis => "thesis",
            WorkType::Report => "report",
            WorkType::Dataset => "dataset",
            WorkType::Software => "software",
            WorkType::Review => "review",
            WorkType::WebPage => "web page",
            WorkType::Other(s) => match s.as_str() {
                "article-magazine" => "magazine article",
                "article-newspaper" => "newspaper article",
                "entry-dictionary" => "dictionary entry",
                "entry-encyclopedia" => "encyclopedia entry",
                "post-weblog" => "blog post",
                "retraction" => "retraction notice",
                "erratum" => "correction notice",
                other => return other.replace(['-', '_'], " "),
            },
        };
        named.to_owned()
    }

    /// Inverse of [`WorkType::as_csl`]; never fails.
    pub fn from_csl(s: &str) -> Self {
        match s {
            "article-journal" => WorkType::JournalArticle,
            "paper-conference" => WorkType::ConferencePaper,
            "article" => WorkType::Preprint,
            "book" => WorkType::Book,
            "chapter" => WorkType::Chapter,
            "thesis" => WorkType::Thesis,
            "report" => WorkType::Report,
            "dataset" => WorkType::Dataset,
            "software" => WorkType::Software,
            "review" => WorkType::Review,
            "webpage" => WorkType::WebPage,
            other => WorkType::Other(other.to_owned()),
        }
    }
}

impl Serialize for WorkType {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_csl())
    }
}

impl<'de> Deserialize<'de> for WorkType {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(WorkType::from_csl(&String::deserialize(d)?))
    }
}

impl Serialize for PartialDate {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for PartialDate {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_are_stored_under_their_old_names() {
        // Caches are shared across versions: a work stored by this one
        // reads in one before the rename (#160), and the other way round.
        let mut w = Work::default();
        w.notices.push(Notice::new(NoticeKind::Retraction));
        let doi: crate::id::WorkId = "10.1234/x".parse().unwrap();
        w.notice_for
            .push(NoticeFor::new(NoticeKind::Correction, doi));
        let stored = serde_json::to_value(&w).unwrap();
        assert_eq!(stored["updates"][0]["kind"], "retraction");
        assert_eq!(stored["update_to"][0]["kind"], "correction");
        assert!(stored.get("notices").is_none());
        let read: Work = serde_json::from_value(stored).unwrap();
        assert_eq!(read, w);
    }

    #[test]
    fn names_split_and_shorten_as_vancouver_writes_them() {
        let short = |p: Person| p.short().unwrap();
        assert_eq!(short(Person::new("Dong V.", "Laan")), "Laan DV");
        assert_eq!(short(Person::new("Thi Diem Ngoc", "Vu")), "Vu TDN");
        assert_eq!(short(Person::new("Jean-Pierre", "Dupont")), "Dupont JP");
        assert_eq!(short(Person::literal("Yann LeCun")), "LeCun Y");
        assert_eq!(short(Person::literal("Vincent van Gogh")), "van Gogh V");
        assert_eq!(short(Person::literal("WHO")), "WHO");
        assert_eq!(
            short(Person::literal("GBD 2019 Collaborators")),
            "GBD 2019 Collaborators"
        );
        assert_eq!(
            Person::literal("Vincent van Gogh").parts(),
            Some(("van Gogh".into(), Some("Vincent".into())))
        );
        assert_eq!(Person::literal("World Health Organization").parts(), None);
        // Inverted (DataCite's creators), or the family name in capitals
        // (holdout-2 #407, #482).
        let parts = |name: &str| Person::literal(name).parts();
        assert_eq!(
            parts("Lubrano, Emanuele"),
            Some(("Lubrano".into(), Some("Emanuele".into())))
        );
        assert_eq!(
            parts("van der Berg, Anna"),
            Some(("van der Berg".into(), Some("Anna".into())))
        );
        assert_eq!(
            parts("Seok-Bae LEE"),
            Some(("LEE".into(), Some("Seok-Bae".into())))
        );
        assert_eq!(
            parts("A. H. LINSSEN"),
            Some(("LINSSEN".into(), Some("A. H.".into())))
        );
        assert_eq!(parts("NHS England"), None);
        assert_eq!(parts("Smith, Jr."), None);
        assert_eq!(parts("Consortium, The PROTECT"), None);
        // #69: OpenAlex title-cases "ATLS Subcommittee".
        for body in [
            "Atls Subcommittee",
            "Sepsis Taskforce",
            "CRASH Trialists",
            "Covid Collaborators",
            "Public Health England",
            "Cancer Research UK",
            "World Bank",
            "Health Canada",
            "Sepsis Task Force",
            "CovidSurg Collaborative",
            "Blood Transfusion Service",
            "Indian Health Services",
            "Patient Safety Authority",
            "Clinical Trials Unit",
        ] {
            assert_eq!(Person::literal(body).parts(), None, "{body}");
            assert_eq!(short(Person::literal(body)), body);
        }
    }

    /// OpenAlex gives Siobhan Creanor, on one of the PASTIES trials, as
    /// "S. C. S": not family "S", but the name as written. Two-letter
    /// family names are real.
    #[test]
    fn a_one_letter_family_name_stays_as_written() {
        let mangled = Person::literal("S. C. S");
        assert_eq!(mangled.parts(), None);
        assert_eq!(mangled.short().as_deref(), Some("S. C. S"));
        assert_eq!(mangled.inverted().as_deref(), Some("S. C. S"));
        assert_eq!(
            Person::literal("J. Li").parts(),
            Some(("Li".into(), Some("J.".into())))
        );
        assert_eq!(Person::literal("Wei Wu").short().as_deref(), Some("Wu W"));
    }

    /// Open Library prints degrees and honours after names (wild-4 #322,
    /// #325): the family name is before them.
    #[test]
    fn suffixes_and_degrees_after_a_name_are_not_its_family_name() {
        let parts = |name: &str| Person::literal(name).parts();
        let family = |name: &str| parts(name).map(|(family, _)| family);
        assert_eq!(
            parts("James M. Ritter DPhil FRCP FBPharmacolS FMedSci"),
            Some(("Ritter".into(), Some("James M.".into())))
        );
        assert_eq!(
            family("Yoon Kong Loke MB  BS  MRCP  MD").as_deref(),
            Some("Loke")
        );
        assert_eq!(
            family("David MacEwan PhD  FRSB  FBPhS  SFHEA").as_deref(),
            Some("MacEwan")
        );
        assert_eq!(
            family("Arthur J. Atkinson  Jr.").as_deref(),
            Some("Atkinson")
        );
        assert_eq!(family("John Smith, M.D.").as_deref(), Some("Smith"));
        // A family name that is a degree's letters too, in capitals.
        assert_eq!(family("Wei Jun MA").as_deref(), Some("MA"));
        // Not after an initial: the family name may follow ("Julio A.,
        // M.D. Ramirez", holdout-1 #660), or a degree's letters be a name.
        assert_eq!(
            without_post_nominals("Julio A., M.D. Ramirez"),
            "Julio A., M.D. Ramirez"
        );
        assert_eq!(without_post_nominals("Smith Jr."), "Smith Jr.");
        assert_eq!(without_post_nominals("Ma Long"), "Ma Long");
    }

    #[test]
    fn names_invert_family_first() {
        let inverted = |p: Person| p.inverted();
        assert_eq!(
            inverted(Person::new("Ada", "Lovelace")).as_deref(),
            Some("Lovelace, Ada")
        );
        assert_eq!(
            inverted(Person::literal("Vincent van Gogh")).as_deref(),
            Some("van Gogh, Vincent")
        );
        let family_only = Person {
            family: Some("Plato".into()),
            ..Person::default()
        };
        assert_eq!(inverted(family_only).as_deref(), Some("Plato"));
        assert_eq!(
            inverted(Person::literal("World Health Organization")).as_deref(),
            Some("World Health Organization")
        );
        assert_eq!(inverted(Person::default()), None);
    }

    #[test]
    fn partial_dates_parse_validate_and_round_trip() {
        for s in ["2017", "2017-06", "2017-06-12", "2024-02-29"] {
            assert_eq!(s.parse::<PartialDate>().unwrap().to_string(), s);
        }
        for s in [
            "17",
            "2017-6",
            "2017-13",
            "2017-06-31",
            "2023-02-29",
            "2017--01",
            "x",
        ] {
            assert!(s.parse::<PartialDate>().is_err(), "{s:?}");
        }
        assert_eq!(PartialDate::new(2017, None, Some(3)), None);
    }

    #[test]
    fn work_type_keeps_unknown_values() {
        assert_eq!(
            WorkType::from_csl("article-journal"),
            WorkType::JournalArticle
        );
        let other = WorkType::from_csl("musical-score");
        assert_eq!(other.as_csl(), "musical-score");
    }

    #[test]
    fn work_types_read_as_words() {
        let label = |csl| WorkType::from_csl(csl).label();
        assert_eq!(label("article-journal"), "journal article");
        assert_eq!(label("chapter"), "book chapter");
        assert_eq!(label("article"), "preprint");
        assert_eq!(label("entry-encyclopedia"), "encyclopedia entry");
        assert_eq!(label("musical-score"), "musical score");
    }

    #[test]
    fn work_serialises_sparsely_with_csl_names() {
        let w = Work {
            title: Some("Attention Is All You Need".into()),
            issued: Some(PartialDate::from_year(2017)),
            work_type: Some(WorkType::ConferencePaper),
            authors: vec![Person::new("Ashish", "Vaswani")],
            ..Work::default()
        };
        let json = serde_json::to_value(&w).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "title": "Attention Is All You Need",
                "authors": [{"given": "Ashish", "family": "Vaswani"}],
                "issued": "2017",
                "type": "paper-conference",
            })
        );
        assert_eq!(serde_json::from_value::<Work>(json).unwrap(), w);
    }

    #[test]
    fn tidy_abstract_drops_only_a_standalone_heading() {
        assert_eq!(
            tidy_abstract("Abstract Proteins are essential"),
            "Proteins are essential"
        );
        assert_eq!(tidy_abstract("ABSTRACT: We show"), "We show");
        assert_eq!(
            tidy_abstract("  Abstraction is useful"),
            "Abstraction is useful"
        );
        assert_eq!(tidy_abstract("We show"), "We show");
    }

    #[test]
    fn orcid_drops_the_url() {
        for raw in [
            "0000-0002-1825-0097",
            "https://orcid.org/0000-0002-1825-0097",
            "http://orcid.org/0000-0002-1825-0097",
        ] {
            assert_eq!(orcid(raw).as_deref(), Some("0000-0002-1825-0097"), "{raw}");
        }
        assert_eq!(orcid("https://orcid.org/"), None);
        assert_eq!(orcid(""), None);
    }

    #[test]
    fn person_display_name_prefers_split_names() {
        assert_eq!(
            Person::new("Ada", "Lovelace").display_name().unwrap(),
            "Ada Lovelace"
        );
        assert_eq!(Person::literal("CERN").display_name().unwrap(), "CERN");
        assert_eq!(Person::default().display_name(), None);
    }
}
