//! References pasted in a tagged format, read as structured claims:
//! Wikipedia's citation templates (`{{cite journal |last1=… |title=…}}`),
//! an RIS record (`TY  - JOUR  AU  - …`, its lines run together or not)
//! and an EndNote record (`%0 Journal Article %A … %T …`).
//!
//! Each names its fields, so a reader takes them as given: title,
//! authors, year, journal, volume, first page, publisher and identifiers, as the
//! bibliography readers (`aspero-bib`) take a file's entries. Free text's
//! readings (a byline before the title, a journal before the locator)
//! would misread the tags: "Journal Article" after `%0` is no author's
//! name.

use crate::id::WorkId;
use crate::matching::{Claim, find_ids};

/// The claim a tagged reference makes, or `None` if `text` is none.
pub fn claim(text: &str) -> Option<Claim> {
    let text = text.trim();
    let fields = if text.starts_with("{{") {
        template(text)?
    } else if text.starts_with("TY ") {
        tags(text, ris_tag).map(|f| ris(&f))?
    } else if text.starts_with("%0 ") || text.starts_with("%A ") {
        tags(text, endnote_tag).map(|f| endnote(&f))?
    } else {
        return None;
    };
    let claim = fields.into_claim();
    (!claim.is_empty()).then_some(claim)
}

/// What a tagged reference gives, before it's a claim.
#[derive(Default)]
struct Fields {
    title: Option<String>,
    authors: Vec<String>,
    year: Option<i16>,
    container: Option<String>,
    volume: Option<String>,
    page: Option<String>,
    publisher: Option<String>,
    /// Identifiers as `kind:value`, or text to scan (a URL).
    ids: Vec<String>,
    links: Vec<String>,
}

impl Fields {
    fn into_claim(self) -> Claim {
        let mut ids: Vec<WorkId> = Vec::new();
        let parsed = self
            .ids
            .iter()
            .filter_map(|raw| raw.parse::<WorkId>().ok())
            .chain(self.links.iter().flat_map(|l| find_ids(l)));
        for id in parsed {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        Claim {
            title: self.title,
            authors: self.authors,
            year: self.year,
            container: self.container,
            volume: self.volume,
            page: self.page,
            publisher: self.publisher,
            ids,
            ..Claim::default()
        }
    }
}

/// The first four-digit run: "2015/05/27/" → 2015, "June 1998" → 1998.
fn first_year(s: &str) -> Option<i16> {
    s.as_bytes()
        .windows(4)
        .enumerate()
        .find(|(i, w)| {
            w.iter().all(u8::is_ascii_digit)
                && !s.as_bytes().get(i + 4).is_some_and(u8::is_ascii_digit)
                && !(*i > 0 && s.as_bytes()[i - 1].is_ascii_digit())
        })
        .and_then(|(_, w)| std::str::from_utf8(w).ok()?.parse().ok())
}

fn some(value: &str) -> Option<String> {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    (!value.is_empty()).then_some(value)
}

/// A start and end page as one range: "39" and "43" → "39-43".
fn pages(start: Option<String>, end: Option<String>) -> Option<String> {
    match (start, end) {
        (Some(start), Some(end)) if !start.contains(['-', '–']) => Some(format!("{start}-{end}")),
        (start, _) => start,
    }
}

// ----- Wikipedia's citation templates (CS1) -----

/// Template names read: `cite journal`, `cite book`, `Cite web`,
/// `citation`, …
fn template_name(name: &str) -> bool {
    let name = name.trim().to_ascii_lowercase();
    name == "citation" || name.starts_with("cite ") || name.starts_with("vcite ")
}

/// A CS1 template's fields: `{{cite journal |last1=Yang |first1=Xiaobo
/// |title=… |journal=… |year=2020 |volume=8 |pages=475–481 |doi=…}}`.
/// Numbered names (`last2`, `author3`) keep their order; `vauthors` is a
/// Vancouver list ("Smith AB, Jones C"); `display-authors` and other
/// presentation is ignored. A book's `chapter` is the title cited, the
/// book its container.
fn template(text: &str) -> Option<Fields> {
    let inner = text.strip_prefix("{{")?;
    let inner = inner.trim_end().strip_suffix("}}").unwrap_or(inner);
    let mut parts = split_params(inner).into_iter();
    let name = parts.next()?;
    if !template_name(&name) {
        return None;
    }
    let mut params: Vec<(String, String)> = Vec::new();
    for part in parts {
        if let Some((key, value)) = part.split_once('=') {
            let value = unmarked_wiki(value);
            if !value.is_empty() {
                params.push((key.trim().to_ascii_lowercase(), value));
            }
        }
    }
    let get = |names: &[&str]| {
        names.iter().find_map(|n| {
            params
                .iter()
                .find(|(k, _)| k == n)
                .and_then(|(_, v)| some(v))
        })
    };
    let mut fields = Fields::default();
    let chapter = get(&["chapter", "contribution", "entry"]);
    let title = get(&["title", "script-title", "trans-title"]);
    let container = get(&[
        "journal",
        "work",
        "periodical",
        "magazine",
        "newspaper",
        "website",
        "conference",
        "book-title",
    ]);
    (fields.title, fields.container) = match chapter {
        Some(chapter) => (Some(chapter), title.or(container)),
        None => (title, container),
    };
    fields.authors = template_authors(&params);
    fields.year = get(&["year", "date", "publication-date"]).and_then(|y| first_year(&y));
    fields.volume = get(&["volume"]);
    fields.page = get(&["pages", "page", "at"]);
    fields.publisher = get(&["publisher"]);
    for (key, kind) in [
        ("doi", "doi"),
        ("pmid", "pmid"),
        ("arxiv", "arxiv"),
        ("eprint", "arxiv"),
        ("isbn", "isbn"),
    ] {
        if let Some(v) = get(&[key]) {
            fields.ids.push(format!("{kind}:{v}"));
        }
    }
    if let Some(pmc) = get(&["pmc"]) {
        let pmc = pmc.trim_start_matches("PMC");
        fields.ids.push(format!("pmcid:PMC{pmc}"));
    }
    fields.links.extend(get(&["url"]));
    Some(fields)
}

/// A template's parts between its top-level `|`s: not those inside a
/// nested template (`{{!}}`, `{{doi|…}}`) or a link (`[[a|b]]`).
fn split_params(inner: &str) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut depth = 0usize;
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' | '[' if chars.peek() == Some(&c) => {
                chars.next();
                depth += 1;
                parts.last_mut().expect("one").extend([c, c]);
            }
            '}' | ']' if chars.peek() == Some(&c) => {
                chars.next();
                depth = depth.saturating_sub(1);
                parts.last_mut().expect("one").extend([c, c]);
            }
            '|' if depth == 0 => parts.push(String::new()),
            c => parts.last_mut().expect("one").push(c),
        }
    }
    parts
}

/// A value without its wiki markup: `[[Nature (journal)|Nature]]` →
/// "Nature", `''Science''` → "Science", `{{!}}` → "|", `&ndash;` → "–".
fn unmarked_wiki(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(open) = rest.find("[[") {
        out.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        let Some(close) = after.find("]]") else {
            rest = after;
            break;
        };
        let link = &after[..close];
        out.push_str(link.rsplit('|').next().unwrap_or(link));
        rest = &after[close + 2..];
    }
    out.push_str(rest);
    out.replace("'''", "")
        .replace("''", "")
        .replace("{{!}}", "|")
        .replace("&ndash;", "–")
        .replace("&mdash;", "—")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .trim()
        .to_owned()
}

/// A template's numbered author: its number, family and given names.
type Numbered = (usize, Option<String>, Option<String>);

/// The author numbered `n`, added if not yet seen.
fn slot(people: &mut Vec<Numbered>, n: usize) -> &mut Numbered {
    let i = if let Some(i) = people.iter().position(|(m, _, _)| *m == n) {
        i
    } else {
        people.push((n, None, None));
        people.len() - 1
    };
    &mut people[i]
}

/// The authors a template lists, in their numbered order: `lastN` with
/// `firstN` ("Yang, Xiaobo"), `authorN` as written, or `vauthors`.
fn template_authors(params: &[(String, String)]) -> Vec<String> {
    let numbered = |key: &str, stems: &[&str]| -> Option<usize> {
        stems.iter().find_map(|stem| {
            let n = key.strip_prefix(stem)?;
            if n.is_empty() {
                Some(1)
            } else {
                n.parse().ok()
            }
        })
    };
    let mut people: Vec<Numbered> = Vec::new();
    let mut listed: Vec<String> = Vec::new();
    for (key, value) in params {
        if key == "vauthors" || key == "authors" {
            listed.extend(value.split([',', ';']).filter_map(some));
        } else if let Some(n) = numbered(key, &["last", "surname", "author"]) {
            slot(&mut people, n).1 = Some(value.clone());
        } else if let Some(n) = numbered(key, &["first", "given"]) {
            slot(&mut people, n).2 = Some(value.clone());
        }
    }
    people.sort_by_key(|(n, _, _)| *n);
    let named = people
        .into_iter()
        .filter_map(|(_, last, first)| match (last, first) {
            (Some(last), Some(first)) => Some(format!("{last}, {first}")),
            (Some(last), None) => Some(last),
            (None, _) => None,
        });
    named.chain(listed).collect()
}

// ----- RIS and EndNote -----

/// The tagged fields of a record written on one line or several: each
/// tag where `tag_at` finds one, its value up to the next.
fn tags(
    text: &str,
    tag_at: fn(&str, usize) -> Option<(String, usize)>,
) -> Option<Vec<(String, String)>> {
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut at = 0;
    let mut open: Option<(String, usize)> = None;
    while at < text.len() {
        let boundary = at == 0 || text[..at].ends_with(char::is_whitespace);
        if boundary && let Some((tag, start)) = tag_at(text, at) {
            if let Some((t, from)) = open.take() {
                fields.push((t, text[from..at].trim().to_owned()));
            }
            open = Some((tag, start));
            at = start;
            continue;
        }
        at += text[at..].chars().next().map_or(1, char::len_utf8);
    }
    if let Some((t, from)) = open {
        fields.push((t, text[from..].trim().to_owned()));
    }
    (fields.len() >= 2).then_some(fields)
}

/// RIS tags read: those the bibliography reader takes, and the common
/// others, so their values end where they do.
const RIS_TAGS: &[&str] = &[
    "TY", "AU", "A1", "A2", "A3", "A4", "TI", "T1", "T2", "T3", "CT", "BT", "JO", "JF", "JA", "J1",
    "J2", "PY", "Y1", "Y2", "DA", "VL", "IS", "SP", "EP", "DO", "SN", "UR", "L1", "L2", "L4", "PB",
    "CY", "ER", "AB", "N1", "N2", "KW", "ID", "ET", "M1", "M3", "LA", "ST", "AD", "C1", "DB", "DP",
    "AN", "U1", "U2",
];

/// An RIS tag at `at` ("AU  - "), and where its value starts.
fn ris_tag(text: &str, at: usize) -> Option<(String, usize)> {
    let rest = &text[at..];
    let tag = rest.get(..2)?;
    if !RIS_TAGS.contains(&tag) {
        return None;
    }
    let after = &rest[2..];
    let spaced = after.trim_start_matches(' ');
    let value = spaced.strip_prefix('-')?;
    let ok = spaced.len() < after.len() && (value.is_empty() || value.starts_with(' '));
    ok.then(|| (tag.to_owned(), text.len() - value.len()))
}

fn ris(fields: &[(String, String)]) -> Fields {
    let first = |tags: &[&str]| {
        tags.iter()
            .find_map(|t| fields.iter().find(|(k, v)| k == t && !v.is_empty()))
            .and_then(|(_, v)| some(v))
    };
    let all = |tags: &[&str]| -> Vec<String> {
        fields
            .iter()
            .filter(|(k, v)| tags.contains(&k.as_str()) && !v.is_empty())
            .filter_map(|(_, v)| some(v))
            .collect()
    };
    let mut out = Fields {
        title: first(&["TI", "T1", "CT", "BT"]),
        authors: all(&["AU", "A1"]),
        year: first(&["PY", "Y1", "DA"]).and_then(|y| first_year(&y)),
        container: first(&["T2", "JO", "JF", "JA", "J2"]),
        volume: first(&["VL"]),
        page: pages(first(&["SP"]), first(&["EP"])),
        publisher: first(&["PB"]),
        ..Fields::default()
    };
    out.ids
        .extend(all(&["DO"]).into_iter().map(|d| format!("doi:{d}")));
    // `SN` holds a book's ISBN or a journal's ISSN: an ISBN is one.
    out.ids
        .extend(all(&["SN"]).into_iter().map(|n| format!("isbn:{n}")));
    out.links.extend(all(&["UR", "L1", "L2"]));
    out
}

/// An EndNote tag at `at` ("%A "), and where its value starts.
fn endnote_tag(text: &str, at: usize) -> Option<(String, usize)> {
    let rest = text[at..].strip_prefix('%')?;
    let tag = rest.chars().next()?;
    let value = rest[tag.len_utf8()..].strip_prefix(' ')?;
    (tag.is_ascii_alphanumeric() || "@!#$&()*+~^".contains(tag))
        .then(|| (tag.to_string(), text.len() - value.len()))
}

/// EndNote's fields: `%A` each author, `%T` the title, `%J` the journal
/// (`%B` the book or proceedings), `%D` the year, `%V`, `%P`, `%I` the
/// publisher, `%R` the DOI, `%@` the ISBN, `%U` a link.
fn endnote(fields: &[(String, String)]) -> Fields {
    let first = |tag: &str| {
        fields
            .iter()
            .find(|(k, v)| k == tag && !v.is_empty())
            .and_then(|(_, v)| some(v))
    };
    let mut out = Fields {
        title: first("T"),
        authors: fields
            .iter()
            .filter(|(k, _)| k == "A")
            .filter_map(|(_, v)| some(v))
            .collect(),
        year: first("D").and_then(|y| first_year(&y)),
        container: first("J").or_else(|| first("B")),
        volume: first("V"),
        page: first("P"),
        publisher: first("I"),
        ..Fields::default()
    };
    if let Some(doi) = first("R") {
        out.links.push(doi);
    }
    if let Some(isbn) = first("@") {
        out.ids.push(format!("isbn:{isbn}"));
    }
    out.links.extend(first("U"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wikipedia_templates_are_read_as_their_fields() {
        let c = claim(
            "{{cite journal |last1=Yang |first1=Xiaobo |last2=Yu |first2=Yuan |display-authors=etal \
             |title=Clinical course and outcomes of critically ill patients |journal=[[The Lancet \
             Respiratory Medicine|Lancet Respir Med]] |year=2020 |volume=8 |issue=5 \
             |pages=475&ndash;481 |doi=10.1016/s2213-2600(20)30079-5 |pmid=32105632 |pmc=7102538}}",
        )
        .unwrap();
        assert_eq!(
            c.title.as_deref(),
            Some("Clinical course and outcomes of critically ill patients")
        );
        assert_eq!(c.authors, ["Yang, Xiaobo", "Yu, Yuan"]);
        assert_eq!(c.year, Some(2020));
        assert_eq!(c.container.as_deref(), Some("Lancet Respir Med"));
        assert_eq!(c.volume.as_deref(), Some("8"));
        assert_eq!(c.page.as_deref(), Some("475–481"));
        let ids: Vec<String> = c.ids.iter().map(ToString::to_string).collect();
        assert_eq!(
            ids,
            [
                "doi:10.1016/s2213-2600(20)30079-5",
                "pmid:32105632",
                "pmcid:PMC7102538"
            ]
        );
        assert!(c.text.is_none(), "the tags aren't free text");
    }

    #[test]
    fn templates_for_books_chapters_and_preprints() {
        let book = claim(
            "{{Cite book |last1=Keynes |first1=John Maynard |title=The general theory of \
             employment, interest and money |publisher=Macmillan |year=1936 |isbn=978-0-262-03561-3}}",
        )
        .unwrap();
        assert_eq!(book.authors, ["Keynes, John Maynard"]);
        assert_eq!(book.year, Some(1936));
        assert!(book.container.is_none(), "a publisher is no container");
        assert_eq!(book.publisher.as_deref(), Some("Macmillan"));
        assert_eq!(book.ids[0].to_string(), "isbn:9780262035613");

        let chapter = claim(
            "{{cite book |author=D. Sacerdote |chapter=Peer effects in education |title=Handbook \
             of the Economics of Education |date=June 2011 |vauthors=Hanushek EA, Machin S}}",
        )
        .unwrap();
        assert_eq!(chapter.title.as_deref(), Some("Peer effects in education"));
        assert_eq!(
            chapter.container.as_deref(),
            Some("Handbook of the Economics of Education")
        );
        assert_eq!(chapter.authors, ["D. Sacerdote", "Hanushek EA", "Machin S"]);
        assert_eq!(chapter.year, Some(2011));

        let arxiv = claim("{{cite arXiv |last=Vaswani |title=Attention Is All You Need |eprint=1706.03762 |class=cs.CL |year=2017}}").unwrap();
        assert_eq!(arxiv.ids[0].to_string(), "arxiv:1706.03762");
        // Not a citation template, or nothing in it.
        assert!(claim("{{Infobox journal |title=Nature}}").is_none());
        assert!(claim("{{cite journal}}").is_none());
    }

    #[test]
    fn ris_and_endnote_records_on_one_line_are_read_as_their_fields() {
        let ris = claim(
            "TY  - JOUR  AU  - Choo, Qui-Lim  AU  - Kuo, George  TI  - Isolation of a cDNA clone \
             derived from a blood-borne non-A, non-B viral hepatitis genome  T2  - European Heart \
             Journal  PY  - 1989  VL  - 244  IS  - 4902  SP  - 359  EP  - 362  SN  - 0036-8075  \
             DO  - 10.1126/science.2523562  ER  -",
        )
        .unwrap();
        assert_eq!(ris.authors, ["Choo, Qui-Lim", "Kuo, George"]);
        assert_eq!(ris.container.as_deref(), Some("European Heart Journal"));
        assert_eq!(ris.page.as_deref(), Some("359-362"));
        let ids: Vec<String> = ris.ids.iter().map(ToString::to_string).collect();
        assert_eq!(ids, ["doi:10.1126/science.2523562"], "an ISSN is no ISBN");
        let book = claim(
            "TY  - BOOK  TI  - Deep Learning  PB  - MIT Press  SN  - 978-0-262-03561-3  ER  -",
        )
        .unwrap();
        assert_eq!(book.ids[0].to_string(), "isbn:9780262035613");
        assert_eq!(book.publisher.as_deref(), Some("MIT Press"));
        let endnote_book =
            claim("%0 Book %A Goodfellow, Ian %T Deep Learning %I MIT Press %D 2016").unwrap();
        assert_eq!(endnote_book.publisher.as_deref(), Some("MIT Press"));
        // Its lines kept, too.
        let lines =
            claim("TY  - BOOK\nAU  - 西田幾多郎\nTI  - 善の研究\nPY  - 1911\nER  - ").unwrap();
        assert_eq!(lines.title.as_deref(), Some("善の研究"));
        assert_eq!(lines.year, Some(1911));

        let endnote = claim(
            "%0 Journal Article %A Ryu, Kihyun %A Kim, A %T Microbiome and Digestive System \
             Diseases: A Diagnostic Approach %J The Korean Journal of Medicine %V 101 %N 1 \
             %P 39-43 %D 2026 %R 10.3904/kjm.2026.101.1.39",
        )
        .unwrap();
        assert_eq!(endnote.authors, ["Ryu, Kihyun", "Kim, A"]);
        assert_eq!(
            endnote.title.as_deref(),
            Some("Microbiome and Digestive System Diseases: A Diagnostic Approach")
        );
        assert_eq!(
            endnote.container.as_deref(),
            Some("The Korean Journal of Medicine")
        );
        assert_eq!(
            (endnote.volume.as_deref(), endnote.page.as_deref()),
            (Some("101"), Some("39-43"))
        );
        assert_eq!(endnote.ids[0].to_string(), "doi:10.3904/kjm.2026.101.1.39");

        // Free text that merely starts so isn't a record.
        assert!(claim("TY Smith, A study of things. 2020.").is_none());
        assert!(claim("%0 done").is_none());
    }
}
