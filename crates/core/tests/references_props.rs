//! Splitting never fails on arbitrary text, and its spans can be used.

use aspero_core::{IndentedLine, split_lines, split_references};
use proptest::prelude::*;

proptest! {
    #[test]
    fn spans_are_usable_and_in_order(text in "(\\PC{0,30}[\\n\\r ]{0,3}(\\[?[0-9]{1,2}[.\\])]? )?){0,8}") {
        let mut end = 0;
        for r in split_references(&text) {
            prop_assert!(r.span.start >= end && r.span.start < r.span.end);
            prop_assert!(text.get(r.span.clone()).is_some(), "off a char boundary");
            prop_assert!(!r.text.trim().is_empty());
            end = r.span.end;
        }
    }
}

proptest! {
    #[test]
    fn line_ranges_are_in_bounds_and_in_order(
        lines in proptest::collection::vec(("(\\PC{0,20}( [0-9]{1,2}\\.)?)", 60.0f64..120.0), 0..12)
    ) {
        let lines: Vec<IndentedLine> = lines
            .into_iter()
            .map(|(text, indent)| IndentedLine::new(text, indent))
            .collect();
        let mut end = 0;
        for r in split_lines(&lines) {
            prop_assert!(r.lines.start >= end && r.lines.start < r.lines.end);
            prop_assert!(r.lines.end <= lines.len());
            end = r.lines.end;
        }
    }
}
