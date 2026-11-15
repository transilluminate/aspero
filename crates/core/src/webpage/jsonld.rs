//! schema.org JSON-LD: the article a page describes, as its scripts give
//! it.
//!
//! The first `ScholarlyArticle`, `MedicalScholarlyArticle` or `Article`
//! (a type written bare, prefixed `schema:` or as a schema.org URL) at a
//! block's top level, in its `@graph`, or as a `WebPage`'s
//! `mainEntity`; articles nested deeper (related articles, citations) are
//! never read. From it: identifiers in `identifier` (a string, or a
//! `PropertyValue` whose `propertyID` names the kind), `sameAs` and `@id`;
//! `headline`, else `name`; `author` (people or organisations, objects or
//! strings); `datePublished`; `pagination`, else `pageStart` and
//! `pageEnd`; and, up its `isPartOf` chain, a `PublicationVolume`'s
//! `volumeNumber` and a `Periodical`'s `name`, never another kind's (a
//! `WebSite`'s name is not a journal).

use serde_json::{Map, Value};

use super::{Found, PageSource, add_author, ids_in, pages, tidy_value, year_of};
use crate::id::IdKind;

/// Article types read.
const ARTICLES: &[&str] = &["ScholarlyArticle", "MedicalScholarlyArticle", "Article"];

/// What the first article in `blocks` says.
pub(super) fn read(blocks: &[String]) -> Found {
    for block in blocks {
        let Ok(value) = serde_json::from_str::<Value>(unwrapped(block)) else {
            continue;
        };
        if let Some(article) = article(&value) {
            return fields(article);
        }
    }
    Found::default()
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

/// The first article at `value`'s top level, in its `@graph`, or as a
/// page's `mainEntity`.
fn article(value: &Value) -> Option<&Map<String, Value>> {
    match value {
        Value::Array(items) => items.iter().find_map(article),
        Value::Object(node) => {
            if is_type(node, ARTICLES) {
                return Some(node);
            }
            if let Some(graph) = node.get("@graph")
                && let Some(found) = article(graph)
            {
                return Some(found);
            }
            match node.get("mainEntity") {
                Some(Value::Object(main)) if is_type(main, ARTICLES) => Some(main),
                Some(Value::Array(mains)) => mains.iter().find_map(|m| match m {
                    Value::Object(main) if is_type(main, ARTICLES) => Some(main),
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
    found.title = ["headline", "name"]
        .iter()
        .find_map(|k| article.get(*k).and_then(text));
    for author in article.get("author").map(each).unwrap_or_default() {
        let name = match author {
            Value::Object(person) => person.get("name").and_then(text).or_else(|| {
                let given = person.get("givenName").and_then(text);
                let family = person.get("familyName").and_then(text)?;
                Some(match given {
                    Some(given) => format!("{given} {family}"),
                    None => family,
                })
            }),
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
