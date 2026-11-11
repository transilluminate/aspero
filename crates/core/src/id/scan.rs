//! Finding identifiers in documents: every mention, where it is, and how it
//! was written. Which mentions matter (a citation, an aside, a reference
//! list) is the caller's call; this only says what is a valid identifier.
//!
//! Recognised anywhere: DOIs (bare, `doi:`-prefixed or as URLs), PMCIDs
//! (`PMC…`) and NCBI Bookshelf IDs (`NBK…`). Recognised only when labelled
//! (`PMID: 123`, `ISBN 978-…`), prefixed (`arXiv:…`) or as a URL: everything
//! else, since bare digits, `W1` or a 40-digit hex string in prose are more
//! often something else. Mentions are cleaned like pasted input: wrapping
//! brackets, quotes and emphasis and trailing sentence punctuation are not
//! part of them, while brackets inside a DOI are. Markdown links and table
//! cells are split apart. A DOI or URL broken across lines, as text from
//! PDFs is, is rejoined by the rules `split_references` uses: the mention
//! then spans both lines.

use std::ops::Range;

use super::syntax::clean_pasted;
use super::url::Url;
use super::{IdKind, WorkId};

/// One identifier mentioned in a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    pub id: WorkId,
    /// 1-based line number.
    pub line: usize,
    /// 1-based column of the mention's start, in characters.
    pub column: usize,
    /// Byte range of the mention as written, label included (`PMID:
    /// 123`), wrappers and trailing punctuation not. A link broken across
    /// lines spans both, line break included.
    pub span: Range<usize>,
}

impl Mention {
    /// For a DOI broken across lines at a hyphen, the DOI without that
    /// hyphen: typesetters rarely add one inside a link, but some do, so
    /// it's worth a lookup if the DOI as printed doesn't exist. `text` is
    /// the text scanned.
    pub fn alternative(&self, text: &str) -> Option<WorkId> {
        let folded = crate::fold::Folded::new(text.get(self.span.clone())?);
        let written: &str = &folded.text;
        let mut joined = String::new();
        let mut broken = false;
        let mut rest = written;
        while let Some(at) = rest.find('-') {
            let after = &rest[at + 1..];
            let gap = after.len() - after.trim_start().len();
            let at_break = after[..gap].contains('\n');
            joined.push_str(&rest[..at + usize::from(!at_break)]);
            broken |= at_break;
            rest = &after[if at_break { gap } else { 0 }..];
        }
        joined.push_str(rest);
        if !broken || self.id.kind() != IdKind::Doi {
            return None;
        }
        let joined: String = joined.split_whitespace().collect::<Vec<_>>().join(" ");
        let id: WorkId = scan_clean(&joined)
            .into_iter()
            .find(|m| m.span.start == 0)?
            .id;
        (id != self.id && id.kind() == IdKind::Doi).then_some(id)
    }
}

/// Every identifier mentioned in `text`, in order, repeats included. Text
/// from PDFs is cleaned first (ligatures, invisible characters, look-alike
/// dashes and quotes); spans point into `text` as given.
pub fn scan(text: &str) -> Vec<Mention> {
    let folded = crate::fold::Folded::new(text);
    located(&folded.text)
        .into_iter()
        .map(|(m, line_start)| {
            let span = folded.original(&m.span);
            // After the line break, which is never cleaned.
            let line_start = match line_start {
                0 => 0,
                at => folded.original(&(at - 1..at)).end,
            };
            Mention {
                line: m.line,
                column: text[line_start..span.start].chars().count() + 1,
                span,
                ..m
            }
        })
        .collect()
}

/// [`scan`] of text already cleaned.
fn scan_clean(text: &str) -> Vec<Mention> {
    located(text)
        .into_iter()
        .map(|(m, line_start)| Mention {
            column: text[line_start..m.span.start].chars().count() + 1,
            ..m
        })
        .collect()
}

/// [`scan_clean`], each mention with where its line starts and its
/// column not yet counted.
fn located(text: &str) -> Vec<(Mention, usize)> {
    let bytes = text.as_bytes();
    let mut raw = Vec::new();
    let mut broken = Vec::new();
    find(text, &mut raw, &mut broken);
    if !broken.is_empty() {
        raw.extend(broken.iter().filter_map(|b| mend(text, b)));
        raw.sort_by_key(|(_, span)| span.start);
    }
    let starts: Vec<usize> = raw.iter().map(|(_, span)| span.start).collect();
    let mut found = Vec::with_capacity(raw.len());
    // Where a mention continued onto the next line ends: nothing before it
    // there is scanned again.
    let mut resume = 0;
    // The line at `counted` and where it starts.
    let (mut line, mut line_start, mut counted) = (1, 0, 0);
    for (k, (mut id, mut span)) in raw.into_iter().enumerate() {
        if span.start < resume {
            continue;
        }
        let gap = &bytes[counted..span.start];
        if let Some(i) = memrchr(b'\n', gap) {
            line += newlines(gap);
            line_start = counted + i + 1;
        }
        counted = span.start;
        let line_end = memchr(b'\n', &bytes[span.end..]).map_or(text.len(), |i| span.end + i);
        let last = starts.get(k + 1).is_none_or(|&next| next > line_end);
        if last && line_end < text.len() {
            let next_start = line_end + 1;
            let next_end =
                memchr(b'\n', &bytes[next_start..]).map_or(text.len(), |i| next_start + i);
            if let Some((joined, end)) = rejoin(
                text,
                &span,
                line_end,
                next_start,
                &text[next_start..next_end],
            ) && joined != id
                && joined.kind() == id.kind()
            {
                id = joined;
                span.end = end;
                resume = end;
            }
        }
        // A mention mended or rejoined across lines covers what starts the
        // next.
        resume = resume.max(span.end);
        found.push((
            Mention {
                id,
                line,
                column: 0,
                span,
            },
            line_start,
        ));
    }
    found
}

/// A link mention at the end of its line (`span`, the line ending at
/// `line_end`) continued at the start of the next: the whole identifier and
/// where the continuation ends.
fn rejoin(
    text: &str,
    span: &Range<usize>,
    line_end: usize,
    next_start: usize,
    next: &str,
) -> Option<(WorkId, usize)> {
    // What the cleaning dropped after the mention (a `.` before the
    // break), if nothing else follows it on the line.
    let tail = text[span.end..line_end].trim_end();
    if tail.chars().count() > 1 || tail.contains(char::is_whitespace) {
        return None;
    }
    let raw = &text[span.start..span.end + tail.len()];
    if !crate::references::link_continues(raw, next) {
        return None;
    }
    let indent = next.len() - next.trim_start().len();
    let token = next.split_whitespace().next()?;
    let joined = format!("{raw}{token}");
    let mut mentions = Vec::new();
    find(&joined, &mut mentions, &mut Vec::new());
    let (id, within) = mentions
        .into_iter()
        .find(|(_, s)| s.start == 0 && s.end > raw.len())?;
    Some((id, next_start + indent + within.end - raw.len()))
}

/// Identifiers in `text`, with byte ranges, line by line: a label
/// before a line break doesn't apply after it.
///
/// Most pieces of a text are words, which can't be identifiers: only a
/// piece with a digit, `/` or `:` (bytes `/` to `:`, found a chunk at a
/// time) is parsed, and the piece before it is read as a label only then.
fn find(text: &str, found: &mut Vec<(WorkId, Range<usize>)>, broken: &mut Vec<Broken>) {
    let bytes = text.as_bytes();
    // The last piece parsed: where it ends, and the label it was.
    let mut last: Option<(usize, Option<(IdKind, usize)>)> = None;
    let mut at = 0;
    while let Some(hit) = next_hit(bytes, at) {
        let start = piece_start(text, hit + 1);
        let Some((piece, class)) = (Pieces { text, at: start }).next() else {
            break;
        };
        at = piece.end;
        if class & SOLID == 0 {
            // Punctuation only: nothing if it cleans away, else neither a
            // label nor an identifier.
            if !clean_pasted(&text[piece.clone()]).is_empty() {
                last = Some((piece.end, None));
            }
            continue;
        }
        let label = label_before(text, start, last);
        // A number: neither a label nor, unlabelled, an identifier.
        if label.is_none() && class & (LINK | ALPHA) == 0 {
            last = Some((piece.end, None));
            continue;
        }
        let cleaned = clean_pasted(&text[piece.clone()]);
        let start = offset(text, cleaned);
        let span = start..start + cleaned.len();
        if let Some(kind) = IdKind::from_label(cleaned) {
            last = Some((piece.end, Some((kind, start))));
            continue;
        }
        last = Some((piece.end, None));
        let labelled =
            label.and_then(|(kind, at)| Some((WorkId::parse_as(kind, cleaned).ok()?, at)));
        if let Some((id, at)) = labelled {
            found.push((id, at..span.end));
        } else if could_be_unlabelled(cleaned)
            && let Some(id) = unlabelled(cleaned)
        {
            found.push((id, span));
        } else if cleaned.ends_with('/') {
            broken.push(Broken {
                from: label.map_or(span.start, |(_, at)| at),
                cleaned: span,
                label: label.map(|(kind, _)| kind),
            });
        }
    }
}

/// A piece ending in `/` that isn't an identifier, maybe because the line
/// broke after it (`https://doi.org/10.1017/` then the suffix): where it
/// starts, label included, its cleaned bytes and its label.
struct Broken {
    from: usize,
    cleaned: Range<usize>,
    label: Option<IdKind>,
}

/// The identifier a [`Broken`] piece ending its line makes with the start
/// of the next, and its span.
fn mend(text: &str, broken: &Broken) -> Option<(WorkId, Range<usize>)> {
    let bytes = text.as_bytes();
    let line_end = broken.cleaned.end + memchr(b'\n', &bytes[broken.cleaned.end..])?;
    if !text[broken.cleaned.end..line_end].trim().is_empty() {
        return None;
    }
    let next_start = line_end + 1;
    let next_end = memchr(b'\n', &bytes[next_start..]).map_or(text.len(), |i| next_start + i);
    let next = &text[next_start..next_end];
    let raw = &text[broken.cleaned.clone()];
    if !crate::references::link_continues(raw, next) {
        return None;
    }
    let indent = next.len() - next.trim_start().len();
    let joined = format!("{raw}{}", next.split_whitespace().next()?);
    let cleaned = clean_pasted(&joined);
    if offset(&joined, cleaned) != 0 || cleaned.len() <= raw.len() {
        return None;
    }
    let id = match broken.label {
        Some(kind) => WorkId::parse_as(kind, cleaned).ok()?,
        None if could_be_unlabelled(cleaned) => unlabelled(cleaned)?,
        None => return None,
    };
    Some((
        id,
        broken.from..next_start + indent + cleaned.len() - raw.len(),
    ))
}

/// The label the piece starting at `start` follows, if any: the last
/// piece before it on its line that doesn't clean away, if a label. `last`
/// is the last piece parsed, which every piece with a hit was.
fn label_before(
    text: &str,
    start: usize,
    last: Option<(usize, Option<(IdKind, usize)>)>,
) -> Option<(IdKind, usize)> {
    let bytes = text.as_bytes();
    let mut end = start;
    loop {
        // Back over what separates pieces, not past the line's start.
        while end > 0 {
            let (c, len) = char_before(text, end);
            if c == '\n' {
                return None;
            }
            let space = if c.is_ascii() {
                CLASSES[c as usize] & SPACE != 0
            } else {
                c.is_whitespace()
            };
            if !space {
                break;
            }
            end -= len;
        }
        if end == 0 {
            return None;
        }
        if let Some((last_end, label)) = last
            && last_end == end
        {
            return label;
        }
        let start = piece_start(text, end);
        let class = bytes[start..end].iter().fold(0, |class, &b| {
            class | if b < 0x80 { CLASSES[b as usize] } else { SOLID }
        });
        let cleaned = clean_pasted(&text[start..end]);
        if class & SOLID == 0 {
            if cleaned.is_empty() {
                end = start;
                continue;
            }
            return None;
        }
        return IdKind::from_label(cleaned).map(|kind| (kind, offset(text, cleaned)));
    }
}

/// Where the piece with its last byte before `end` starts.
fn piece_start(text: &str, end: usize) -> usize {
    let bytes = text.as_bytes();
    let mut at = end;
    while at > 0 {
        // A link's text ends at its `]`, before the target's `(`.
        if at < end && bytes[at] == b'(' && bytes[at - 1] == b']' {
            break;
        }
        let (c, len) = char_before(text, at);
        let space = if c.is_ascii() {
            CLASSES[c as usize] & SPACE != 0
        } else {
            c.is_whitespace()
        };
        if space {
            break;
        }
        at -= len;
    }
    at
}

/// The character ending at byte `at`, and its length.
fn char_before(text: &str, at: usize) -> (char, usize) {
    let c = text[..at].chars().next_back().unwrap_or_default();
    (c, c.len_utf8())
}

/// The first byte at or after `from` from `/` to `:`: a digit, `/` or `:`.
fn next_hit(bytes: &[u8], from: usize) -> Option<usize> {
    const CHUNK: usize = 32;
    let hit = |b: u8| b.wrapping_sub(b'/') <= b':' - b'/';
    let mut at = from;
    while let Some(chunk) = bytes.get(at..at + CHUNK) {
        // No early exit within a chunk, so it vectorises.
        if chunk.iter().fold(0, |any, &b| any | u8::from(hit(b))) != 0 {
            break;
        }
        at += CHUNK;
    }
    bytes
        .get(at..)?
        .iter()
        .position(|&b| hit(b))
        .map(|i| at + i)
}

fn newlines(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&b| b == b'\n').count()
}

fn memchr(needle: u8, bytes: &[u8]) -> Option<usize> {
    bytes.iter().position(|&b| b == needle)
}

fn memrchr(needle: u8, bytes: &[u8]) -> Option<usize> {
    bytes.iter().rposition(|&b| b == needle)
}

/// Byte classes of a piece.
const DIGIT: u8 = 1;
/// `/` or `:`, in every URL, DOI and prefixed identifier.
const LINK: u8 = 2;
const ALPHA: u8 = 4;
/// Not what [`clean_pasted`] can strip: the piece won't clean away.
const SOLID: u8 = 8;
/// Separates pieces.
const SPACE: u8 = 16;
/// `]`, which ends a piece before `(`.
const CLOSE: u8 = 32;

/// [`SPACE`], [`DIGIT`], … per ASCII byte.
const CLASSES: [u8; 128] = {
    let mut classes = [SOLID; 128];
    let strippable = b"([{<*_\"'`>.,;:)]}";
    let mut i = 0;
    while i < strippable.len() {
        classes[strippable[i] as usize] = 0;
        i += 1;
    }
    let mut b = 0;
    while b < 128 {
        let c = b as u8;
        if c.is_ascii_digit() {
            classes[b] |= DIGIT;
        } else if c.is_ascii_alphabetic() {
            classes[b] |= ALPHA;
        } else if c == b'/' || c == b':' {
            classes[b] |= LINK;
        } else if c == b'|' || matches!(c, b'\t' | b'\n' | 0x0B | 0x0C | b'\r' | b' ') {
            classes[b] = SPACE;
        } else if c == b']' {
            classes[b] |= CLOSE;
        }
        b += 1;
    }
    classes
};

/// Whitespace-separated pieces of a text from `at`, also split at
/// Markdown table bars and between a link's text and its target
/// (`[text](url)`), each with the classes of its bytes.
struct Pieces<'a> {
    text: &'a str,
    at: usize,
}

impl Iterator for Pieces<'_> {
    type Item = (Range<usize>, u8);

    fn next(&mut self) -> Option<Self::Item> {
        let bytes = self.text.as_bytes();
        let mut start = None;
        let mut class = 0;
        while let Some(&b) = bytes.get(self.at) {
            let i = self.at;
            let (len, c) = if b < 0x80 {
                (1, CLASSES[b as usize])
            } else {
                let c = self.text[i..].chars().next().unwrap_or_default();
                let solid = if c.is_whitespace() { SPACE } else { SOLID };
                (c.len_utf8(), solid)
            };
            self.at += len;
            if c & (SPACE | CLOSE) == 0 {
                start.get_or_insert(i);
                class |= c;
                continue;
            }
            // A link's text keeps its `]`, so its brackets stay balanced.
            let link = c & CLOSE != 0 && bytes.get(i + 1) == Some(&b'(');
            if c & SPACE != 0 || link {
                let end = if link { i + 1 } else { i };
                if let Some(start) = start {
                    return Some((start..end, class));
                }
            } else {
                start.get_or_insert(i);
                class |= c;
            }
        }
        start.map(|start| (start..bytes.len(), class))
    }
}

/// Whether [`unlabelled`] could find an identifier in a cleaned piece:
/// URLs and DOIs have a `/`, prefixed identifiers a known prefix before a
/// `:`, and of the rest only PMCIDs and Bookshelf IDs are recognised.
fn could_be_unlabelled(piece: &str) -> bool {
    piece.contains('/')
        || piece.split_once(':').is_some_and(|(head, _)| {
            IdKind::from_prefix(head).is_some() || IdKind::from_label(head).is_some()
        })
        || ["pmc", "nbk"].iter().any(|p| {
            piece
                .get(..3)
                .is_some_and(|head| head.eq_ignore_ascii_case(p))
        })
}

/// An identifier written without a separate label: any kind when prefixed
/// or a URL; otherwise only kinds whose shape can't be mistaken.
fn unlabelled(piece: &str) -> Option<WorkId> {
    let explicit = Url::parse(piece).is_some()
        || piece.split_once(':').is_some_and(|(head, _)| {
            IdKind::from_prefix(head).is_some() || IdKind::from_label(head).is_some()
        });
    match piece.parse::<WorkId>() {
        Ok(id) if explicit || matches!(id.kind(), IdKind::Doi | IdKind::Pmcid | IdKind::Nbk) => {
            Some(id)
        }
        Ok(_) => None,
        // A DOI inside something else, e.g. `doi.org/10.…` without a
        // scheme on an unknown host, or `see:10.…`.
        Err(_) => {
            let at = piece.find("10.")?;
            let before = piece[..at].chars().next_back();
            if before.is_some_and(char::is_alphanumeric) {
                return None;
            }
            WorkId::parse_as(IdKind::Doi, &piece[at..]).ok()
        }
    }
}

/// Byte offset of `part`, a subslice of `whole`.
fn offset(whole: &str, part: &str) -> usize {
    part.as_ptr() as usize - whole.as_ptr() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first implementation of `scan_line`, for comparison.
    fn reference_scan_line(line: &str) -> Vec<(WorkId, Range<usize>)> {
        let mut found = Vec::new();
        let mut label: Option<(IdKind, usize)> = None;
        for piece in reference_pieces(line) {
            let cleaned = clean_pasted(&line[piece.clone()]);
            if cleaned.is_empty() {
                continue;
            }
            let start = offset(line, cleaned);
            let span = start..start + cleaned.len();
            if let Some(kind) = IdKind::from_label(cleaned) {
                label = Some((kind, start));
                continue;
            }
            let labelled = label
                .take()
                .and_then(|(kind, at)| Some((WorkId::parse_as(kind, cleaned).ok()?, at)));
            if let Some((id, at)) = labelled {
                found.push((id, at..span.end));
            } else if let Some(id) = unlabelled(cleaned) {
                found.push((id, span));
            }
        }
        found
    }

    fn reference_pieces(line: &str) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        let mut start: Option<usize> = None;
        let mut chars = line.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            let link = c == ']' && chars.peek().is_some_and(|&(_, next)| next == '(');
            if c.is_whitespace() || c == '|' || link {
                let end = if link { i + 1 } else { i };
                out.extend(start.take().map(|s| s..end));
            } else {
                start.get_or_insert(i);
            }
        }
        out.extend(start.map(|s| s..line.len()));
        out
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(4000))]

        #[test]
        fn texts_scan_as_the_reference_does(
            parts in proptest::collection::vec(proptest::sample::select(vec![
                " ", "  ", "\t", "\n", "\r\n", "|", "](", "]", "(", ")", "[", "*", "_", "\"", "'", "`",
                ".", ",", ";", ":", "/", "-", "<", ">", "{", "}", "\u{85}", "\u{2028}",
                "é", "\u{1F600}", "0", "1", "9", "10.", "10.1056/NEJMoa1302097",
                "1234", "a", "x", "W", "PMID", "pmid", "PMCID", "PMC", "NBK", "doi",
                "DOI", "arXiv", "ISBN", "ISBN-13", "isbn10", "978-0-262-03561-3",
                "1706.03762", "https://", "http://", "doi.org/", "www.", "arxiv.org/abs/",
                "pubmed.ncbi.nlm.nih.gov/", "s2:", "corpusid:", "openalex:", "info:doi/",
                "hep-th/9901001", "0123456789abcdef0123456789abcdef01234567",
            ]), 0..40),
        ) {
            let text = parts.concat();
            let mut found = Vec::new();
            find(&text, &mut found, &mut Vec::new());
            let mut want = Vec::new();
            let mut at = 0;
            for line in text.split('\n') {
                want.extend(
                    reference_scan_line(line)
                        .into_iter()
                        .map(|(id, span)| (id, at + span.start..at + span.end)),
                );
                at += line.len() + 1;
            }
            proptest::prop_assert_eq!(found, want, "{:?}", text);
        }
    }

    /// (id, text as written) per mention.
    fn found(text: &str) -> Vec<(String, &str)> {
        scan(text)
            .into_iter()
            .map(|m| (m.id.to_string(), &text[m.span]))
            .collect()
    }

    #[test]
    fn prose_punctuation_and_wrappers_are_not_part_of_a_mention() {
        assert_eq!(
            found("as shown (doi:10.1056/NEJMoa1302097). Then *10.1001/jama.2014.2135*, and"),
            [
                (
                    "doi:10.1056/nejmoa1302097".into(),
                    "doi:10.1056/NEJMoa1302097"
                ),
                (
                    "doi:10.1001/jama.2014.2135".into(),
                    "10.1001/jama.2014.2135"
                ),
            ]
        );
    }

    #[test]
    fn links_broken_across_lines_are_rejoined() {
        let text = "3. Wakefield AJ. Lancet. 1998;351:637-641.\n   https://doi.org/10.1016/S0140-\n   6736(97)11096-0\nnext";
        assert_eq!(
            found(text),
            [(
                "doi:10.1016/s0140-6736(97)11096-0".into(),
                "https://doi.org/10.1016/S0140-\n   6736(97)11096-0"
            )]
        );
        let m = &scan(text)[0];
        assert_eq!((m.line, m.column), (2, 4));
        assert_eq!(
            found("doi:10.1016/j.cell.\r\n2020.01.001."),
            [(
                "doi:10.1016/j.cell.2020.01.001".into(),
                "doi:10.1016/j.cell.\r\n2020.01.001"
            )]
        );
        assert_eq!(
            found("DOI: 10.1016/j.ce\nll.2021.02.003 and PMID: 30252312"),
            [
                (
                    "doi:10.1016/j.cell.2021.02.003".into(),
                    "DOI: 10.1016/j.ce\nll.2021.02.003"
                ),
                ("pmid:30252312".into(), "PMID: 30252312"),
            ]
        );
    }

    #[test]
    fn pdf_look_alikes_are_cleaned_but_spans_are_as_written() {
        let text = "ﬁnding: doi:10.1038/s41586\u{2013}020\u{AD}-2649-2";
        let m = &scan(text)[0];
        assert_eq!(m.id.to_string(), "doi:10.1038/s41586-020-2649-2");
        assert_eq!(
            &text[m.span.clone()],
            "doi:10.1038/s41586\u{2013}020\u{AD}-2649-2"
        );
        assert_eq!(m.column, 9);
    }

    #[test]
    fn links_broken_after_a_slash_are_mended() {
        let text =
            "Disaster Med. 2015;30:1-9. http://dx.doi.org/10.1017/\n   S1049023X15004653.\nnext";
        assert_eq!(
            found(text),
            [(
                "doi:10.1017/s1049023x15004653".into(),
                "http://dx.doi.org/10.1017/\n   S1049023X15004653"
            )]
        );
        assert_eq!((scan(text)[0].line, scan(text)[0].column), (1, 28));
        assert_eq!(
            found("DOI: 10.1017/\nS1049023X15004653 and PMC6716493"),
            [
                (
                    "doi:10.1017/s1049023x15004653".into(),
                    "DOI: 10.1017/\nS1049023X15004653"
                ),
                ("pmcid:PMC6716493".into(), "PMC6716493"),
            ]
        );
        // Not at the end of its line, or nothing to mend it with: nothing.
        assert!(found("https://doi.org/10.1017/ and S1049023X15004653").is_empty());
        assert!(found("https://doi.org/10.1017/\n\nS1049023X15004653").is_empty());
    }

    #[test]
    fn dois_broken_at_a_hyphen_have_an_alternative() {
        let text = "see https://doi.org/10.1038/nat-\n   ure14539 and doi:10.1/x";
        let m = &scan(text)[0];
        assert_eq!(m.id.to_string(), "doi:10.1038/nat-ure14539");
        assert_eq!(
            m.alternative(text).map(|id| id.to_string()).as_deref(),
            Some("doi:10.1038/nature14539")
        );
        // Not broken at a hyphen, or not broken at all: none.
        let text = "doi:10.1016/j.cell.\n2020.01.001 doi:10.1038/s41586-020-2649-2";
        assert!(scan(text).iter().all(|m| m.alternative(text).is_none()));
    }

    #[test]
    fn links_ending_their_line_stay_whole() {
        assert_eq!(
            found("see doi:10.1038/nature14539.\nNature reviewed it, PMC6716493"),
            [
                ("doi:10.1038/nature14539".into(), "doi:10.1038/nature14539"),
                ("pmcid:PMC6716493".into(), "PMC6716493"),
            ]
        );
        assert_eq!(
            found("https://doi.org/10.1234/x.y-z\narXiv:1706.03762 https://pubmed.ncbi.nlm.nih.gov/31452104/").len(),
            3,
            "a link starting the next line is another"
        );
        assert_eq!(
            found("DOI: 10.1056/NEJMoa1302097.\n18. Smith A")[0].1,
            "DOI: 10.1056/NEJMoa1302097",
            "the next reference's number doesn't continue it"
        );
        assert_eq!(
            found("https://pubmed.ncbi.nlm.nih.gov/31452104/\nincidence")[0].1,
            "https://pubmed.ncbi.nlm.nih.gov/31452104/",
            "a word that leaves the id as it was doesn't continue it"
        );
        assert_eq!(
            found("https://pubmed.ncbi.nlm.nih.gov/31452104/\n10.1001/jama.2014.2135").len(),
            2,
            "a continuation that turns one kind into another is another mention"
        );
        for next in [
            "meta-analysis",
            "10.1001/jama.2014.2135",
            "10.5 mg",
            "www.example.com",
        ] {
            let text = format!("doi:10.1016/S0140-6736(10)60835-5\n{next}");
            assert_eq!(found(&text)[0].1, "doi:10.1016/S0140-6736(10)60835-5");
        }
        for next in ["2015 was a good year", "2015. A good year"] {
            let text = format!("doi:10.1038/nature14539\n{next}");
            assert_eq!(found(&text)[0].1, "doi:10.1038/nature14539");
        }
    }

    #[test]
    fn brackets_inside_a_doi_stay() {
        assert_eq!(
            found("Lancet (10.1016/S0140-6736(10)60835-5)."),
            [(
                "doi:10.1016/s0140-6736(10)60835-5".into(),
                "10.1016/S0140-6736(10)60835-5"
            )]
        );
    }

    #[test]
    fn labels_join_their_values() {
        assert_eq!(
            // The last isn't a DOI: registrant codes have four digits or more.
            found("PMID: 30252312; ISBN 978-0-262-03561-3, doi: 10.1/abcd"),
            [
                ("pmid:30252312".into(), "PMID: 30252312"),
                ("isbn:9780262035613".into(), "ISBN 978-0-262-03561-3"),
            ]
        );
        assert_eq!(
            found("DOI: 10.1056/NEJMoa1302097"),
            [(
                "doi:10.1056/nejmoa1302097".into(),
                "DOI: 10.1056/NEJMoa1302097"
            )]
        );
    }

    #[test]
    fn markdown_links_and_tables() {
        assert_eq!(
            found("[PEITHO](https://doi.org/10.1056/NEJMoa1302097) |PMC6716493| [[NBK526056]]"),
            [
                (
                    "doi:10.1056/nejmoa1302097".into(),
                    "https://doi.org/10.1056/NEJMoa1302097"
                ),
                ("pmcid:PMC6716493".into(), "PMC6716493"),
                ("nbk:NBK526056".into(), "NBK526056"),
            ]
        );
    }

    #[test]
    fn ambiguous_shapes_need_a_prefix_or_url() {
        assert!(found("In 1706.03762 cases, W1 and 31452104 patients; p 2017.1234").is_empty());
        assert_eq!(
            found("arXiv:1706.03762v5 and https://pubmed.ncbi.nlm.nih.gov/31452104/"),
            [
                ("arxiv:1706.03762v5".into(), "arXiv:1706.03762v5"),
                (
                    "pmid:31452104".into(),
                    "https://pubmed.ncbi.nlm.nih.gov/31452104/"
                ),
            ]
        );
    }

    #[test]
    fn lines_columns_and_spans_locate_each_mention() {
        let text = "Title\r\n  café 10.1234/abcd\nsee PMC1";
        let m = scan(text);
        assert_eq!((m[0].line, m[0].column), (2, 8), "columns count characters");
        assert_eq!(&text[m[0].span.clone()], "10.1234/abcd");
        assert_eq!((m[1].line, m[1].column), (3, 5));
        assert_eq!(&text[m[1].span.clone()], "PMC1");
    }
}
