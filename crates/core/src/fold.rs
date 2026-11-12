//! Cleaning text extracted from PDFs and word processors before it is
//! scanned or split, keeping the way back to the original: ligatures
//! (`ﬁ`) expanded, invisible characters (soft hyphens, zero-width spaces)
//! dropped, and look-alikes folded to ASCII (Unicode hyphens and dashes to
//! `-`, curly quotes to straight, unusual spaces to ` `). A soft hyphen at
//! the end of a line marks a word broken there, so it becomes `-` for the
//! hyphenation mending in [`split_references`](crate::split_references).

use std::borrow::Cow;
use std::ops::Range;

/// Text cleaned, with the way back to the original.
pub(crate) struct Folded<'a> {
    /// Borrowed when nothing needed cleaning, as in most text.
    pub(crate) text: Cow<'a, str>,
    /// Each character changed, in order: its bytes in `text` (empty if
    /// dropped) and in the original. Bytes between are as in the original.
    edits: Vec<(Range<usize>, Range<usize>)>,
}

impl<'a> Folded<'a> {
    pub(crate) fn new(original: &'a str) -> Self {
        let bytes = original.as_bytes();
        let mut text = String::new();
        let mut edits = Vec::new();
        // Original bytes up to here are in `text`.
        let mut copied = 0;
        let mut i = next_foldable(bytes, 0);
        while i < bytes.len() {
            let c = original[i..].chars().next().unwrap_or_default();
            let source = i..i + c.len_utf8();
            if let Some(folded) = fold(c, &original[source.end..]) {
                if text.is_empty() {
                    text.reserve(original.len());
                }
                text.push_str(&original[copied..i]);
                let at = text.len();
                text.push_str(folded);
                edits.push((at..text.len(), source.clone()));
                copied = source.end;
            }
            i = next_foldable(bytes, source.end);
        }
        if edits.is_empty() {
            return Folded {
                text: Cow::Borrowed(original),
                edits,
            };
        }
        text.push_str(&original[copied..]);
        Folded {
            text: Cow::Owned(text),
            edits,
        }
    }

    /// The original byte range of `range` in the cleaned text: from its
    /// first character's start to its last one's end.
    pub(crate) fn original(&self, range: &Range<usize>) -> Range<usize> {
        if range.is_empty() {
            let at = if range.start < self.text.len() {
                self.start(range.start)
            } else {
                self.end(self.text.len())
            };
            return at..at;
        }
        self.start(range.start)..self.end(range.end)
    }

    /// Where the original character with byte `at` of the text starts.
    fn start(&self, at: usize) -> usize {
        match self.edit_at(at) {
            None => at,
            Some((t, o)) if at < t.end => o.start,
            Some((t, o)) => o.end + (at - t.end),
        }
    }

    /// Where the original character with byte `end - 1` of the text ends.
    fn end(&self, end: usize) -> usize {
        let Some(last) = end.checked_sub(1) else {
            return 0;
        };
        match self.edit_at(last) {
            None => end,
            Some((t, o)) if last < t.end => o.end,
            Some((t, o)) => o.end + (end - t.end),
        }
    }

    /// The last edit starting at or before byte `at` of the text.
    fn edit_at(&self, at: usize) -> Option<&(Range<usize>, Range<usize>)> {
        let i = self.edits.partition_point(|(t, _)| t.start <= at);
        i.checked_sub(1).map(|i| &self.edits[i])
    }
}

/// What ends a line in text as given: cleaning makes each `\n` (but for
/// the `\r` of `\r\n`, which trimming the line drops).
pub(crate) const LINE_BREAKS: [char; 5] = ['\n', '\r', '\u{85}', '\u{2028}', '\u{2029}'];

/// What `c` becomes, if it changes; `rest` is the text after it.
fn fold(c: char, rest: &str) -> Option<&'static str> {
    Some(match c {
        '\u{AD}' => {
            if rest.trim_start_matches([' ', '\t', '\r']).starts_with('\n') {
                "-"
            } else {
                ""
            }
        }
        '\u{200B}'..='\u{200D}' | '\u{2060}' | '\u{FEFF}' => "",
        '\u{2010}'..='\u{2015}' | '\u{2212}' | '\u{FE63}' | '\u{FF0D}' => "-",
        '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{2032}' => "'",
        '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{2033}' => "\"",
        '\u{A0}' | '\u{2000}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}' => " ",
        // Line breaks as some copies write them: classic Mac's lone `\r`,
        // Unicode's line and paragraph separators, NEL.
        '\r' if rest.starts_with('\n') => return None,
        c if c != '\n' && LINE_BREAKS.contains(&c) => "\n",
        '\u{FB00}' => "ff",
        '\u{FB01}' => "fi",
        '\u{FB02}' => "fl",
        '\u{FB03}' => "ffi",
        '\u{FB04}' => "ffl",
        '\u{FB05}' | '\u{FB06}' => "st",
        _ => return None,
    })
}

/// The first byte at or after `from` that may start a character [`fold`]
/// changes, or the end: every one is `\r`, U+0080 to U+00FF (lead byte
/// `C2`), U+2000 to U+2FFF (`E2`), U+3000 to U+3FFF (`E3`) or U+F000 to
/// U+FFFF (`EF`).
fn next_foldable(bytes: &[u8], from: usize) -> usize {
    const CHUNK: usize = 32;
    let lead = |b: u8| matches!(b, b'\r' | 0xC2 | 0xE2 | 0xE3 | 0xEF);
    let mut at = from;
    while let Some(chunk) = bytes.get(at..at + CHUNK) {
        // No early exit within a chunk, so it vectorises.
        if chunk.iter().fold(0, |any, &b| any | u8::from(lead(b))) != 0 {
            break;
        }
        at += CHUNK;
    }
    bytes
        .get(at..)
        .and_then(|rest| rest.iter().position(|&b| lead(b)))
        .map_or(bytes.len(), |i| at + i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// The first implementation: every byte's source, for comparison.
    fn reference(original: &str) -> (String, Vec<Range<usize>>) {
        let mut text = String::new();
        let mut from = Vec::new();
        for (i, c) in original.char_indices() {
            let source = i..i + c.len_utf8();
            let mut buf = [0; 4];
            let s = fold(c, &original[source.end..]).unwrap_or_else(|| c.encode_utf8(&mut buf));
            text.push_str(s);
            from.extend(std::iter::repeat_n(source, s.len()));
        }
        (text, from)
    }

    fn reference_original(from: &[Range<usize>], range: &Range<usize>) -> Range<usize> {
        if range.is_empty() {
            let at = from
                .get(range.start)
                .map_or_else(|| from.last().map_or(0, |r| r.end), |r| r.start);
            return at..at;
        }
        from[range.start].start..from[range.end - 1].end
    }

    proptest! {
        #[test]
        fn matches_the_reference(
            parts in prop::collection::vec(prop::sample::select(vec![
                "a", "1", " ", "\n", "\t", "/", "é", "\u{AD}", "\u{AD}\n", "\u{200B}",
                "\u{FEFF}", "\u{2013}", "\u{201C}", "\u{2019}", "\u{A0}", "\u{3000}",
                "\u{FB03}", "\u{FB01}", "\u{1F600}", "\u{2028}", "\u{2029}", "\u{85}", "\r",
                "\r\n", "abcdefghij",
            ]), 0..40),
        ) {
            let original = parts.concat();
            let folded = Folded::new(&original);
            let (text, from) = reference(&original);
            prop_assert_eq!(&*folded.text, text.as_str());
            let bounds: Vec<usize> = (0..=text.len()).filter(|&i| text.is_char_boundary(i)).collect();
            for (n, &a) in bounds.iter().enumerate() {
                for &b in &bounds[n..] {
                    prop_assert_eq!(
                        folded.original(&(a..b)),
                        reference_original(&from, &(a..b)),
                        "{:?} {}..{}", original, a, b
                    );
                }
            }
        }
    }

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
    fn line_breaks_however_written_are_newlines() {
        assert_eq!(
            Folded::new("a\rb\u{2028}c\u{2029}d\u{85}e").text,
            "a\nb\nc\nd\ne"
        );
        // A Windows line ending is one break, not two.
        assert_eq!(Folded::new("a\r\nb").text, "a\r\nb");
    }

    #[test]
    fn soft_hyphens_mark_breaks_only_at_line_ends() {
        assert_eq!(Folded::new("neuro\u{AD}\nlogical").text, "neuro-\nlogical");
        assert_eq!(Folded::new("neuro\u{AD}logical").text, "neurological");
    }
}
