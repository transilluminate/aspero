//! A tolerant scanner for the parts of an HTML document that describe it:
//! `<meta>` tags, the `<title>` and `application/ld+json` scripts. Not a
//! parser: no tree, no error recovery beyond skipping what isn't a tag.
//! Anything (a whole page, a `<head>`, a fragment, junk) scans; what it
//! can't read as a tag is text and passed over. Also kept, for a web
//! page's record: the `<html>` tag's `lang` and the first `<link
//! rel="canonical">`'s `href`.
//!
//! Skipped: comments, `<!…>` and `<?…>` declarations, closing tags, and
//! the contents of `<script>` (other than JSON-LD), `<style>`,
//! `<textarea>` and `<template>`, so markup inside a script's strings is
//! never read as tags. Attribute values are entity-decoded.

use std::borrow::Cow;

/// Input read at most: the rest is ignored. A page's `<head>` with its
/// inline scripts is a few hundred KB (JAMA's, 145 KB).
pub const MAX_INPUT: usize = 2 * 1024 * 1024;
/// Tags read at most, then the scan stops.
pub const MAX_TAGS: usize = 20_000;
/// JSON-LD blocks kept at most.
pub const MAX_JSON_LD: usize = 32;

/// What a document says of itself.
#[derive(Debug, Default)]
pub(super) struct Markup {
    /// `<meta>` tags' `name` (or `property`, or `http-equiv`), lowercased,
    /// and `content`, decoded, in document order.
    pub metas: Vec<(String, String)>,
    /// The first `<title>`'s text, decoded.
    pub title: Option<String>,
    /// The JSON-LD scripts' contents, as written.
    pub json_ld: Vec<String>,
    /// The first `<html>` tag's `lang`, decoded.
    pub lang: Option<String>,
    /// The first `<link rel="canonical">`'s `href`, decoded, as written
    /// (it may be relative).
    pub canonical: Option<String>,
}

impl Markup {
    /// Every `content` of meta tags named `name` (lowercase), in order.
    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.metas
            .iter()
            .filter(move |(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// The first non-blank `content` of a meta tag named `name`.
    pub fn first<'a>(&'a self, name: &'a str) -> Option<&'a str> {
        self.all(name).find(|v| !v.trim().is_empty())
    }
}

/// Scans `html` (its first [`MAX_INPUT`] bytes, at most [`MAX_TAGS`] tags).
pub(super) fn read(html: &str) -> Markup {
    let html = &html[..floor_char_boundary(html, MAX_INPUT)];
    let mut markup = Markup::default();
    let mut at = 0;
    let mut tags = 0;
    while let Some(lt) = html[at..].find('<').map(|i| at + i) {
        let rest = &html[lt..];
        if rest.starts_with("<!--") {
            at = html[lt + 4..]
                .find("-->")
                .map_or(html.len(), |i| lt + 4 + i + 3);
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") || rest.starts_with("</") {
            at = html[lt..].find('>').map_or(html.len(), |i| lt + i + 1);
            continue;
        }
        let Some(tag) = Tag::read(html, lt + 1) else {
            at = lt + 1;
            continue;
        };
        tags += 1;
        if tags > MAX_TAGS {
            break;
        }
        at = tag.end;
        match tag.name.as_str() {
            "meta" => {
                let name = tag
                    .attr("name")
                    .or_else(|| tag.attr("property"))
                    .or_else(|| tag.attr("http-equiv"));
                if let (Some(name), Some(content)) = (name, tag.attr("content")) {
                    markup
                        .metas
                        .push((name.trim().to_ascii_lowercase(), content.into_owned()));
                }
            }
            "title" => {
                let (text, end) = raw_text(html, at, "title");
                if markup.title.is_none() {
                    markup.title = Some(decode(text).into_owned());
                }
                at = end;
            }
            "script" => {
                let json = tag
                    .attr("type")
                    .is_some_and(|t| t.to_ascii_lowercase().contains("ld+json"));
                let (text, end) = raw_text(html, at, "script");
                if json && markup.json_ld.len() < MAX_JSON_LD {
                    markup.json_ld.push(text.to_owned());
                }
                at = end;
            }
            "html" if markup.lang.is_none() => markup.lang = tag.attr("lang").map(Cow::into_owned),
            "link" if markup.canonical.is_none() => {
                let canonical = tag.attr("rel").is_some_and(|rel| {
                    rel.split_ascii_whitespace()
                        .any(|r| r.eq_ignore_ascii_case("canonical"))
                });
                if canonical {
                    markup.canonical = tag.attr("href").map(Cow::into_owned);
                }
            }
            "style" | "textarea" | "template" => at = raw_text(html, at, &tag.name).1,
            _ => {}
        }
    }
    markup
}

/// One start tag: its name (lowercased), attributes as written, and where
/// it ends.
struct Tag<'a> {
    name: String,
    attrs: Vec<(String, &'a str)>,
    end: usize,
}

impl<'a> Tag<'a> {
    /// The tag starting at `html[start..]`, just after its `<`; `None` if
    /// what follows isn't a tag name.
    fn read(html: &'a str, start: usize) -> Option<Self> {
        let bytes = html.as_bytes();
        if !bytes.get(start)?.is_ascii_alphabetic() {
            return None;
        }
        let name_char = |b: u8| b.is_ascii_alphanumeric() || b == b'-' || b == b':';
        let mut i = start;
        while i < bytes.len() && name_char(bytes[i]) {
            i += 1;
        }
        let name = html[start..i].to_ascii_lowercase();
        let mut attrs = Vec::new();
        loop {
            while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b'/') {
                i += 1;
            }
            match bytes.get(i) {
                None => break,
                Some(b'>') => {
                    i += 1;
                    break;
                }
                Some(_) => {}
            }
            let name_start = i;
            while i < bytes.len()
                && !bytes[i].is_ascii_whitespace()
                && !matches!(bytes[i], b'=' | b'>' | b'/')
            {
                i += 1;
            }
            // A stray `=` or quote where a name should be: step over it.
            if i == name_start {
                i += 1;
                continue;
            }
            let attr = html[name_start..i].to_ascii_lowercase();
            let mut j = i;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if bytes.get(j) != Some(&b'=') {
                attrs.push((attr, ""));
                continue;
            }
            j += 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            let (value, next) = if let Some(&q @ (b'"' | b'\'')) = bytes.get(j) {
                let close = html[j + 1..]
                    .find(q as char)
                    .map_or(html.len(), |k| j + 1 + k);
                (&html[j + 1..close], (close + 1).min(html.len()))
            } else {
                let mut k = j;
                while k < bytes.len() && !bytes[k].is_ascii_whitespace() && bytes[k] != b'>' {
                    k += 1;
                }
                (&html[j..k], k)
            };
            attrs.push((attr, value));
            i = next;
        }
        Some(Tag {
            name,
            attrs,
            end: i,
        })
    }

    /// The first attribute named `name` (lowercase), decoded.
    fn attr(&self, name: &str) -> Option<Cow<'a, str>> {
        self.attrs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| decode(v))
    }
}

/// The text of a raw-text element from `start` to its closing tag (any
/// case), and where scanning goes on: after the closing tag, or at the
/// end when there's none.
fn raw_text<'a>(html: &'a str, start: usize, name: &str) -> (&'a str, usize) {
    let close = format!("</{name}");
    match find_ignoring_case(&html.as_bytes()[start..], close.as_bytes()) {
        Some(i) => {
            let end = start + i;
            let after = html[end..].find('>').map_or(html.len(), |k| end + k + 1);
            (&html[start..end], after)
        }
        None => (&html[start..], html.len()),
    }
}

/// Where `needle` (ASCII, lowercase) first occurs in `haystack`, ignoring
/// ASCII case.
fn find_ignoring_case(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    let first = needle[0];
    let mut from = 0;
    while let Some(i) = haystack[from..]
        .iter()
        .position(|b| b.to_ascii_lowercase() == first)
    {
        let at = from + i;
        let end = at + needle.len();
        if end > haystack.len() {
            return None;
        }
        if haystack[at..end].eq_ignore_ascii_case(needle) {
            return Some(at);
        }
        from = at + 1;
    }
    None
}

/// The largest char boundary in `s` at or below `max`.
pub(super) fn floor_char_boundary(s: &str, max: usize) -> usize {
    if max >= s.len() {
        return s.len();
    }
    let mut at = max;
    while !s.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// Named character references decoded; others are left as written.
const ENTITIES: &[(&str, char)] = &[
    ("amp", '&'),
    ("lt", '<'),
    ("gt", '>'),
    ("quot", '"'),
    ("apos", '\''),
    ("nbsp", '\u{a0}'),
    ("ndash", '–'),
    ("mdash", '—'),
    ("lsquo", '‘'),
    ("rsquo", '’'),
    ("ldquo", '“'),
    ("rdquo", '”'),
    ("hellip", '…'),
    ("middot", '·'),
    ("bull", '•'),
    ("copy", '©'),
    ("reg", '®'),
    ("trade", '™'),
    ("deg", '°'),
    ("times", '×'),
    ("minus", '−'),
    ("alpha", 'α'),
    ("beta", 'β'),
    ("gamma", 'γ'),
    ("delta", 'δ'),
    ("kappa", 'κ'),
    ("mu", 'μ'),
    ("aacute", 'á'),
    ("agrave", 'à'),
    ("acirc", 'â'),
    ("atilde", 'ã'),
    ("auml", 'ä'),
    ("aring", 'å'),
    ("aelig", 'æ'),
    ("ccedil", 'ç'),
    ("eacute", 'é'),
    ("egrave", 'è'),
    ("ecirc", 'ê'),
    ("euml", 'ë'),
    ("iacute", 'í'),
    ("igrave", 'ì'),
    ("icirc", 'î'),
    ("iuml", 'ï'),
    ("ntilde", 'ñ'),
    ("oacute", 'ó'),
    ("ograve", 'ò'),
    ("ocirc", 'ô'),
    ("otilde", 'õ'),
    ("ouml", 'ö'),
    ("oslash", 'ø'),
    ("uacute", 'ú'),
    ("ugrave", 'ù'),
    ("ucirc", 'û'),
    ("uuml", 'ü'),
    ("yacute", 'ý'),
    ("szlig", 'ß'),
    ("Aacute", 'Á'),
    ("Agrave", 'À'),
    ("Auml", 'Ä'),
    ("Aring", 'Å'),
    ("Ccedil", 'Ç'),
    ("Eacute", 'É'),
    ("Oacute", 'Ó'),
    ("Ouml", 'Ö'),
    ("Oslash", 'Ø'),
    ("Uuml", 'Ü'),
];

/// `s` with character references decoded: `&amp;`, `&#39;`, `&#x2019;`
/// and the named ones in [`ENTITIES`]. Unknown or malformed references
/// stay as written.
pub(super) fn decode(s: &str) -> Cow<'_, str> {
    if !s.contains('&') {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        match after.find(';').filter(|&semi| semi <= 10) {
            Some(semi) if reference(&after[..semi]).is_some() => {
                out.push(reference(&after[..semi]).unwrap_or('&'));
                rest = &after[semi + 1..];
            }
            _ => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// The character a reference's name (between `&` and `;`) stands for.
fn reference(name: &str) -> Option<char> {
    if let Some(number) = name.strip_prefix('#') {
        let code = match number.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => number.parse().ok()?,
        };
        return char::from_u32(code).filter(|&c| c != '\0');
    }
    ENTITIES
        .iter()
        .find(|(n, _)| *n == name)
        .or_else(|| ENTITIES.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)))
        .map(|&(_, c)| c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_decode() {
        assert_eq!(
            decode("Smith &amp; Wesson&#39;s &quot;x&quot; &#x2019; &ndash; &eacute;"),
            "Smith & Wesson's \"x\" ’ – é"
        );
        // Unknown, unterminated and malformed references stay.
        assert_eq!(
            decode("a &bogus; b & c &#xZZ; &amp"),
            "a &bogus; b & c &#xZZ; &amp"
        );
        assert_eq!(decode("&#0;"), "&#0;");
    }

    #[test]
    fn metas_in_any_quoting_and_case() {
        let m = read(
            "<META NAME=\"Citation_DOI\" CONTENT=\"10.1/a\">\
             <meta content='Deep &amp; wide' name='citation_title'/>\
             <meta name=citation_volume content=12>\
             <meta property=\"og:title\" content=\"OG\">",
        );
        assert_eq!(
            m.metas,
            [
                ("citation_doi".into(), "10.1/a".into()),
                ("citation_title".into(), "Deep & wide".into()),
                ("citation_volume".into(), "12".into()),
                ("og:title".into(), "OG".into()),
            ]
        );
    }

    #[test]
    fn the_language_and_canonical_link() {
        let m = read(
            "<HTML class=\"client-nojs\" LANG=\"en-GB\" dir=ltr><head>\
             <link rel=\"stylesheet\" href=\"/a.css\">\
             <link rel=\"Canonical\" href=\"https://example.org/a?x=1&amp;y=2\">\
             <link rel=canonical href=/second>\
             <meta http-equiv=\"Content-Language\" content=\"fr\">",
        );
        assert_eq!(m.lang.as_deref(), Some("en-GB"));
        assert_eq!(
            m.canonical.as_deref(),
            Some("https://example.org/a?x=1&y=2")
        );
        assert_eq!(m.metas, [("content-language".into(), "fr".into())]);
    }

    #[test]
    fn scripts_comments_and_styles_hide_markup() {
        let m = read(
            "<script>var s = '<meta name=\"citation_doi\" content=\"10.9/fake\">';</script>\
             <!-- <meta name=\"citation_doi\" content=\"10.9/comment\"> -->\
             <style>/* <meta name=\"citation_doi\" content=\"10.9/style\"> */</style>\
             <SCRIPT type=\"application/ld+json\">{\"doi\": \"<meta>\"}</SCRIPT >\
             <meta name=\"citation_doi\" content=\"10.1/real\">",
        );
        assert_eq!(m.metas, [("citation_doi".into(), "10.1/real".into())]);
        assert_eq!(m.json_ld, [r#"{"doi": "<meta>"}"#]);
    }

    #[test]
    fn junk_and_truncated_markup_scans() {
        for junk in [
            "",
            "<",
            "<<<>>>",
            "< meta name=x>",
            "<meta name=\"citation_title\" content=\"unterminated",
            "<script>never closed <meta name=a content=b>",
            "<!-- never closed",
            "<title>no end",
            "\u{0}\u{fffd}<é>",
        ] {
            let _ = read(junk);
        }
        let m = read("<title>no end");
        assert_eq!(m.title.as_deref(), Some("no end"));
        // A script never closed hides the rest, as in a browser.
        assert!(read("<script>x <meta name=a content=b>").metas.is_empty());
    }

    #[test]
    fn reading_stops_at_the_cap() {
        let meta = "<meta name=\"citation_title\" content=\"x\">";
        let html = meta.repeat(MAX_TAGS + 10);
        assert_eq!(read(&html).metas.len(), MAX_TAGS);
        let mut big = "é".repeat(MAX_INPUT / 2 + 1);
        big.push_str(meta);
        assert!(read(&big).metas.is_empty());
    }
}
