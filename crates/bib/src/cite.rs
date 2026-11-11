//! Works formatted in a citation style (APA, Vancouver, …), offline, by
//! the `hayagriva` CSL processor. Eight styles from the official CSL
//! repository are bundled (see `styles/NOTICE.md`); any other independent
//! `.csl` style can be loaded from its text.

use std::fmt::Write as _;

use aspero_core::{Work, WorkId, WorkType};
use hayagriva::citationberg::json::Item;
use hayagriva::citationberg::{
    IndependentStyle, Locale, LocaleCode, LocaleFile, Style as CslStyle,
};
use hayagriva::{
    BibliographyDriver, BibliographyRequest, BufWriteFormat, CitationItem, CitationRequest,
};
use serde_json::{Map, Value, json};

use crate::write::{Name, name};

/// A bundled style: `id` is how it's asked for.
#[derive(Debug, Clone, Copy)]
pub struct Bundled {
    pub id: &'static str,
    pub title: &'static str,
    xml: &'static str,
}

/// The bundled styles, most used first.
pub const BUNDLED: [Bundled; 8] = [
    Bundled {
        id: "apa",
        title: "APA 7th edition",
        xml: include_str!("../styles/apa.csl"),
    },
    Bundled {
        id: "vancouver",
        title: "Vancouver (NLM, Citing Medicine 2nd edition)",
        xml: include_str!("../styles/nlm-citation-sequence.csl"),
    },
    Bundled {
        id: "harvard",
        title: "Harvard (Cite Them Right 12th edition)",
        xml: include_str!("../styles/harvard-cite-them-right.csl"),
    },
    Bundled {
        id: "chicago",
        title: "Chicago 18th edition (author-date)",
        xml: include_str!("../styles/chicago-author-date.csl"),
    },
    Bundled {
        id: "ieee",
        title: "IEEE",
        xml: include_str!("../styles/ieee.csl"),
    },
    Bundled {
        id: "nature",
        title: "Nature",
        xml: include_str!("../styles/nature.csl"),
    },
    Bundled {
        id: "ama",
        title: "AMA 11th edition",
        xml: include_str!("../styles/american-medical-association.csl"),
    },
    Bundled {
        id: "mla",
        title: "MLA 9th edition",
        xml: include_str!("../styles/modern-language-association.csl"),
    },
];

const LOCALES: [&str; 2] = [
    include_str!("../styles/locales-en-US.xml"),
    include_str!("../styles/locales-en-GB.xml"),
];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum StyleError {
    /// A dependent style: a journal's alias for `parent`, the style to
    /// load instead (its id, e.g. `nlm-citation-sequence`).
    #[error("a dependent style; use its parent, {parent}")]
    Dependent { parent: String },
    #[error("not a CSL style: {message}")]
    Invalid { message: String },
    #[error("no bundled style {id:?}")]
    Unknown { id: String },
}

/// A citation style, ready to format works with.
#[derive(Debug)]
pub struct Style {
    csl: IndependentStyle,
}

impl Style {
    /// A bundled style by id (`apa`, `vancouver`, …; see [`BUNDLED`]).
    pub fn bundled(id: &str) -> Result<Self, StyleError> {
        let found = BUNDLED.iter().find(|b| b.id == id);
        let bundled = found.ok_or_else(|| StyleError::Unknown { id: id.into() })?;
        Style::from_xml(bundled.xml)
    }

    /// A style from a `.csl` file's text.
    pub fn from_xml(xml: &str) -> Result<Self, StyleError> {
        let xml = with_default_names(xml);
        match CslStyle::from_xml(&xml) {
            Ok(CslStyle::Independent(csl)) => Ok(Style { csl }),
            Ok(CslStyle::Dependent(d)) => Err(StyleError::Dependent {
                parent: d
                    .parent_link
                    .href
                    .rsplit('/')
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
            }),
            Err(e) => Err(StyleError::Invalid {
                message: e.to_string(),
            }),
        }
    }

    pub fn title(&self) -> &str {
        &self.csl.info.title.value
    }
}

/// Gives each `<names>` element without a `<name>` child an empty one, as
/// the CSL specification implies: hayagriva 0.10.1 renders no names
/// without it (fixed upstream in typst/hayagriva#520).
fn with_default_names(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len() + 64);
    let mut rest = xml;
    while let Some(at) = rest.find("<names") {
        let (before, from) = rest.split_at(at);
        out.push_str(before);
        let Some(end) = from.find('>') else {
            rest = from;
            break;
        };
        let open = &from[..=end];
        out.push_str(open);
        rest = &from[end + 1..];
        // `<namesake`, or a self-closing `<names/>` (inside a substitute,
        // which inherits its parent's name) needs nothing.
        let tag = open["<names".len()..].starts_with([' ', '>', '\n', '\t', '\r']);
        if !tag || open.ends_with("/>") {
            continue;
        }
        let close = rest.find("</names>").unwrap_or(rest.len());
        let body = &rest[..close];
        // Only this element's own children count, not a substitute's.
        let own = body.find("<substitute").map_or(body, |s| &body[..s]);
        if !own.contains("<name ") && !own.contains("<name>") && !own.contains("<name/") {
            out.push_str("<name/>");
        }
    }
    out.push_str(rest);
    out
}

/// Text, or HTML (`<i>`, `<b>`, small caps as `<span>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Markup {
    #[default]
    Text,
    Html,
}

/// One work in a formatted reference list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cited {
    /// The work's position in the input.
    pub index: usize,
    /// The style's number or label for it ("1.", "[1]"), when the style
    /// sets it apart from the reference.
    pub label: Option<String>,
    pub text: String,
    /// How the style cites it in running text: "(LeCun et al., 2015)",
    /// "[1]".
    pub in_text: String,
}

/// `works` as a reference list in `style`, in the style's order
/// (alphabetical, or as cited: the input order). `locale` (`en-GB`) picks
/// the language of terms like "et al."; by default, the style's own.
pub fn cite(
    works: &[(&Work, &[WorkId])],
    style: &Style,
    locale: Option<&str>,
    markup: Markup,
) -> Vec<Cited> {
    let locales: Vec<Locale> = LOCALES
        .iter()
        .map(|xml| LocaleFile::from_xml(xml).expect("bundled locale").into())
        .collect();
    let items: Vec<Item> = works
        .iter()
        .enumerate()
        .filter_map(|(i, (work, ids))| serde_json::from_value(item(i, work, ids)).ok())
        .collect();
    let mut driver = BibliographyDriver::new();
    for item in &items {
        driver.citation(CitationRequest::from_items(
            vec![CitationItem::with_entry(item)],
            &style.csl,
            &locales,
        ));
    }
    let locale = locale.map(|l| LocaleCode(l.into()));
    let rendered = driver.finish(BibliographyRequest {
        style: &style.csl,
        locale,
        locale_files: &locales,
    });
    let format = match markup {
        Markup::Text => BufWriteFormat::Plain,
        Markup::Html => BufWriteFormat::Html,
    };
    let write = |write_buf: &dyn Fn(&mut String) -> std::fmt::Result| {
        let mut s = String::new();
        let _ = write_buf(&mut s);
        tidy(s.trim())
    };
    // Citations come in input order, one per work.
    let in_text: Vec<String> = rendered
        .citations
        .iter()
        .map(|c| write(&|s| c.citation.write_buf(s, format)))
        .collect();
    let Some(bibliography) = rendered.bibliography else {
        return Vec::new();
    };
    bibliography
        .items
        .iter()
        .filter_map(|b| {
            let index: usize = b.key.parse().ok()?;
            Some(Cited {
                index,
                label: b
                    .first_field
                    .as_ref()
                    .map(|f| write(&|s| f.write_buf(s, format))),
                text: write(&|s| b.content.write_buf(s, format)),
                in_text: in_text.get(index).cloned().unwrap_or_default(),
            })
        })
        .collect()
}

/// Without spaces before `;`, `,` or `.`, or a comma before a full stop:
/// hayagriva 0.10.1 writes a delimiter even when the parts after it are
/// missing ("2015 ;521", "Preprint, arXiv,.").
fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, ';' | ',' | '.') {
            while out.ends_with(' ') {
                out.pop();
            }
        }
        if c == '.' && out.ends_with(',') {
            out.pop();
        }
        out.push(c);
    }
    out
}

/// A work as a CSL-JSON item keyed by its position.
fn item(index: usize, work: &Work, ids: &[WorkId]) -> Value {
    let mut item = Map::new();
    let mut put = |key: &str, value: Option<Value>| {
        if let Some(v) = value {
            item.insert(key.into(), v);
        }
    };
    put("id", Some(json!(index.to_string())));
    // Indexes list arXiv as a journal ("arXiv (Cornell University)"),
    // sometimes with a later version's volume and pages: cite the preprint.
    let on_arxiv = ids.iter().any(|id| matches!(id, WorkId::Arxiv(_)))
        && work
            .container_title
            .as_deref()
            .is_none_or(|c| c.to_lowercase().contains("arxiv"));
    let preprint = on_arxiv || matches!(work.work_type, Some(WorkType::Preprint));
    let kind = match &work.work_type {
        _ if preprint => "article",
        Some(WorkType::Other(_)) | None => "article",
        Some(t) => t.as_csl(),
    };
    put("type", Some(json!(kind)));
    put("title", work.title.as_ref().map(|t| json!(t)));
    let authors: Vec<Value> = work
        .authors
        .iter()
        .filter_map(|p| match name(p) {
            Name::Person { family, given } => Some(json!({"family": family, "given": given})),
            Name::Whole(whole) if whole.is_empty() => None,
            Name::Whole(whole) => Some(json!({"literal": whole})),
        })
        .collect();
    put("author", (!authors.is_empty()).then(|| json!(authors)));
    put(
        "issued",
        work.issued.map(|d| {
            let parts: Vec<i64> = [
                Some(i64::from(d.year())),
                d.month().map(i64::from),
                d.day().map(i64::from),
            ]
            .into_iter()
            .map_while(|p| p)
            .collect();
            json!({"date-parts": [parts]})
        }),
    );
    if !on_arxiv {
        let text = |v: &Option<String>| v.as_ref().map(|v| json!(v));
        put("container-title", text(&work.container_title));
        put("volume", text(&work.volume));
        put("issue", text(&work.issue));
        put("page", text(&work.page));
        put("publisher", text(&work.publisher));
    }
    put("URL", work.url.as_ref().map(|v| json!(v)));
    for id in ids {
        match id {
            WorkId::Doi(doi) if !item.contains_key("DOI") => {
                item.insert("DOI".into(), json!(doi.to_string()));
            }
            WorkId::Isbn(isbn) if !item.contains_key("ISBN") => {
                item.insert("ISBN".into(), json!(isbn.to_string()));
            }
            // A preprint's repository and number, as styles print them.
            WorkId::Arxiv(arxiv) if preprint => {
                item.entry("publisher").or_insert(json!("arXiv"));
                item.insert("number".into(), json!(format!("arXiv:{arxiv}")));
                if work.url.is_none() {
                    item.insert(
                        "URL".into(),
                        json!(format!("https://arxiv.org/abs/{arxiv}")),
                    );
                }
            }
            _ => {}
        }
    }
    Value::Object(item)
}

/// The bundled styles' and locales' license and authors, to show with
/// formatted references.
pub fn credits() -> String {
    let mut out = String::from(
        "Citation styles and locales from the Citation Style Language project \
         (https://citationstyles.org), licensed under the Creative Commons \
         Attribution-ShareAlike 3.0 license \
         (https://creativecommons.org/licenses/by-sa/3.0/).\n",
    );
    for b in BUNDLED {
        let Ok(CslStyle::Independent(csl)) = CslStyle::from_xml(b.xml) else {
            continue;
        };
        let info = &csl.info;
        let names: Vec<&str> = info
            .authors
            .iter()
            .chain(&info.contributors)
            .map(|a| a.name.as_str())
            .collect();
        let _ = write!(out, "\n{}: {}", info.title.value, info.id);
        if !names.is_empty() {
            let _ = write!(out, "\n  by {}", names.join(", "));
        }
    }
    out.push_str(
        "\n\nLocales en-US and en-GB: https://github.com/citation-style-language/locales\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use aspero_core::{PartialDate, Person};

    use super::*;

    fn work(title: &str, kind: WorkType, authors: &[(&str, &str)], year: i16) -> Work {
        let mut w = Work::default();
        w.title = Some(title.into());
        w.work_type = Some(kind);
        w.authors = authors.iter().map(|(g, f)| Person::new(*g, *f)).collect();
        w.issued = Some(PartialDate::from_year(year));
        w
    }

    fn lecun() -> Work {
        let mut w = work(
            "Deep learning",
            WorkType::JournalArticle,
            &[
                ("Yann", "LeCun"),
                ("Yoshua", "Bengio"),
                ("Geoffrey", "Hinton"),
            ],
            2015,
        );
        w.container_title = Some("Nature".into());
        w.volume = Some("521".into());
        w.issue = Some("7553".into());
        w.page = Some("436-444".into());
        w
    }

    fn one(style: &str, w: &Work, ids: &[WorkId]) -> Cited {
        let style = Style::bundled(style).unwrap();
        cite(&[(w, ids)], &style, None, Markup::Text).remove(0)
    }

    #[test]
    fn every_bundled_style_names_the_authors() {
        let doi: Vec<WorkId> = vec!["doi:10.1038/nature14539".parse().unwrap()];
        for b in BUNDLED {
            let c = one(b.id, &lecun(), &doi);
            assert!(c.text.contains("LeCun"), "{}: {}", b.id, c.text);
            assert!(c.text.contains("Deep"), "{}: {}", b.id, c.text);
        }
        let c = one("vancouver", &lecun(), &doi);
        assert_eq!(
            c.text,
            "LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015;521(7553):436–44. doi:10.1038/nature14539"
        );
        assert_eq!(c.label.as_deref(), Some("1."));
        assert_eq!(c.in_text, "(1)");
        let c = one("apa", &lecun(), &doi);
        assert_eq!(c.in_text, "(LeCun et al., 2015)");
        assert_eq!(c.label, None);
    }

    #[test]
    fn books_chapters_and_preprints() {
        let mut book = work(
            "Deep Learning",
            WorkType::Book,
            &[("Ian", "Goodfellow")],
            2016,
        );
        book.publisher = Some("MIT Press".into());
        let mut chapter = work(
            "Convolutional networks",
            WorkType::Chapter,
            &[("Ian", "Goodfellow")],
            2016,
        );
        chapter.container_title = Some("Deep Learning".into());
        chapter.publisher = Some("MIT Press".into());
        chapter.page = Some("326-366".into());
        let preprint = work(
            "Attention is all you need",
            WorkType::Preprint,
            &[("Ashish", "Vaswani")],
            2017,
        );
        let arxiv: Vec<WorkId> = vec!["arxiv:1706.03762".parse().unwrap()];
        let apa = |w: &Work, ids: &[WorkId]| one("apa", w, ids).text;
        assert_eq!(
            apa(&book, &[]),
            "Goodfellow, I. (2016). Deep Learning. MIT Press."
        );
        let c = apa(&chapter, &[]);
        assert!(
            c.contains("In Deep Learning") && c.contains("326–366"),
            "{c}"
        );
        let p = apa(&preprint, &arxiv);
        assert!(p.contains("arXiv") && p.contains("1706.03762"), "{p}");
        let v = one("vancouver", &book, &[]).text;
        assert!(v.starts_with("Goodfellow I. Deep Learning."), "{v}");
    }

    /// Prints each bundled style's book, chapter and preprint:
    /// `cargo test -p aspero-bib --features styles show_types -- --ignored --nocapture`.
    #[test]
    #[ignore = "for reading"]
    fn show_types() {
        let mut book = work(
            "Deep Learning",
            WorkType::Book,
            &[("Ian", "Goodfellow"), ("Yoshua", "Bengio")],
            2016,
        );
        book.publisher = Some("MIT Press".into());
        let mut chapter = work(
            "Convolutional networks",
            WorkType::Chapter,
            &[("Ian", "Goodfellow")],
            2016,
        );
        chapter.container_title = Some("Deep Learning".into());
        chapter.publisher = Some("MIT Press".into());
        chapter.page = Some("326-366".into());
        let preprint = work(
            "Attention is all you need",
            WorkType::Preprint,
            &[("Ashish", "Vaswani")],
            2017,
        );
        let isbn: Vec<WorkId> = vec!["isbn:9780262035613".parse().unwrap()];
        let arxiv: Vec<WorkId> = vec!["arxiv:1706.03762".parse().unwrap()];
        for b in BUNDLED {
            let style = Style::bundled(b.id).unwrap();
            let works = [
                (&book, &isbn[..]),
                (&chapter, &[][..]),
                (&preprint, &arxiv[..]),
            ];
            for c in cite(&works, &style, None, Markup::Text) {
                eprintln!("{:8} {} {}", b.id, c.label.unwrap_or_default(), c.text);
            }
        }
    }

    #[test]
    fn arxiv_listed_as_a_journal_is_cited_as_a_preprint() {
        let mut w = work(
            "Attention is all you need",
            WorkType::JournalArticle,
            &[("Ashish", "Vaswani")],
            2017,
        );
        w.container_title = Some("arXiv (Cornell University)".into());
        w.volume = Some("30".into());
        w.page = Some("5998-6008".into());
        let ids: Vec<WorkId> = vec!["arxiv:1706.03762".parse().unwrap()];
        let c = one("vancouver", &w, &ids).text;
        assert!(!c.contains("Cornell") && !c.contains("5998"), "{c}");
        assert!(c.contains("arXiv"), "{c}");
    }

    #[test]
    fn order_follows_the_style() {
        let doi: Vec<WorkId> = Vec::new();
        let a = work("Zebra", WorkType::JournalArticle, &[("A", "Zed")], 2020);
        let b = work("Apple", WorkType::JournalArticle, &[("B", "Able")], 2021);
        let works = [(&a, &doi[..]), (&b, &doi[..])];
        let order = |id: &str| -> Vec<usize> {
            cite(&works, &Style::bundled(id).unwrap(), None, Markup::Text)
                .iter()
                .map(|c| c.index)
                .collect()
        };
        assert_eq!(order("apa"), [1, 0], "alphabetical");
        assert_eq!(order("vancouver"), [0, 1], "as cited");
    }

    #[test]
    fn html_marks_italics() {
        let c = cite(
            &[(&lecun(), &[])],
            &Style::bundled("apa").unwrap(),
            None,
            Markup::Html,
        );
        assert!(
            c[0].text
                .contains(r#"<span style="font-style: italic;">Nature</span>"#),
            "{}",
            c[0].text
        );
    }

    #[test]
    fn styles_load_from_text_or_say_why_not() {
        let dependent = r#"<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0">
  <info><title>The Lancet</title><id>http://www.zotero.org/styles/x</id>
    <link href="http://www.zotero.org/styles/lancet" rel="independent-parent"/>
    <updated>2020-01-01T00:00:00+00:00</updated></info></style>"#;
        assert_eq!(
            Style::from_xml(dependent).unwrap_err(),
            StyleError::Dependent {
                parent: "lancet".into()
            }
        );
        assert!(matches!(
            Style::from_xml("<p>hello</p>"),
            Err(StyleError::Invalid { .. })
        ));
        assert!(matches!(
            Style::bundled("nope"),
            Err(StyleError::Unknown { .. })
        ));
        let own = Style::from_xml(BUNDLED[0].xml).unwrap();
        assert_eq!(own.title(), "APA Style 7th edition");
    }

    #[test]
    fn credits_name_the_license_and_authors() {
        let c = credits();
        assert!(c.contains("Attribution-ShareAlike 3.0"));
        assert!(c.contains("NLM/Vancouver"), "{c}");
    }

    #[test]
    fn stray_delimiters_are_tidied() {
        assert_eq!(tidy("Nature. 2015 ;521"), "Nature. 2015;521");
        assert_eq!(
            tidy("Preprint, arXiv,. https://x"),
            "Preprint, arXiv. https://x"
        );
    }

    #[test]
    fn default_names_fill_only_what_is_missing() {
        assert_eq!(
            with_default_names(
                r#"<names variable="author"><label/><substitute><names variable="editor"/></substitute></names>"#
            ),
            r#"<names variable="author"><name/><label/><substitute><names variable="editor"/></substitute></names>"#
        );
        let named = r#"<names variable="author"><name form="short"/></names>"#;
        assert_eq!(with_default_names(named), named);
    }
}
