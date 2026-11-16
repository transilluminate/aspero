//! What's found in text: identifiers mentioned (`scan`), references split
//! from a list or laid-out lines, and their UTF-16 ranges.

use super::IdAlternative;

/// A UTF-16 range, for `NSString` and `NSTextView`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct Utf16Range {
    pub start: u64,
    pub end: u64,
}

/// An identifier mentioned in text.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Mention {
    /// With its prefix.
    pub id: String,
    /// The prefix alone: `doi`, `pmid`, `arxiv`, `pii` (an Elsevier PII), ….
    pub kind: String,
    /// As written, label included (`PMID: 123`).
    pub text: String,
    /// 1-based.
    pub line: u64,
    /// 1-based, in characters.
    pub column: u64,
    pub utf16: Utf16Range,
    /// For a DOI broken across lines at a hyphen, the DOI without it: look
    /// it up if `id` isn't found.
    pub alternative_id: Option<String>,
    /// Read from a web address: a publisher's article page
    /// (`nature.com/articles/…`, a ScienceDirect page's PII) or an
    /// identifier's link (`doi.org/…`, PubMed, arXiv), so the app can say
    /// it was found from the address. False when written out (`doi:…`,
    /// `PMID: …`).
    #[uniffi(default = false)]
    pub from_url: bool,
}

impl Mention {
    pub(crate) fn new(text: &str, m: &aspero_core::id::Mention, utf16: &mut Utf16Offsets) -> Self {
        Mention {
            id: m.id.to_string(),
            kind: m.id.kind().prefix().to_owned(),
            text: text[m.span.clone()].to_owned(),
            line: m.line as u64,
            column: m.column as u64,
            utf16: utf16.range(&m.span),
            alternative_id: m.alternative(text).map(|id| id.to_string()),
            from_url: m.from_url,
        }
    }
}

/// One reference from a pasted list.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Reference {
    /// On one line, without its number or bullet: a claim for `verify`.
    pub text: String,
    /// Where it is in the list, marker excluded.
    pub utf16: Utf16Range,
    /// DOIs broken at a hyphen, read without it: pass them on as the
    /// claim's `alternativeIds`.
    pub alternative_ids: Vec<IdAlternative>,
}

impl Reference {
    pub(crate) fn new(r: &aspero_core::Reference, utf16: &mut Utf16Offsets) -> Self {
        Reference {
            text: r.text.clone(),
            utf16: utf16.range(&r.span),
            alternative_ids: r.alternatives.iter().map(Into::into).collect(),
        }
    }
}

/// A line of a reference list as laid out: its text and its distance from
/// the left edge (PDFKit's x-offset, in any unit).
#[derive(Debug, Clone, uniffi::Record)]
pub struct Line {
    pub text: String,
    pub indent: f64,
}

/// Line indices, `end` exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct LineRange {
    pub start: u64,
    pub end: u64,
}

/// One reference from laid-out lines.
#[derive(Debug, Clone, uniffi::Record)]
pub struct LineReference {
    /// On one line, without its number or bullet: a claim for `verify`.
    pub text: String,
    /// The lines it spans.
    pub lines: LineRange,
    /// Pass on as the claim's `alternativeIds`.
    pub alternative_ids: Vec<IdAlternative>,
}

impl From<&aspero_core::LineReference> for LineReference {
    fn from(r: &aspero_core::LineReference) -> Self {
        LineReference {
            text: r.text.clone(),
            lines: LineRange {
                start: r.lines.start as u64,
                end: r.lines.end as u64,
            },
            alternative_ids: r.alternatives.iter().map(Into::into).collect(),
        }
    }
}

/// Byte offsets in a text as UTF-16 ones, counted on from the last one
/// asked for: spans in order cost one pass over the text.
pub(crate) struct Utf16Offsets<'a> {
    text: &'a str,
    byte: usize,
    utf16: u64,
}

impl<'a> Utf16Offsets<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        Utf16Offsets {
            text,
            byte: 0,
            utf16: 0,
        }
    }

    pub(super) fn at(&mut self, byte: usize) -> u64 {
        if byte < self.byte {
            // Out of order: count from the start again.
            *self = Utf16Offsets::new(self.text);
        }
        self.utf16 += self.text[self.byte..byte].encode_utf16().count() as u64;
        self.byte = byte;
        self.utf16
    }

    pub(super) fn range(&mut self, span: &std::ops::Range<usize>) -> Utf16Range {
        Utf16Range {
            start: self.at(span.start),
            end: self.at(span.end),
        }
    }
}
