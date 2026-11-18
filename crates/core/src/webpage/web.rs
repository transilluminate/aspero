//! A page that isn't a scholarly work, cited as itself (#119): a
//! Wikipedia article, a news story, an organisation's guidance. Its
//! record is read from the same markup as a claim, and writes out as a
//! [`Work`] of type [`WorkType::WebPage`] (CSL `webpage`).
//!
//! Each field from the first that gives it:
//! - title: a JSON-LD article's `name`, else its `headline` (#118), then
//!   `og:title`, `<title>`, a JSON-LD page's `name` (less sure: sites
//!   name pages by their template, WHO's "Detail"), and the title given,
//!   each as the page gives it but for its site's own name (see
//!   `title`);
//! - site: `og:site_name`, JSON-LD's `WebSite` (the main node's
//!   `isPartOf`, else any), the site's name as the page's title writes it
//!   when that names the host ("Wikipedia" on `en.wikipedia.org`),
//!   JSON-LD's publisher, else the host (`www.` dropped);
//! - address: `<link rel="canonical">`, `og:url`, JSON-LD's `url`, else
//!   the address given, resolved against it and normalised
//!   ([`normalise_url`]);
//! - authors: people only. JSON-LD's `Person` authors, else `author` meta
//!   tags, else `article:author` ones that aren't addresses; never an
//!   organisation (JSON-LD's, the publisher, the site), a byline holding
//!   one's name ("Guardian staff reporter"), or "Contributors to …";
//! - published: JSON-LD's `datePublished`, `article:published_time`,
//!   `citation_publication_date`, `citation_date`; updated:
//!   `dateModified`, `article:modified_time`, `og:updated_time`;
//! - language: `<html lang>`, JSON-LD's `inLanguage`, a
//!   `Content-Language` meta tag, `og:locale` (an app passing the `<head>`
//!   alone has no `<html>` tag).
//!
//! Its key is `url:` and the normalised address: the same page shared
//! twice, with other tracking parameters or fragments, is one record.

use super::html::Markup;
use super::jsonld::WebLd;
use super::{add_author, tidy_value, title, year_of};
use crate::work::{PartialDate, Person, Work, WorkType};

/// Whether a page shows a scholarly work or is a web page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PageKind {
    /// A scholarly work's page: it gives an identifier, citation tags or
    /// a scholarly article in JSON-LD, or is on a publisher's site
    /// ([`crate::site::PUBLISHERS`]); or no markup was given to tell.
    Work,
    /// Anything else with a title: cited as itself ([`WebPage`]).
    WebPage,
}

impl PageKind {
    /// As JSON names it: `work`, `web_page`.
    pub fn as_str(self) -> &'static str {
        match self {
            PageKind::Work => "work",
            PageKind::WebPage => "web_page",
        }
    }
}

/// A web page, as a citation of it needs: see the module notes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct WebPage {
    pub title: Option<String>,
    /// The website's name: "Wikipedia", "BBC News", or its host.
    pub site: Option<String>,
    /// The canonical address, normalised.
    pub url: String,
    /// `url:` and [`WebPage::url`]: the same page, the same key.
    pub key: String,
    /// People, in page order, as written.
    pub authors: Vec<String>,
    pub published: Option<PartialDate>,
    pub updated: Option<PartialDate>,
    /// A language tag: `en`, `en-GB`.
    pub language: Option<String>,
}

impl WebPage {
    /// The page as a work to export or cite: type `webpage`, the site as
    /// its container, published as issued, read on `accessed` (the
    /// caller's date: when the page was read).
    pub fn to_work(&self, accessed: Option<PartialDate>) -> Work {
        let mut w = Work::default();
        w.title.clone_from(&self.title);
        w.authors = self.authors.iter().map(Person::literal).collect();
        w.issued = self.published;
        w.work_type = Some(WorkType::WebPage);
        w.container_title.clone_from(&self.site);
        w.url = Some(self.url.clone());
        w.accessed = accessed;
        w.language.clone_from(&self.language);
        w
    }
}

/// The page's record, when it has a title (`hint`, a title given, is the
/// last resort).
pub(super) fn read(m: &Markup, ld: &WebLd, url: &str, hint: Option<&str>) -> Option<WebPage> {
    let og_site = m.first("og:site_name").and_then(tidy_value);
    let names: Vec<String> = [&og_site, &ld.website, &ld.publisher]
        .into_iter()
        .flatten()
        .cloned()
        .collect();
    let page_titles: Vec<&str> = m
        .first("og:title")
        .into_iter()
        .chain(m.title.as_deref())
        .collect();
    let title = ld
        .title
        .iter()
        .map(String::as_str)
        .chain(page_titles.iter().copied())
        .chain(ld.page_title.as_deref())
        .find_map(|t| title::title_named(t, url, &names, 1, 1))
        .or_else(|| {
            hint.and_then(|t| {
                title::title_named(t, url, &names, title::MIN_WORDS, title::MIN_CHARS)
            })
        })?;
    let site = og_site
        .clone()
        .or_else(|| ld.website.clone())
        .or_else(|| {
            page_titles
                .iter()
                .find_map(|t| title::site_in_title(t, url))
        })
        .or_else(|| ld.publisher.clone())
        .or_else(|| crate::site::site_of(url));
    let address = [
        m.canonical.as_deref(),
        m.first("og:url"),
        ld.url.as_deref(),
        Some(url),
    ]
    .into_iter()
    .flatten()
    .find_map(|u| resolve(u, url))
    .unwrap_or_else(|| url.trim().to_owned());
    let not_people: Vec<&String> = names.iter().chain(&ld.organisations).chain(&site).collect();
    let authors = people(m, ld, &not_people);
    let published = [ld.published.as_deref()]
        .into_iter()
        .chain(
            [
                "article:published_time",
                "citation_publication_date",
                "citation_date",
            ]
            .map(|n| m.first(n)),
        )
        .flatten()
        .find_map(date_of);
    let updated = [ld.updated.as_deref()]
        .into_iter()
        .chain(["article:modified_time", "og:updated_time"].map(|n| m.first(n)))
        .flatten()
        .find_map(date_of);
    let language = [
        m.lang.as_deref(),
        ld.language.as_deref(),
        m.first("content-language"),
        m.first("og:locale"),
    ]
    .into_iter()
    .flatten()
    .find_map(language_tag);
    Some(WebPage {
        title: Some(title),
        site,
        key: format!("url:{address}"),
        url: address,
        authors,
        published,
        updated,
        language,
    })
}

/// The page's authors, people only: see the module notes.
fn people(m: &Markup, ld: &WebLd, not_people: &[&String]) -> Vec<String> {
    let person = |name: &str| {
        let lower = name.to_lowercase();
        // The site's or publisher's name, or a byline holding it
        // ("Guardian staff reporter", "Ines Schöffmann, DER SPIEGEL").
        let organisation = not_people.iter().any(|o| names_in(name, o));
        let crowd = lower.starts_with("contributors to") || lower.ends_with("contributors");
        let address = lower.contains("://") || lower.starts_with("www.");
        !organisation && !crowd && !address
    };
    let named = |names: Vec<&str>| {
        let mut out = Vec::new();
        for name in names {
            let name = name.trim();
            let name = ["By ", "by "]
                .iter()
                .find_map(|by| name.strip_prefix(by))
                .unwrap_or(name);
            if person(name) {
                add_author(&mut out, name);
            }
        }
        out
    };
    [
        ld.authors.iter().map(String::as_str).collect(),
        m.all("author").collect(),
        m.all("article:author").collect(),
    ]
    .into_iter()
    .map(named)
    .find(|authors| !authors.is_empty())
    .unwrap_or_default()
}

/// Whether `name`'s words hold `organisation`'s (a leading "The" aside),
/// in order, ignoring case and punctuation.
fn names_in(name: &str, organisation: &str) -> bool {
    let words = |s: &str| -> Vec<String> {
        s.split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    let name = words(name);
    let mut org = words(organisation);
    if org.len() > 1 && org[0] == "the" {
        org.remove(0);
    }
    !org.is_empty() && name.windows(org.len()).any(|w| w == org.as_slice())
}

/// A date as written: `2001-09-13T05:19:30Z`, `2024/03/09`, `2024-03`,
/// `23 Oct 2017, 5:31 p.m.`, `Jan 24, 2022`, else the year it gives.
fn date_of(s: &str) -> Option<PartialDate> {
    let s = s.trim();
    if let Some(date) = written_out(s) {
        return Some(date);
    }
    let mut parts = s.split(['-', '/', 'T', ' ']);
    let year = parts
        .next()
        .filter(|y| y.len() == 4)
        .and_then(|y| y.parse::<i16>().ok());
    if let Some(year) = year {
        let mut two = || {
            parts
                .next()
                .filter(|p| (1..=2).contains(&p.len()))
                .and_then(|p| p.parse::<u8>().ok())
        };
        let month = two();
        let day = month.and_then(|_| two());
        if let Some(date) = PartialDate::new(year, month, day) {
            return Some(date);
        }
    }
    year_of(s).map(PartialDate::from_year)
}

/// A date with its month in English words, day first or month first:
/// `23 Oct 2017`, `October 23, 2017`, `Jan 24, 2022`.
fn written_out(s: &str) -> Option<PartialDate> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let words: Vec<&str> = s
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|w| !w.is_empty())
        .take(3)
        .collect();
    let [first, second, last] = words[..] else {
        return None;
    };
    let month = |w: &str| {
        let lower = w.trim_end_matches('.').to_ascii_lowercase();
        let at = MONTHS.iter().position(|name| lower.starts_with(name))?;
        u8::try_from(at + 1).ok()
    };
    let day = |w: &str| w.parse::<u8>().ok();
    let year = last.parse::<i16>().ok().filter(|_| last.len() == 4)?;
    let (month, day) = match (month(first), month(second)) {
        (Some(month), None) => (month, day(second)?),
        (None, Some(month)) => (month, day(first)?),
        _ => return None,
    };
    PartialDate::new(year, Some(month), Some(day))
}

/// A language tag tidied (`en_us` is `en-US`), or `None` when `s` isn't
/// one: the first of a list, its language two or three letters.
fn language_tag(s: &str) -> Option<String> {
    let s = s.split(',').next()?.trim().replace('_', "-");
    let mut subtags = s.split('-');
    let language = subtags.next()?;
    if !(2..=3).contains(&language.len()) || !language.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    let mut tag = language.to_ascii_lowercase();
    for sub in subtags {
        if sub.is_empty() || sub.len() > 8 || !sub.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return None;
        }
        tag.push('-');
        match sub.len() {
            2 if sub.bytes().all(|b| b.is_ascii_alphabetic()) => {
                tag.push_str(&sub.to_ascii_uppercase());
            }
            4 if sub.bytes().all(|b| b.is_ascii_alphabetic()) => {
                tag.push_str(&sub[..1].to_ascii_uppercase());
                tag.push_str(&sub[1..].to_ascii_lowercase());
            }
            _ => tag.push_str(&sub.to_ascii_lowercase()),
        }
    }
    Some(tag)
}

/// `href` resolved against `base` (an absolute address, `//host/…` or
/// `/path`), normalised; `None` unless it's then a web address.
fn resolve(href: &str, base: &str) -> Option<String> {
    let href = href.trim();
    let base = base.trim();
    let (scheme, rest) = base.split_once("://")?;
    let origin_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let starts = |prefix: &str| {
        href.get(..prefix.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(prefix))
    };
    let absolute = if starts("https://") || starts("http://") {
        href.to_owned()
    } else if let Some(path) = href.strip_prefix("//") {
        format!("{scheme}://{path}")
    } else if href.starts_with('/') {
        format!("{scheme}://{}{href}", &rest[..origin_end])
    } else {
        return None;
    };
    normalise_url(&absolute)
}

/// Query parameters that only track how a page was reached, dropped from
/// its address: `utm_*` and these.
const TRACKING: &[&str] = &[
    "fbclid", "gclid", "dclid", "gbraid", "wbraid", "msclkid", "yclid", "mc_cid", "mc_eid",
    "igshid", "_ga", "_gl",
];

/// A web address normalised as a page's key: scheme and host lower case,
/// the default port and the fragment dropped, tracking parameters
/// (`TRACKING`, `utm_*`) taken out of the query (the rest kept, in
/// order), an empty path `/`. `None` unless it's `http` or `https` with a
/// host.
pub fn normalise_url(url: &str) -> Option<String> {
    let (scheme, rest) = url.trim().split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let rest = rest.split('#').next().unwrap_or(rest);
    let authority_end = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, rest) = rest.split_at(authority_end);
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let mut host = host.to_ascii_lowercase();
    let default_port = if scheme == "https" { ":443" } else { ":80" };
    if host.ends_with(default_port) {
        host.truncate(host.len() - default_port.len());
    }
    if host.is_empty() || host.contains(char::is_whitespace) {
        return None;
    }
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let path = if path.is_empty() { "/" } else { path };
    let kept: Vec<&str> = query
        .split('&')
        .filter(|pair| {
            let name = pair
                .split('=')
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            !pair.is_empty() && !name.starts_with("utm_") && !TRACKING.contains(&name.as_str())
        })
        .collect();
    let mut out = format!("{scheme}://{host}{path}");
    if !kept.is_empty() {
        out.push('?');
        out.push_str(&kept.join("&"));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_normalise() {
        for (url, normal) in [
            (
                "HTTPS://EN.Wikipedia.org/wiki/Gene_Hackman#Early_life",
                Some("https://en.wikipedia.org/wiki/Gene_Hackman"),
            ),
            (
                "https://www.bbc.co.uk/news/articles/c1?utm_source=x&at_medium=RSS&fbclid=abc&utm_campaign=y",
                Some("https://www.bbc.co.uk/news/articles/c1?at_medium=RSS"),
            ),
            (
                "https://example.org:443?gclid=1&_ga=2",
                Some("https://example.org/"),
            ),
            (
                "http://example.org:8080/a?b=1&&c",
                Some("http://example.org:8080/a?b=1&c"),
            ),
            ("ftp://example.org/a", None),
            ("not an address", None),
            ("https:///path", None),
        ] {
            assert_eq!(normalise_url(url).as_deref(), normal, "{url}");
        }
    }

    #[test]
    fn links_resolve_against_the_address() {
        let base = "https://www.nice.org.uk/guidance/ng51?utm_source=x";
        assert_eq!(
            resolve("/guidance/ng51#tab", base).as_deref(),
            Some("https://www.nice.org.uk/guidance/ng51")
        );
        assert_eq!(
            resolve("//cdn.example.org/a", base).as_deref(),
            Some("https://cdn.example.org/a")
        );
        assert_eq!(resolve("relative/path", base), None);
        assert_eq!(resolve("/a", "no scheme"), None);
    }

    #[test]
    fn dates_as_written() {
        for (date, written) in [
            ("2001-09-13T05:19:30Z", Some("2001-09-13")),
            ("2024/03/09", Some("2024-03-09")),
            ("2024-03", Some("2024-03")),
            ("2024", Some("2024")),
            ("Jan 24, 2022", Some("2022-01-24")),
            ("23 Oct 2017, 5:31 p.m.", Some("2017-10-23")),
            ("September 13, 2001", Some("2001-09-13")),
            ("Published 2019", Some("2019")),
            ("2024-13-40", Some("2024")),
            ("soon", None),
        ] {
            assert_eq!(
                date_of(date).map(|d| d.to_string()).as_deref(),
                written,
                "{date}"
            );
        }
    }

    #[test]
    fn language_tags_tidy() {
        for (tag, tidy) in [
            ("en", Some("en")),
            ("en_us", Some("en-US")),
            ("EN-gb", Some("en-GB")),
            ("zh-hant-TW", Some("zh-Hant-TW")),
            ("fr, en", Some("fr")),
            ("english", None),
            ("", None),
            ("e1", None),
        ] {
            assert_eq!(language_tag(tag).as_deref(), tidy, "{tag:?}");
        }
    }
}
