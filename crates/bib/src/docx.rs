//! Word documents (.docx): the citations Zotero and Mendeley keep in field
//! codes (`ADDIN ZOTERO_ITEM CSL_CITATION {…}`, `ADDIN CSL_CITATION {…}`),
//! each with its works' CSL-JSON, identifiers included. That is better
//! input than the rendered reference list, whose text is returned too as
//! the fallback for documents without them.

use std::io::{Cursor, Read};

use aspero_core::csl::CslItem;
use quick_xml::Reader;
use quick_xml::events::Event;
use serde::Deserialize;

use crate::{BibEntry, ReadError};

/// The citation manager whose fields a document holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Manager {
    Zotero,
    Mendeley,
}

/// What a document cites.
#[derive(Debug, Clone, PartialEq)]
pub struct DocxReferences {
    /// Each work cited, once, in order of first citation; `key` is the
    /// manager's item key.
    pub entries: Vec<BibEntry>,
    /// Whose fields they came from; `None` when there were none.
    pub manager: Option<Manager>,
    /// Citations in the text (one may cite several works).
    pub citations: usize,
    /// The document's text, a paragraph per line: its reference list can
    /// be split instead when there are no fields.
    pub text: String,
}

/// Parts that can hold citations: the body, footnotes and endnotes.
const PARTS: [&str; 3] = [
    "word/document.xml",
    "word/footnotes.xml",
    "word/endnotes.xml",
];

/// Reads a .docx file's citation fields and text.
pub fn read_docx(bytes: &[u8]) -> Result<DocxReferences, ReadError> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| ReadError::Unrecognised)?;
    let mut read = |name: &str| -> Option<String> {
        let mut part = zip.by_name(name).ok()?;
        let mut xml = String::new();
        part.read_to_string(&mut xml).ok()?;
        Some(xml)
    };
    let body = read(PARTS[0]).ok_or(ReadError::Unrecognised)?;
    let mut fields = Fields::default();
    let text = fields.scan(&body);
    for part in &PARTS[1..] {
        if let Some(xml) = read(part) {
            fields.scan(&xml);
        }
    }
    Ok(DocxReferences {
        entries: fields.entries,
        manager: fields.manager,
        citations: fields.citations,
        text,
    })
}

#[derive(Default)]
struct Fields {
    entries: Vec<BibEntry>,
    /// Each entry's identity: the manager's URI or id.
    seen: Vec<String>,
    manager: Option<Manager>,
    citations: usize,
}

/// A citation field's JSON: the works it cites.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Citation {
    #[serde(default)]
    citation_items: Vec<CitationItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CitationItem {
    #[serde(default)]
    id: Option<serde_json::Value>,
    #[serde(default)]
    uris: Vec<String>,
    #[serde(default)]
    uri: Vec<String>,
    item_data: Option<serde_json::Value>,
}

impl Fields {
    /// Collects a part's citation fields; returns its text, a paragraph
    /// per line.
    fn scan(&mut self, xml: &str) -> String {
        let mut reader = Reader::from_str(xml);
        let mut text = String::new();
        // Instructions of the fields open at this point (they nest).
        let mut open: Vec<String> = Vec::new();
        let mut in_instr = false;
        let mut in_text = false;
        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                    b"w:fldChar" => match attr(&e, b"w:fldCharType").as_deref() {
                        Some("begin") => open.push(String::new()),
                        Some("end") => {
                            if let Some(instr) = open.pop() {
                                self.field(&instr);
                            }
                        }
                        _ => {}
                    },
                    b"w:fldSimple" => {
                        if let Some(instr) = attr(&e, b"w:instr") {
                            self.field(&instr);
                        }
                    }
                    b"w:instrText" => in_instr = true,
                    b"w:t" => in_text = true,
                    b"w:tab" => text.push('\t'),
                    b"w:br" => text.push('\n'),
                    _ => {}
                },
                Ok(Event::End(e)) => match e.name().as_ref() {
                    b"w:instrText" => in_instr = false,
                    b"w:t" => in_text = false,
                    b"w:p" => text.push('\n'),
                    _ => {}
                },
                Ok(Event::Text(t)) => {
                    let Ok(s) = t.decode() else { continue };
                    let s = quick_xml::escape::unescape(&s)
                        .map_or_else(|_| s.to_string(), |u| u.into_owned());
                    if in_instr {
                        if let Some(instr) = open.last_mut() {
                            instr.push_str(&s);
                        }
                    } else if in_text {
                        text.push_str(&s);
                    }
                }
                Ok(Event::GeneralRef(r)) => {
                    // An entity inside text: `&amp;` and friends.
                    let name: &[u8] = &r;
                    let resolved = match name {
                        b"amp" => "&",
                        b"lt" => "<",
                        b"gt" => ">",
                        b"quot" => "\"",
                        b"apos" => "'",
                        _ => "",
                    };
                    if in_instr {
                        if let Some(instr) = open.last_mut() {
                            instr.push_str(resolved);
                        }
                    } else if in_text {
                        text.push_str(resolved);
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
        text
    }

    /// One field's instruction: a citation's works, if it is one.
    fn field(&mut self, instr: &str) {
        let instr = instr.trim();
        let Some(rest) = instr.strip_prefix("ADDIN").map(str::trim_start) else {
            return;
        };
        let (manager, json) = if let Some(r) = rest.strip_prefix("ZOTERO_ITEM") {
            (Manager::Zotero, r)
        } else if rest.starts_with("CSL_CITATION") {
            (Manager::Mendeley, rest)
        } else {
            return;
        };
        let Some(start) = json.find('{') else { return };
        // The JSON may be followed by more text; take the first value.
        let mut values = serde_json::Deserializer::from_str(&json[start..]).into_iter::<Citation>();
        let Some(Ok(citation)) = values.next() else {
            return;
        };
        self.manager.get_or_insert(manager);
        self.citations += 1;
        for item in citation.citation_items {
            let Some(data) = item.item_data else { continue };
            let identity = item
                .uris
                .first()
                .or(item.uri.first())
                .cloned()
                .or_else(|| item.id.as_ref().map(ToString::to_string))
                .unwrap_or_else(|| data.to_string());
            if self.seen.contains(&identity) {
                continue;
            }
            let Ok(csl) = serde_json::from_value::<CslItem>(data) else {
                continue;
            };
            let mut entry = crate::csl::entry(&csl, self.entries.len() + 1);
            // Zotero's item key ends its URI: …/items/ABCD1234.
            if identity.contains("/items/") {
                entry.key = identity.rsplit('/').next().unwrap_or_default().to_owned();
            }
            self.seen.push(identity);
            self.entries.push(entry);
        }
    }
}

fn attr(e: &quick_xml::events::BytesStart, name: &[u8]) -> Option<String> {
    let a = e.try_get_attribute(name).ok()??;
    Some(a.unescape_value().ok()?.into_owned())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    /// A minimal .docx: a body, and optionally footnotes.
    fn docx(body: &str, footnotes: Option<&str>) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut out));
            let stored = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            let wrap = |inner: &str| {
                format!(
                    r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{inner}</w:body></w:document>"#
                )
            };
            zip.start_file("word/document.xml", stored).unwrap();
            zip.write_all(wrap(body).as_bytes()).unwrap();
            if let Some(notes) = footnotes {
                zip.start_file("word/footnotes.xml", stored).unwrap();
                zip.write_all(wrap(notes).as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }
        out
    }

    fn escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('"', "&quot;")
    }

    /// A Zotero citation field, its instruction split over runs as Word
    /// does, rendered as `shown`.
    fn zotero(json: &str, shown: &str) -> String {
        let instr = escape(&format!(" ADDIN ZOTERO_ITEM CSL_CITATION {json} "));
        let (a, b) = instr.split_at(instr.len() / 2);
        format!(
            r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve">{a}</w:instrText></w:r><w:r><w:instrText>{b}</w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>{shown}</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>"#
        )
    }

    const LECUN: &str = r#"{"citationID":"a1","properties":{"formattedCitation":"(LeCun et al., 2015)"},"citationItems":[{"id":12,"uris":["http://zotero.org/users/1/items/ABCD1234"],"itemData":{"id":12,"type":"article-journal","title":"Deep learning","container-title":"Nature","volume":"521","page":"436-444","author":[{"family":"LeCun","given":"Yann"}],"issued":{"date-parts":[["2015",5,27]]},"DOI":"10.1038/nature14539"}}],"schema":"https://github.com/citation-style-language/schema/raw/master/csl-citation.json"}"#;

    #[test]
    fn zotero_fields_become_claims_once_each() {
        let body = format!(
            r#"<w:p><w:r><w:t>Deep nets work </w:t></w:r>{}<w:r><w:t>.</w:t></w:r></w:p><w:p>{}</w:p><w:p><w:r><w:t>References &amp; notes</w:t></w:r></w:p>"#,
            zotero(LECUN, "(LeCun et al., 2015)"),
            zotero(LECUN, "(LeCun et al., 2015)"),
        );
        let read = read_docx(&docx(&body, None)).unwrap();
        assert_eq!(read.manager, Some(Manager::Zotero));
        assert_eq!(read.citations, 2);
        assert_eq!(read.entries.len(), 1, "cited twice, read once");
        let e = &read.entries[0];
        assert_eq!(e.key, "ABCD1234");
        assert_eq!(e.claim.ids[0].to_string(), "doi:10.1038/nature14539");
        assert_eq!(e.claim.authors, ["LeCun, Yann"]);
        assert_eq!(e.claim.volume.as_deref(), Some("521"));
        assert_eq!(
            read.text,
            "Deep nets work (LeCun et al., 2015).\n(LeCun et al., 2015)\nReferences & notes\n"
        );
    }

    #[test]
    fn mendeley_fields_and_footnotes_count_too() {
        let mendeley = r#"{"citationItems":[{"id":"ITEM-1","uris":["http://www.mendeley.com/documents/?uuid=1"],"itemData":{"type":"book","title":"Deep Learning","ISBN":"9780262035613"}}],"mendeley":{}}"#;
        let instr = escape(&format!("ADDIN CSL_CITATION {mendeley}"));
        let simple = format!(
            r#"<w:p><w:fldSimple w:instr="{instr}"><w:r><w:t>[1]</w:t></w:r></w:fldSimple></w:p>"#
        );
        let read = read_docx(&docx("<w:p/>", Some(&simple))).unwrap();
        assert_eq!(read.manager, Some(Manager::Mendeley));
        assert_eq!(
            read.entries[0].claim.ids[0].to_string(),
            "isbn:9780262035613"
        );
        assert_eq!(read.entries[0].key, "1");
    }

    #[test]
    fn documents_without_fields_give_their_text() {
        let body = r#"<w:p><w:r><w:t>1. Smith J. A study. 2020.</w:t></w:r></w:p>"#;
        let read = read_docx(&docx(body, None)).unwrap();
        assert!(read.entries.is_empty());
        assert_eq!(read.manager, None);
        assert_eq!(read.text, "1. Smith J. A study. 2020.\n");
        assert_eq!(read_docx(b"not a zip"), Err(ReadError::Unrecognised));
    }
}
