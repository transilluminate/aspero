//! Cleaning text extracted from PDFs and word processors before it is
//! scanned or split, keeping the way back to the original: ligatures
//! (`ﬁ`) expanded, invisible characters (soft hyphens, zero-width spaces)
//! dropped, and look-alikes folded to ASCII (Unicode hyphens and dashes to
//! `-`, curly quotes to straight, unusual spaces to ` `). A soft hyphen at
//! the end of a line marks a word broken there, so it becomes `-` for the
//! hyphenation mending in [`split_references`](crate::split_references).

use std::ops::Range;

/// Text cleaned, with each of its bytes' source in the original.
pub(crate) struct Folded {
    pub(crate) text: String,
    /// Per byte of `text`: the byte range of the original character it
    /// came from.
    from: Vec<Range<usize>>,
}

impl Folded {
    pub(crate) fn new(original: &str) -> Self {
        let mut text = String::with_capacity(original.len());
        let mut from = Vec::with_capacity(original.len());
        let mut push = |s: &str, source: Range<usize>, text: &mut String| {
            text.push_str(s);
            from.extend(std::iter::repeat_n(source, s.len()));
        };
        for (i, c) in original.char_indices() {
            let source = i..i + c.len_utf8();
            let mut buf = [0; 4];
            match c {
                '\u{AD}' => {
                    let rest = original[source.end..].trim_start_matches([' ', '\t', '\r']);
                    if rest.starts_with('\n') {
                        push("-", source, &mut text);
                    }
                }
                '\u{200B}'..='\u{200D}' | '\u{2060}' | '\u{FEFF}' => {}
                '\u{2010}'..='\u{2015}' | '\u{2212}' | '\u{FE63}' | '\u{FF0D}' => {
                    push("-", source, &mut text)
                }
                '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{2032}' => push("'", source, &mut text),
                '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{2033}' => push("\"", source, &mut text),
                '\u{A0}' | '\u{2000}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}' => {
                    push(" ", source, &mut text)
                }
                '\u{FB00}' => push("ff", source, &mut text),
                '\u{FB01}' => push("fi", source, &mut text),
                '\u{FB02}' => push("fl", source, &mut text),
                '\u{FB03}' => push("ffi", source, &mut text),
                '\u{FB04}' => push("ffl", source, &mut text),
                '\u{FB05}' | '\u{FB06}' => push("st", source, &mut text),
                _ => push(c.encode_utf8(&mut buf), source, &mut text),
            }
        }
        Folded { text, from }
    }

    /// The original byte range of `range` in the cleaned text: from its
    /// first character's start to its last one's end.
    pub(crate) fn original(&self, range: &Range<usize>) -> Range<usize> {
        if range.is_empty() {
            let at = self
                .from
                .get(range.start)
                .map_or_else(|| self.from.last().map_or(0, |r| r.end), |r| r.start);
            return at..at;
        }
        self.from[range.start].start..self.from[range.end - 1].end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn look_alikes_fold_and_spans_map_back() {
        let original = "ﬁnd 10.1038/s41586\u{2010}020\u{200B}-2649-2 “x”";
        let folded = Folded::new(original);
        assert_eq!(folded.text, "find 10.1038/s41586-020-2649-2 \"x\"");
        let doi = folded.text.find("10.").unwrap()..folded.text.find(" \"").unwrap();
        assert_eq!(
            &original[folded.original(&doi)],
            "10.1038/s41586\u{2010}020\u{200B}-2649-2"
        );
        assert_eq!(&original[folded.original(&(0..1))], "ﬁ");
    }

    #[test]
    fn soft_hyphens_mark_breaks_only_at_line_ends() {
        assert_eq!(Folded::new("neuro\u{AD}\nlogical").text, "neuro-\nlogical");
        assert_eq!(Folded::new("neuro\u{AD}logical").text, "neurological");
    }
}
