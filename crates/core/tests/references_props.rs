//! Splitting never fails on arbitrary text, and its spans can be used.

use aspero_core::split_references;
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
