//! Splitting a pasted reference list (`split_references`) or a PDF's
//! lines with their indents (`split_lines`): spans usable and in order.
//! The first byte picks which; for lines, the rest is read as `arbitrary`
//! lines and indents.

#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use aspero_core::{IndentedLine, split_lines, split_references};
use libfuzzer_sys::fuzz_target;

#[path = "common.rs"]
mod common;

#[derive(Arbitrary, Debug)]
struct Line {
    text: String,
    /// Tenths of a point: indents as PDFKit reports them, finite.
    indent: u16,
}

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    if mode & 1 == 0 {
        let Ok(text) = std::str::from_utf8(rest) else {
            return;
        };
        let mut end = 0;
        for r in split_references(text) {
            common::assert_span(text, &r.span, "reference");
            assert!(r.span.start >= end, "references overlap in {text:?}");
            assert!(!r.text.trim().is_empty());
            end = r.span.end;
        }
    } else {
        let Ok(lines) = Vec::<Line>::arbitrary_take_rest(Unstructured::new(rest)) else {
            return;
        };
        let lines: Vec<IndentedLine> = lines
            .into_iter()
            .map(|l| IndentedLine::new(l.text, f64::from(l.indent) / 10.0))
            .collect();
        let mut end = 0;
        for r in split_lines(&lines) {
            assert!(r.lines.start >= end && r.lines.start < r.lines.end);
            assert!(r.lines.end <= lines.len());
            end = r.lines.end;
        }
    }
});
