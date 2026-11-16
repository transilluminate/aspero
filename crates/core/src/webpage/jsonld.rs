//! schema.org JSON-LD: the article a page describes, as its scripts give
//! it.
//!
//! The first `ScholarlyArticle`, `MedicalScholarlyArticle` or `Article`
//! (a type written bare, prefixed `schema:` or as a schema.org URL) at a
//! block's top level, in its `@graph`, or as a `WebPage`'s
//! `mainEntity`; articles nested deeper (related articles, citations) are
//! never read. From it: identifiers in `identifier` (a string, or a
//! `PropertyValue` whose `propertyID` names the kind), `sameAs` and `@id`;
//! `name`, else `headline` (which pages also fill with a description,
//! #118); `author` (people or organisations, objects or strings);
//! `datePublished`; `pagination`, else `pageStart` and `pageEnd`; and, up
//! its `isPartOf` chain, a `PublicationVolume`'s `volumeNumber` and a
//! `Periodical`'s `name`, never another kind's (a `WebSite`'s name is not
//! a journal).
//!
//! For a web page's record ([`web`]), the main node is read the same way
//! but of more kinds: any article, post or report, else a page.

use serde_json::{Map, Value};

use super::{Found, PageSource, add_author, ids_in, pages, tidy_value, year_of};
use crate::id::IdKind;

/// Article types read.
const ARTICLES: &[&str] = &["ScholarlyArticle", "MedicalScholarlyArticle", "Article"];

/// Scholarly article types: a page giving one is a scholarly work's.
const SCHOLARLY: &[&str] = &["ScholarlyArticle", "MedicalScholarlyArticle"];

/// What a web page's main node may be, before [`PAGES`]: articles, posts
/// and reports of any kind.
const WEB_ARTICLES: &[&str] = &[
    "Article",
    "ScholarlyArticle",
    "MedicalScholarlyArticle",
    "NewsArticle",
    "AnalysisNewsArticle",
    "BackgroundNewsArticle",
    "OpinionNewsArticle",
    "ReportageNewsArticle",
    "ReviewNewsArticle",
    "BlogPosting",
    "LiveBlogPosting",
    "SocialMediaPosting",
    "DiscussionForumPosting",
    "TechArticle",
    "Report",
];

/// Pages, for a web page's main node when no article is given.
const PAGES: &[&str] = &[
    "WebPage",
    "MedicalWebPage",
    "AboutPage",
    "FAQPage",
    "QAPage",
    "ItemPage",
    "CollectionPage",
    "ProfilePage",
];

/// The blocks that parse as JSON.
fn parsed(blocks: &[String]) -> impl Iterator<Item = Value> + '_ {
    blocks
        .iter()
        .filter_map(|block| serde_json::from_str::<Value>(unwrapped(block)).ok())
}

/// What the first article in `blocks` says.
pub(super) fn read(blocks: &[String]) -> Found {
    parsed(blocks)
        .find_map(|value| find(&value, ARTICLES).map(fields))
        .unwrap_or_default()
}

/// Whether `blocks` give a `ScholarlyArticle` or `MedicalScholarlyArticle`
/// where an article is read.
pub(super) fn scholarly(blocks: &[String]) -> bool {
    parsed(blocks).any(|value| find(&value, SCHOLARLY).is_some())
}

/// What a web page's JSON-LD says of it: from its main node (the first
/// article of any kind, else the first page, where an article is read),
/// and its website.
#[derive(Debug, Default)]
pub(super) struct WebLd {
    /// An article's `name`, else its `headline` (see [`title`]).
    pub title: Option<String>,
    /// A page's `name`, else its `headline`, when no article is given:
    /// less sure, as sites name pages by their template ("Detail").
    pub page_title: Option<String>,
    /// People only: `Person` authors, untyped ones with a given or family
    /// name, and names written as strings.
    pub authors: Vec<String>,
    /// The organisations named as authors, or as the publisher: not
    /// people, wherever else they're named.
    pub organisations: Vec<String>,
    pub published: Option<String>,
    pub updated: Option<String>,
    pub url: Option<String>,
    pub language: Option<String>,
    /// The `WebSite` the main node is part of, else the first at a
    /// block's top or in its `@graph`: its name.
    pub website: Option<String>,
    /// The main node's publisher's name.
    pub publisher: Option<String>,
}

/// What `blocks` say of the page: see [`WebLd`].
pub(super) fn web(blocks: &[String]) -> WebLd {
    let values: Vec<Value> = parsed(blocks).collect();
    let mut ld = WebLd::default();
    let article = values.iter().find_map(|v| find(v, WEB_ARTICLES));
    let main = article.or_else(|| values.iter().find_map(|v| find(v, PAGES)));
    if let Some(main) = main {
        if article.is_some() {
            ld.title = title(main);
        } else {
            ld.page_title = title(main);
        }
        for author in main.get("author").map(each).unwrap_or_default() {
            match author {
                Value::Object(node) if is_person(node) => {
                    if let Some(name) = person_name(node) {
                        add_author(&mut ld.authors, &name);
                    }
                }
                Value::Object(node) => ld.organisations.extend(node.get("name").and_then(text)),
                other => {
                    if let Some(name) = text(other) {
                        add_author(&mut ld.authors, &name);
                    }
                }
            }
        }
        ld.published = main.get("datePublished").and_then(text);
        ld.updated = main.get("dateModified").and_then(text);
        ld.url = main
            .get("url")
            .map(each)
            .and_then(|urls| urls.iter().find_map(text));
        ld.language = main.get("inLanguage").and_then(language);
        ld.publisher = main.get("publisher").map(each).and_then(|publishers| {
            publishers.iter().find_map(|p| match p {
                Value::Object(node) => node.get("name").and_then(text),
                other => text(other),
            })
        });
        ld.organisations.extend(ld.publisher.clone());
        ld.website = main.get("isPartOf").map(each).and_then(|parts| {
            parts.iter().find_map(|p| match p {
                Value::Object(node) if is_type(node, &["WebSite"]) => {
                    node.get("name").and_then(text)
                }
                _ => None,
            })
        });
    }
    ld.website = ld.website.or_else(|| {
        values
            .iter()
            .find_map(|v| find(v, &["WebSite"]))
            .and_then(|site| site.get("name").and_then(text))
    });
    ld
}

/// A script's JSON, without the CDATA or comment wrapping old pages give
/// it.
fn unwrapped(block: &str) -> &str {
    let mut s = block.trim();
    for prefix in ["//<![CDATA[", "<![CDATA[", "<!--"] {
        s = s.strip_prefix(prefix).unwrap_or(s).trim();
    }
    for suffix in ["//]]>", "]]>", "-->"] {
        s = s.strip_suffix(suffix).unwrap_or(s).trim();
    }
    s
}

/// The first node of one of `types` at `value`'s top level, in its
/// `@graph`, or as a page's `mainEntity`.
fn find<'a>(value: &'a Value, types: &[&str]) -> Option<&'a Map<String, Value>> {
    match value {
        Value::Array(items) => items.iter().find_map(|v| find(v, types)),
        Value::Object(node) => {
            if is_type(node, types) {
                return Some(node);
            }
            if let Some(graph) = node.get("@graph")
                && let Some(found) = find(graph, types)
            {
                return Some(found);
            }
            match node.get("mainEntity") {
                Some(Value::Object(main)) if is_type(main, types) => Some(main),
                Some(Value::Array(mains)) => mains.iter().find_map(|m| match m {
                    Value::Object(main) if is_type(main, types) => Some(main),
                    _ => None,
                }),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Whether `node`'s `@type` (a string or a list) is one of `types`.
fn is_type(node: &Map<String, Value>, types: &[&str]) -> bool {
    let named = |t: &Value| {
        t.as_str().is_some_and(|t| {
            let bare = t.rsplit(['/', ':', '#']).next().unwrap_or(t);
            types.contains(&bare)
        })
    };
    match node.get("@type") {
        Some(Value::Array(ts)) => ts.iter().any(named),
        Some(t) => named(t),
        None => false,
    }
}

/// A string or a number as text.
fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => tidy_value(&super::html::decode(s)),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// `value` as a list: itself, or its items.
fn each(value: &Value) -> &[Value] {
    match value {
        Value::Array(items) => items,
        other => std::slice::from_ref(other),
    }
}

/// A node's title: its `name`, which names the thing, else its
/// `headline`, which pages also fill with a description (#118: a
/// Wikipedia article's is its short description). A `name` that isn't a
/// title (an address, "Article") is passed over.
fn title(node: &Map<String, Value>) -> Option<String> {
    node.get("name")
        .and_then(text)
        .filter(|name| super::title::is_title(name, 1, 1))
        .or_else(|| node.get("headline").and_then(text))
}

/// Whether an author node is a person: typed `Person`, or untyped with a
/// given or family name.
fn is_person(node: &Map<String, Value>) -> bool {
    if node.contains_key("@type") {
        is_type(node, &["Person"])
    } else {
        node.contains_key("givenName") || node.contains_key("familyName")
    }
}

/// A person's name: `name`, else the given and family names.
fn person_name(person: &Map<String, Value>) -> Option<String> {
    person.get("name").and_then(text).or_else(|| {
        let given = person.get("givenName").and_then(text);
        let family = person.get("familyName").and_then(text)?;
        Some(match given {
            Some(given) => format!("{given} {family}"),
            None => family,
        })
    })
}

/// `inLanguage`: a tag, or a `Language`'s `alternateName` (its tag).
fn language(value: &Value) -> Option<String> {
    each(value).iter().find_map(|v| match v {
        Value::Object(node) => node.get("alternateName").and_then(text),
        other => text(other),
    })
}

fn fields(article: &Map<String, Value>) -> Found {
    let source = PageSource::JsonLd;
    let mut found = Found::default();
    for value in article.get("identifier").map(each).unwrap_or_default() {
        match value {
            Value::Object(pv) => {
                let kind = pv.get("propertyID").and_then(Value::as_str).and_then(|k| {
                    IdKind::from_prefix(k.trim()).or_else(|| IdKind::from_label(k.trim()))
                });
                if let Some(v) = pv.get("value").and_then(text) {
                    found.ids.extend(ids_in(&v, kind, source));
                }
            }
            other => {
                if let Some(v) = text(other) {
                    found.ids.extend(ids_in(&v, None, source));
                }
            }
        }
    }
    for key in ["sameAs", "@id"] {
        for value in article.get(key).map(each).unwrap_or_default() {
            if let Some(v) = value.as_str() {
                found.ids.extend(ids_in(v, None, source));
            }
        }
    }
    found.title = title(article);
    for author in article.get("author").map(each).unwrap_or_default() {
        let name = match author {
            Value::Object(person) => person_name(person),
            other => text(other),
        };
        if let Some(name) = name {
            add_author(&mut found.authors, &name);
        }
    }
    found.year = article
        .get("datePublished")
        .and_then(text)
        .and_then(|d| year_of(&d));
    found.page = article.get("pagination").and_then(text).or_else(|| {
        let start = article.get("pageStart").and_then(text);
        let end = article.get("pageEnd").and_then(text);
        pages(start.as_deref(), end.as_deref())
    });
    // Up the chain: issue, volume, periodical.
    let mut part = article.get("isPartOf");
    for _ in 0..4 {
        let Some(Value::Object(node)) = part.map(|p| each(p).first().unwrap_or(p)) else {
            break;
        };
        if is_type(node, &["PublicationVolume"]) && found.volume.is_none() {
            found.volume = node.get("volumeNumber").and_then(text);
        }
        if is_type(node, &["Periodical"]) && found.container.is_none() {
            found.container = node.get("name").and_then(text);
        }
        part = node.get("isPartOf");
    }
    found
}
