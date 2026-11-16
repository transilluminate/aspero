//! Reading text, offline: identifiers mentioned, and references split from
//! a pasted list or laid-out lines.

use crate::Aspero;
use crate::records::{Line, LineReference, Mention, Reference, Utf16Offsets};

#[uniffi::export]
impl Aspero {
    /// The identifiers mentioned in `text`, in order, where and as written.
    /// Offline.
    // Exported: UniFFI hands the arguments over by value.
    #[allow(clippy::needless_pass_by_value)]
    pub fn scan(&self, text: String) -> Vec<Mention> {
        let mut utf16 = Utf16Offsets::new(&text);
        aspero_core::id::scan(&text)
            .iter()
            .map(|m| Mention::new(&text, m, &mut utf16))
            .collect()
    }

    /// The references in a pasted list (numbered, bulleted, one per
    /// paragraph or line; wrapped lines rejoined), each ready to verify.
    /// Offline.
    // Exported: UniFFI hands the arguments over by value.
    #[allow(clippy::needless_pass_by_value)]
    pub fn split_references(&self, text: String) -> Vec<Reference> {
        let mut utf16 = Utf16Offsets::new(&text);
        aspero_core::split_references(&text)
            .iter()
            .map(|r| Reference::new(r, &mut utf16))
            .collect()
    }

    /// The references in a list's lines as laid out (from PDFKit), using
    /// their indents: hanging or first-line indents mark where references
    /// start when the text alone can't. Numbered lists, and lines all at
    /// one indent, are split as `splitReferences` splits text. Offline.
    pub fn split_lines(&self, lines: Vec<Line>) -> Vec<LineReference> {
        let lines: Vec<aspero_core::IndentedLine> = lines
            .into_iter()
            .map(|l| aspero_core::IndentedLine::new(l.text, l.indent))
            .collect();
        aspero_core::split_lines(&lines)
            .iter()
            .map(Into::into)
            .collect()
    }
}
