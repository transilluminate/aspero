//! A claim from a publisher's article page: what its markup says the
//! work is, for `verify` or `lookup`. Pure: no network, nothing fetched.
//!
//! [`claim_from_page`] takes the page's markup as a browser rendered it
//! (its `<head>` and JSON-LD scripts, a whole page, a fragment, or
//! nothing), its address, and a title a browser gave it (a dragged
//! link's name). It reads, each field from the first that gives it
//! ([`PageSource`] order):
//!
//! 1. Highwire Press `citation_*` tags (Google Scholar's), as most
//!    publishers write them: `citation_doi`, `citation_pmid`,
//!    `citation_pmcid`, `citation_arxiv_id`, `citation_pii`,
//!    `citation_isbn`; `citation_title`; each `citation_author` (or the
//!    old `citation_authors`, `;`-separated); the journal
//!    (`citation_journal_title`, else `citation_journal_abbrev`,
//!    `citation_conference_title`, `citation_book_title`); volume; first
//!    and last page; the year of `citation_publication_date`, else
//!    `citation_date`, `citation_online_date`.
//! 2. PRISM: `prism.doi`, `prism.publicationName`, `prism.volume`,
//!    `prism.startingPage`/`endingPage`, `prism.publicationDate` (else
//!    `prism.coverDate`).
//! 3. Dublin Core (`dc.` or `dcterms.`, any case): `identifier` (`doi:…`,
//!    `https://doi.org/…`, `info:doi/…`), `title`, `creator`, the year of
//!    `issued`, `date.issued`, `date` or `created`, and `source` as the
//!    journal when it reads as a name (no digits: not a citation).
//! 4. schema.org JSON-LD (see `jsonld`).
//! 5. For a title only: `og:title`, else `<title>`, without the site's
//!    name (see `title`).
//! 6. For a title only, and only when nothing above gives one: the `title`
//!    given, if it is one.
//! 7. The address, scanned as [`scan`](crate::id::scan) reads it (a DOI in
//!    the path, Nature's articles, PIIs, PubMed, arXiv).
//!
//! Every identifier found, in that order and each once, is in
//! [`PageClaim::ids`] and the claim's. The journal is only ever the page's
//! metadata, never guessed from the address: a wrong one makes `verify`
//! report a journal mismatch. The address's host is the claim's
//! [`site`](Claim::site), a hint of the publisher for a claim without
//! identifiers (see [`crate::site`]), never a claimed field. The page's address is not an identifier
//! unless it holds one. With no identifier and no title, there's no claim.
//!
//! Caps: [`MAX_INPUT`] bytes of markup and [`MAX_TAGS`] tags are read,
//! [`MAX_AUTHORS`] authors kept, and values over [`MAX_VALUE`] characters
//! ignored.

mod html;
mod jsonld;
mod title;

pub use html::{MAX_INPUT, MAX_JSON_LD, MAX_TAGS};
use serde::{Deserialize, Serialize};

use crate::id::{IdKind, Mention, WorkId, scan};
use crate::matching::Claim;
use html::Markup;

/// Authors kept, in page order: enough for any comparison `verify` makes,
/// without carrying a physics collaboration's thousands.
pub const MAX_AUTHORS: usize = 50;
/// Longest value read, in characters: longer is a description or junk.
pub const MAX_VALUE: usize = 1000;

/// What a page's markup, its address or its given title says the work is.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PageClaim {
    /// Every identifier found, each once, in priority order.
    pub ids: Vec<PageId>,
    /// The work as the page describes it, identifiers included; `None`
    /// with no identifier and no title.
    pub claim: Option<Claim>,
    /// The first vocabulary, in priority order, that gave an identifier or
    /// the title: what answered. [`PageSource::Url`] when nothing did.
    pub source: PageSource,
}

/// One identifier a page gave.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PageId {
    /// Located in [`PageId::text`]; `from_url` only when read from the
    /// page's address (`source` is [`PageSource::Url`]), so an app can
    /// say the work was found from its address.
    pub mention: Mention,
    /// The value it was read from: the tag's content (decoded) or the
    /// address.
    pub text: String,
    pub source: PageSource,
}

/// Where a page's claim came from, in priority order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PageSource {
    /// Highwire Press `citation_*` meta tags.
    Highwire,
    /// PRISM `prism.*` meta tags.
    Prism,
    /// Dublin Core `dc.*` / `dcterms.*` meta tags.
    DublinCore,
    /// schema.org JSON-LD.
    JsonLd,
    /// The page's `og:title` or `<title>`.
    Title,
    /// The title given with the page (a dragged link's name).
    TitleHint,
    /// The page's address.
    Url,
}

impl PageSource {
    /// As JSON names it: `highwire`, `dublin_core`, `title_hint`, ….
    pub fn as_str(self) -> &'static str {
        match self {
            PageSource::Highwire => "highwire",
            PageSource::Prism => "prism",
            PageSource::DublinCore => "dublin_core",
            PageSource::JsonLd => "json_ld",
            PageSource::Title => "title",
            PageSource::TitleHint => "title_hint",
            PageSource::Url => "url",
        }
    }

    /// In English: "the page's Highwire tags (citation_*)".
    pub fn describe(self) -> &'static str {
        match self {
            PageSource::Highwire => "the page's Highwire tags (citation_*)",
            PageSource::Prism => "the page's PRISM tags (prism.*)",
            PageSource::DublinCore => "the page's Dublin Core tags (dc.*)",
            PageSource::JsonLd => "the page's schema.org JSON-LD",
            PageSource::Title => "the page's title",
            PageSource::TitleHint => "the title given",
            PageSource::Url => "the page's address",
        }
    }
}

/// What one vocabulary says.
#[derive(Debug, Default)]
struct Found {
    ids: Vec<PageId>,
    title: Option<String>,
    authors: Vec<String>,
    year: Option<i16>,
    container: Option<String>,
    volume: Option<String>,
    page: Option<String>,
}

/// The claim a page makes for the work it shows: see the module notes.
/// `html` is the page's markup (`None` when it didn't load), `url` its
/// address, `title` a title a browser gave it (a dragged link's name,
/// which may be link text such as "Full Text": then ignored).
pub fn claim_from_page(html: Option<&str>, url: &str, title: Option<&str>) -> PageClaim {
    let markup = html.map(html::read).unwrap_or_default();
    let vocabularies = [
        (PageSource::Highwire, highwire(&markup)),
        (PageSource::Prism, prism(&markup)),
        (PageSource::DublinCore, dublin_core(&markup)),
        (PageSource::JsonLd, jsonld::read(&markup.json_ld)),
    ];
    let mut source = None;
    let mut ids: Vec<PageId> = Vec::new();
    let mut claim = Claim::default();
    for (vocabulary, found) in vocabularies {
        if source.is_none() && (!found.ids.is_empty() || found.title.is_some()) {
            source = Some(vocabulary);
        }
        for id in found.ids {
            add_id(&mut ids, id);
        }
        claim.title = claim.title.or(found.title);
        if claim.authors.is_empty() {
            claim.authors = found.authors;
        }
        claim.year = claim.year.or(found.year);
        claim.container = claim.container.or(found.container);
        claim.volume = claim.volume.or(found.volume);
        claim.page = claim.page.or(found.page);
    }
    if claim.title.is_none() {
        let page_title = markup
            .first("og:title")
            .into_iter()
            .chain(markup.title.as_deref())
            .find_map(|t| title::title_from(t, url, title::MIN_PAGE_WORDS, 1));
        if let Some(t) = page_title {
            claim.title = Some(t);
            source = source.or(Some(PageSource::Title));
        }
    }
    if claim.title.is_none()
        && let Some(t) =
            title.and_then(|t| title::title_from(t, url, title::MIN_WORDS, title::MIN_CHARS))
    {
        claim.title = Some(t);
        source = source.or(Some(PageSource::TitleHint));
    }
    let address: Vec<PageId> = scan(url)
        .into_iter()
        .map(|m| PageId {
            mention: Mention {
                from_url: true,
                ..m
            },
            text: url.to_owned(),
            source: PageSource::Url,
        })
        .collect();
    for id in address {
        add_id(&mut ids, id);
    }
    claim.ids = ids.iter().map(|id| id.mention.id.clone()).collect();
    claim.site = crate::site::site_of(url);
    let claim = (!claim.ids.is_empty() || claim.title.is_some()).then_some(claim);
    PageClaim {
        ids,
        claim,
        source: source.unwrap_or(PageSource::Url),
    }
}

fn add_id(ids: &mut Vec<PageId>, id: PageId) {
    if !ids.iter().any(|i| i.mention.id == id.mention.id) {
        ids.push(id);
    }
}

fn highwire(m: &Markup) -> Found {
    let source = PageSource::Highwire;
    let mut found = Found::default();
    for (name, kind) in [
        ("citation_doi", IdKind::Doi),
        ("citation_pmid", IdKind::Pmid),
        ("citation_pmcid", IdKind::Pmcid),
        ("citation_arxiv_id", IdKind::Arxiv),
        ("citation_pii", IdKind::Pii),
        ("citation_isbn", IdKind::Isbn),
    ] {
        for value in m.all(name) {
            found.ids.extend(ids_in(value, Some(kind), source));
        }
    }
    found.title = field(m.first("citation_title"));
    for name in m.all("citation_author") {
        add_author(&mut found.authors, name);
    }
    if found.authors.is_empty() {
        for name in m.all("citation_authors").flat_map(|v| v.split(';')) {
            add_author(&mut found.authors, name);
        }
    }
    let book = field(m.first("citation_book_title")).filter(|b| Some(b) != found.title.as_ref());
    found.container = [
        "citation_journal_title",
        "citation_journal_abbrev",
        "citation_conference_title",
    ]
    .iter()
    .find_map(|n| field(m.first(n)))
    .or(book);
    found.volume = field(m.first("citation_volume"));
    found.page = pages(m.first("citation_firstpage"), m.first("citation_lastpage"));
    found.year = [
        "citation_publication_date",
        "citation_date",
        "citation_online_date",
    ]
    .iter()
    .find_map(|n| m.all(n).find_map(year_of));
    found
}

fn prism(m: &Markup) -> Found {
    let mut found = Found::default();
    for value in m.all("prism.doi") {
        found
            .ids
            .extend(ids_in(value, Some(IdKind::Doi), PageSource::Prism));
    }
    found.container = field(m.first("prism.publicationname"));
    found.volume = field(m.first("prism.volume"));
    found.page = pages(m.first("prism.startingpage"), m.first("prism.endingpage"));
    found.year = ["prism.publicationdate", "prism.coverdate"]
        .iter()
        .find_map(|n| m.all(n).find_map(year_of));
    found
}

fn dublin_core(m: &Markup) -> Found {
    // `dc.title`, `DCTERMS.title`, `dc.identifier.doi`, …
    let dc = |fields: &'static [&'static str]| {
        m.metas.iter().filter_map(move |(name, value)| {
            let rest = name
                .strip_prefix("dc.")
                .or_else(|| name.strip_prefix("dcterms."))?;
            let field = rest.split('.').next().unwrap_or(rest);
            (fields.contains(&rest) || fields.contains(&field)).then_some(value.as_str())
        })
    };
    let mut found = Found::default();
    for value in dc(&["identifier"]) {
        found
            .ids
            .extend(ids_in(value, None, PageSource::DublinCore));
    }
    found.title = dc(&["title"]).find_map(|t| field(Some(t)));
    for name in dc(&["creator"]) {
        add_author(&mut found.authors, name);
    }
    found.year = ["issued", "date.issued", "date", "created"]
        .iter()
        .find_map(|f| {
            m.metas.iter().find_map(|(name, value)| {
                let rest = name
                    .strip_prefix("dc.")
                    .or_else(|| name.strip_prefix("dcterms."))?;
                (rest == *f).then(|| year_of(value)).flatten()
            })
        });
    found.container = dc(&["source"])
        .filter_map(|s| field(Some(s)))
        .find(|s| !s.bytes().any(|b| b.is_ascii_digit()) && !s.contains("://"));
    found
}

/// The identifiers in a tag's value: the whole value as `kind` (or any
/// kind, when the tag doesn't say), else those [`scan`] finds in it.
fn ids_in(value: &str, kind: Option<IdKind>, source: PageSource) -> Vec<PageId> {
    let text = value.trim();
    if text.is_empty() || text.chars().count() > MAX_VALUE {
        return Vec::new();
    }
    let whole = match kind {
        Some(kind) => WorkId::parse_as(kind, text).ok(),
        None => text.parse::<WorkId>().ok(),
    };
    let mentions = match whole {
        Some(id) => vec![Mention {
            id,
            line: 1,
            column: 1,
            span: 0..text.len(),
            from_url: false,
        }],
        None => scan(text)
            .into_iter()
            .map(|m| Mention {
                from_url: false,
                ..m
            })
            .collect(),
    };
    mentions
        .into_iter()
        .map(|mention| PageId {
            mention,
            text: text.to_owned(),
            source,
        })
        .collect()
}

/// A tag's value, tidied; `None` when blank or too long to be one.
fn field(value: Option<&str>) -> Option<String> {
    value.and_then(tidy_value)
}

/// A value tidied ([`tidy`]), unless longer than [`MAX_VALUE`].
fn tidy_value(value: &str) -> Option<String> {
    tidy(value).filter(|v| v.chars().count() <= MAX_VALUE)
}

/// `s` with markup such as `<i>` removed and its whitespace collapsed;
/// `None` when nothing's left.
fn tidy(s: &str) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(lt) = rest.find('<') {
        let after = &rest[lt + 1..];
        let tag = after
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '/');
        match after.find('>') {
            Some(gt) if tag && !after[..gt].contains('<') => {
                out.push_str(&rest[..lt]);
                rest = &after[gt + 1..];
            }
            _ => {
                out.push_str(&rest[..=lt]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    let joined = out.split_whitespace().collect::<Vec<_>>().join(" ");
    (!joined.is_empty()).then_some(joined)
}

/// Adds an author's name, tidied, unless blank, "et al." or one too many.
fn add_author(authors: &mut Vec<String>, name: &str) {
    let Some(name) = tidy_value(name) else {
        return;
    };
    let name = name.trim_matches([',', ';']).trim().to_owned();
    let lower = name.to_lowercase();
    let filler = [
        "et al",
        "et al.",
        "and others",
        "others",
        "et. al.",
        "[no author]",
    ];
    if name.is_empty() || filler.contains(&lower.as_str()) || authors.len() >= MAX_AUTHORS {
        return;
    }
    authors.push(name);
}

/// A first page, or a range when the last page differs.
fn pages(first: Option<&str>, last: Option<&str>) -> Option<String> {
    let first = field(first)?;
    match field(last) {
        Some(last) if last != first => Some(format!("{first}-{last}")),
        _ => Some(first),
    }
}

/// The year a date gives: its first four-digit number from 1400 to 2199,
/// standing alone ("2026/01/15", "2026-01-15", "Jan 24, 2022", "2026").
fn year_of(date: &str) -> Option<i16> {
    date.split(|c: char| !c.is_ascii_digit())
        .filter(|run| run.len() == 4)
        .filter_map(|run| run.parse::<i16>().ok())
        .find(|y| (1400..2200).contains(y))
}

#[cfg(test)]
mod tests;
