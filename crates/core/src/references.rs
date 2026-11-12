//! Splitting a pasted reference list into one citation each, offline.
//!
//! Lists arrive numbered (`1.`, `[1]`, `(1)`), bulleted, separated by blank
//! lines, or one per line, with lines wrapped by PDFs and word processors.
//! The heuristics, in order:
//!
//! 1. **Numbered or bulleted**: two or more lines start with a marker
//!    (numbers in sequence, so `2015. Title` inside a reference doesn't
//!    count). Each marker starts a reference; other lines continue it.
//! 2. **Paragraphs**: blank lines separate references; within one, a line
//!    starts another reference only if it looks like an author list after
//!    a line that ended a sentence.
//! 3. **Lines**: one reference per line when every line has a year or an
//!    identifier, ends as a reference ends, and doesn't start in lower
//!    case; otherwise wrapped, split as within a paragraph.
//!
//! Wrapped lines are joined with a space; a word hyphenated across lines
//! is mended, and DOIs and URLs broken after `/` or `-` are rejoined.
//! Headings such as "References" are dropped.

use std::ops::Range;

use crate::Alternative;
use crate::fold::Folded;

/// One reference found in a list.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Reference {
    /// On one line, without its number or bullet.
    pub text: String,
    /// Byte range in the list: from after the marker to the end of its
    /// last line.
    pub span: Range<usize>,
    /// DOIs broken across lines at a hyphen, read without it: to try if
    /// the DOI as printed doesn't exist.
    pub alternatives: Vec<Alternative>,
}

/// The references in `text`, in order. Text from PDFs is cleaned first
/// (ligatures, invisible characters, look-alike dashes and quotes); spans
/// point into `text` as given.
pub fn split_references(text: &str) -> Vec<Reference> {
    let folded = Folded::new(text);
    let list = List::new(&folded.text).split_inline();
    let groups = list.groups();
    references(text, &folded, &list, groups)
}

/// A line of a reference list as laid out, with its distance from the
/// left edge (any unit, such as PDF points).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct IndentedLine {
    pub text: String,
    pub indent: f64,
}

impl IndentedLine {
    pub fn new(text: impl Into<String>, indent: f64) -> Self {
        IndentedLine {
            text: text.into(),
            indent,
        }
    }
}

/// One reference found in laid-out lines.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LineReference {
    /// On one line, without its number or bullet.
    pub text: String,
    /// Indices of the lines it spans.
    pub lines: Range<usize>,
    pub alternatives: Vec<Alternative>,
}

/// The references in laid-out lines (as from a PDF), using the layout
/// plain text loses: with a hanging indent, references start at the
/// margin; with a first-line indent, indented. Numbered lists, and lines
/// all at one indent, are split by the text rules of [`split_references`].
pub fn split_lines(lines: &[IndentedLine]) -> Vec<LineReference> {
    let text = lines
        .iter()
        // One line each, whatever breaks they hold (cleaning makes all of
        // these line breaks).
        .map(|l| {
            l.text
                .replace(['\r', '\n', '\u{85}', '\u{2028}', '\u{2029}'], " ")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let folded = Folded::new(&text);
    let list = List::new(&folded.text);
    let groups = match starts_by_indent(lines) {
        Some(starts) if list.numbered().is_none() => {
            let mut groups: Vec<Vec<Range<usize>>> = Vec::new();
            for (i, line) in list.lines.iter().enumerate() {
                let Some(line) = line else { continue };
                // Whatever the indent, a line in lower case or closing a
                // bracket, or after a word broken at a hyphen, continues
                // the reference ("sur-" then "gery: a systematic…").
                let text = &folded.text[line.clone()];
                match groups.last_mut() {
                    Some(group)
                        if !starts[i]
                            || continues(text)
                            || group
                                .last()
                                .is_some_and(|l| folded.text[l.clone()].ends_with('-')) =>
                    {
                        group.push(line.clone())
                    }
                    _ => groups.push(vec![line.clone()]),
                }
            }
            groups
        }
        _ => list.groups(),
    };
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let line_of = |byte: usize| line_starts.partition_point(|&s| s <= byte) - 1;
    references(&text, &folded, &list, groups)
        .into_iter()
        .map(|r| LineReference {
            lines: line_of(r.span.start)..line_of(r.span.end.saturating_sub(1)) + 1,
            text: r.text,
            alternatives: r.alternatives,
        })
        .collect()
}

/// Which lines start a reference, by indent: `None` if the lines don't
/// differ enough to tell. Indents within half the gap to the next level
/// count as the same, so jitter in a PDF's positions doesn't matter.
fn starts_by_indent(lines: &[IndentedLine]) -> Option<Vec<bool>> {
    let content: Vec<&IndentedLine> = lines.iter().filter(|l| !l.text.trim().is_empty()).collect();
    let margin = content
        .iter()
        .map(|l| l.indent)
        .fold(f64::INFINITY, f64::min);
    let next = content
        .iter()
        .map(|l| l.indent)
        .filter(|&i| i > margin + 1.0)
        .fold(f64::INFINITY, f64::min);
    if !next.is_finite() {
        return None;
    }
    let at_margin = |l: &IndentedLine| l.indent - margin < (next - margin) / 2.0;
    let first = content.iter().find(|l| !is_heading(l.text.trim()))?;
    let starting = at_margin(first);
    Some(
        lines
            .iter()
            .map(|l| !l.text.trim().is_empty() && at_margin(l) == starting)
            .collect(),
    )
}

/// Groups of lines (ranges in `folded`) as references in `text`.
fn references(
    text: &str,
    folded: &Folded,
    list: &List,
    groups: Vec<Vec<Range<usize>>>,
) -> Vec<Reference> {
    groups
        .into_iter()
        .map(|g| {
            let span = folded.original(&(g[0].start..g[g.len() - 1].end));
            let written = &text[span.clone()];
            let alternatives = crate::id::scan(written)
                .iter()
                .filter_map(|m| Some(Alternative::new(m.id.clone(), m.alternative(written)?)))
                .collect();
            Reference {
                text: list.join(&g),
                span,
                alternatives,
            }
        })
        .filter(|r| !is_heading(&r.text))
        .collect()
}

struct List<'a> {
    text: &'a str,
    /// Every line trimmed, as a byte range; `None` for blank ones.
    lines: Vec<Option<Range<usize>>>,
}

impl<'a> List<'a> {
    fn new(text: &'a str) -> Self {
        let mut lines = Vec::new();
        let mut at = 0;
        for raw in text.split_inclusive('\n') {
            let trimmed = raw.trim();
            lines.push((!trimmed.is_empty()).then(|| {
                let start = at + (raw.len() - raw.trim_start().len());
                start..start + trimmed.len()
            }));
            at += raw.len();
        }
        List { text, lines }
    }

    /// Lines holding references numbered inline, as a PDF's list copied
    /// onto one line ("… 01159.x. [21] Finucane … [22] …"), split before
    /// each number. Only `[n]` markers in sequence count: three or more,
    /// or two with one starting the line, each starting the line or after
    /// a full stop and before a capital, and every piece dated. Citations
    /// in prose ("as shown. [3] The") rarely meet all of these.
    fn split_inline(mut self) -> Self {
        let lines = std::mem::take(&mut self.lines);
        for line in lines {
            match line.as_ref().and_then(|l| self.inline_starts(l)) {
                Some(starts) => {
                    let range = line.expect("has starts");
                    let bounds: Vec<usize> = std::iter::once(range.start)
                        .chain(starts)
                        .chain(std::iter::once(range.end))
                        .collect();
                    for w in bounds.windows(2) {
                        let piece = &self.text[w[0]..w[1]];
                        let start = w[0] + (piece.len() - piece.trim_start().len());
                        let end = w[0] + piece.trim_end().len();
                        if start < end {
                            self.lines.push(Some(start..end));
                        }
                    }
                }
                None => self.lines.push(line),
            }
        }
        self
    }

    /// Where a line's inline-numbered references start, after its own
    /// start: see [`List::split_inline`].
    fn inline_starts(&self, line: &Range<usize>) -> Option<Vec<usize>> {
        let text = self.line(line);
        let mut markers: Vec<(usize, u32)> = Vec::new();
        for (at, _) in text.match_indices('[') {
            let Some((Marker::Number(n), used)) = marker(&text[at..]) else {
                continue;
            };
            let follows_stop = text[..at].trim_end();
            let next = text[at + used..].chars().next();
            if (follows_stop.is_empty() || follows_stop.ends_with('.'))
                && next.is_some_and(char::is_uppercase)
            {
                markers.push((at, n));
            }
        }
        // The longest run numbered in sequence.
        let mut runs: Vec<Vec<(usize, u32)>> = Vec::new();
        for m in markers {
            match runs
                .iter_mut()
                .find(|r| r.last().is_some_and(|l| l.1 + 1 == m.1))
            {
                Some(run) => run.push(m),
                None => runs.push(vec![m]),
            }
        }
        let run = runs.into_iter().max_by_key(Vec::len)?;
        let at_start = run.first().is_some_and(|m| m.0 == 0);
        if run.len() < 3 && !(run.len() == 2 && at_start) {
            return None;
        }
        let starts: Vec<usize> = run
            .iter()
            .filter(|m| m.0 > 0)
            .map(|m| line.start + m.0)
            .collect();
        let bounds: Vec<usize> = std::iter::once(line.start)
            .chain(starts.iter().copied())
            .chain(std::iter::once(line.end))
            .collect();
        // Every piece dated, or all but one of four or more: a reference
        // the style or copy left without a date doesn't undo the list.
        let pieces = bounds.len() - 1;
        let undated = bounds
            .windows(2)
            .filter(|w| !dated(&self.text[w[0]..w[1]]))
            .count();
        (undated == 0 || (undated == 1 && pieces >= 4)).then_some(starts)
    }

    /// Groups by the text rules: numbers or bullets, else paragraphs, else
    /// lines.
    fn groups(&self) -> Vec<Vec<Range<usize>>> {
        self.numbered().unwrap_or_else(|| {
            let paragraphs: Vec<Vec<Range<usize>>> = self
                .lines
                .split(Option::is_none)
                .filter(|p| !p.is_empty())
                .map(|p| p.iter().flatten().cloned().collect())
                .collect();
            let alone = paragraphs.len() == 1;
            paragraphs
                .into_iter()
                .flat_map(|p| self.unnumbered(p, alone))
                .collect()
        })
    }

    fn line(&self, r: &Range<usize>) -> &'a str {
        &self.text[r.clone()]
    }

    /// Groups by number or bullet, if two or more lines start with one.
    /// Lines before the first marker are dropped (a heading, usually).
    fn numbered(&self) -> Option<Vec<Vec<Range<usize>>>> {
        let mut groups: Vec<Vec<Range<usize>>> = Vec::new();
        let mut last: Option<Marker> = None;
        for line in self.lines.iter().flatten() {
            let starts = marker(self.line(line)).filter(|(m, _)| match (last, m) {
                (None, _) => true,
                (Some(Marker::Number(n)), Marker::Number(m)) => *m == n + 1,
                (Some(Marker::Bullet), Marker::Bullet) => true,
                _ => false,
            });
            match (starts, groups.last_mut()) {
                (Some((m, used)), _) => {
                    last = Some(m);
                    // A marker alone on its line: the text follows.
                    let first = line.start + used..line.end;
                    groups.push(Vec::new());
                    if !first.is_empty() {
                        groups.last_mut().expect("just pushed").push(first);
                    }
                }
                (None, Some(group)) => group.push(line.clone()),
                // Before the first number: a heading, or the end of a
                // reference the copy began in the middle of.
                (None, None) if dated(self.line(line)) => groups.push(vec![line.clone()]),
                (None, None) => {}
            }
        }
        // One marker counts only on the first line ("1. Smith J. …"): later,
        // it may be a wrapped line ("2. ed. Berlin").
        let first_marked = self
            .lines
            .iter()
            .flatten()
            .next()
            .is_some_and(|l| marker(self.line(l)).is_some());
        groups.retain(|g| !g.is_empty());
        (groups.len() >= 2 || groups.len() == 1 && first_marked).then_some(groups)
    }

    /// Groups lines without markers: each its own reference if `alone`
    /// (the whole list) and they read as one per line; otherwise split
    /// where an author list follows a finished sentence.
    fn unnumbered(&self, lines: Vec<Range<usize>>, alone: bool) -> Vec<Vec<Range<usize>>> {
        let one_per_line = alone
            && lines.iter().all(|l| dated(self.line(l)))
            && lines.windows(2).all(|w| {
                let (line, next) = (self.line(&w[0]), self.line(&w[1]));
                ends_reference(line) && !continues(next) && !link_continues(line, next)
            });
        if one_per_line {
            return lines.into_iter().map(|l| vec![l]).collect();
        }
        // Each group joined as it grows, not again for every line.
        let mut groups: Vec<(Vec<Range<usize>>, Joined)> = Vec::new();
        for line in lines {
            let text = self.line(&line);
            match groups.last_mut() {
                // A reference so far without a year or identifier (an author
                // line ending "et al.") still needs its title and the rest.
                Some((group, joined))
                    if !(ends_sentence(&joined.text)
                        && joined.dated
                        && starts_with_author(text)) =>
                {
                    group.push(line);
                    joined.push(text);
                }
                _ => groups.push((vec![line], Joined::new(text))),
            }
        }
        groups.into_iter().map(|(group, _)| group).collect()
    }

    /// A group's lines as one: see [`push_line`].
    fn join(&self, group: &[Range<usize>]) -> String {
        let mut out = String::new();
        for line in group {
            push_line(&mut out, self.line(line));
        }
        out
    }
}

/// Appends a line to lines joined so far: after a space, or with none
/// where a word, DOI or URL was broken across lines. False if it runs on
/// from the last character, so a number there may now be longer.
fn push_line(out: &mut String, next: &str) -> bool {
    let first = next.chars().next().unwrap_or(' ');
    let apart = if out.is_empty() {
        // Nothing to join to.
        true
    } else if out.ends_with('-') && first.is_lowercase() && hyphenated_word(out) {
        // Letters either side, so no number runs on.
        out.pop();
        true
    } else if !link_continues(out, next) {
        out.push(' ');
        true
    } else {
        false
    };
    out.push_str(next);
    apart
}

/// Lines joined as [`List::join`] does, and whether they are [`dated`],
/// kept up to date as each line is added.
struct Joined {
    text: String,
    dated: bool,
}

impl Joined {
    fn new(line: &str) -> Self {
        Joined {
            text: line.to_string(),
            dated: dated(line),
        }
    }

    /// Adds a line. Undated text before it stays undated, so only the new
    /// line and the end before it are read: as much as the longest mark
    /// (14 characters) could overlap, back to the start of a number.
    fn push(&mut self, line: &str) {
        let tail = self.text.char_indices().rev().nth(15).map_or(0, |(i, _)| i);
        let tail = self.text[..tail]
            .trim_end_matches(|c: char| c.is_ascii_digit())
            .len();
        let apart = push_line(&mut self.text, line);
        if !self.dated {
            self.dated = dated(&self.text[tail..]);
        } else if !apart {
            // A year running on ("2019" then "5") is a year no longer.
            self.dated = dated(&self.text);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Marker {
    Number(u32),
    Bullet,
}

/// The marker a line starts with, and its length with the space after.
fn marker(line: &str) -> Option<(Marker, usize)> {
    let spaced = |rest: &str, used: usize, needs_space: bool| {
        let gap = rest.len() - rest.trim_start().len();
        (gap > 0 || !needs_space).then_some(used + gap)
    };
    if let Some(rest) = line.strip_prefix(['•', '–', '-', '*', '·']) {
        return Some((Marker::Bullet, spaced(rest, line.len() - rest.len(), true)?));
    }
    let (open, close): (usize, &[char]) = match line.as_bytes().first()? {
        b'[' => (1, &[']']),
        b'(' => (1, &[')']),
        _ => (0, &['.', ')']),
    };
    let inner = &line[open..];
    let digits = inner.len() - inner.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if !(1..=4).contains(&digits) {
        return None;
    }
    let n: u32 = inner[..digits].parse().ok()?;
    let rest = inner[digits..].strip_prefix(close)?;
    // `[1]Smith` is unambiguous; `1.` needs a space, so `1.5` isn't one.
    Some((
        Marker::Number(n),
        spaced(rest, line.len() - rest.len(), open == 0)?,
    ))
}

/// Ends as a reference does: a full stop, page numbers, a closing bracket,
/// or an identifier.
fn ends_reference(line: &str) -> bool {
    ends_sentence(line) || line.ends_with(|c: char| c.is_ascii_digit() || c == ')' || c == ']')
}

/// Ends with a full stop (or `?`, `!`), or with a whole DOI or URL, as
/// references often do without one.
fn ends_sentence(line: &str) -> bool {
    line.ends_with(['.', '?', '!']) || in_link(line) && !line.ends_with(['/', '-'])
}

/// Has a year (1600 to 2099) or an identifier, as a whole reference does.
fn dated(line: &str) -> bool {
    let year = line
        .split(|c: char| !c.is_ascii_digit())
        .any(|n| n.len() == 4 && ("1600".."2100").contains(&n));
    // What styles write for a work without a year, or a web page's date.
    let lower = line.to_lowercase();
    let undated = [
        "n.d.",
        "n. d.",
        "no date",
        "accessed",
        "[cited",
        "available from",
    ]
    .iter()
    .any(|m| lower.contains(m));
    year || undated || line.contains("doi") || line.contains("10.") || line.contains("http")
}

/// Starts as a wrapped line's continuation does: in lower case, or with
/// punctuation.
fn continues(line: &str) -> bool {
    line.starts_with(|c: char| c.is_lowercase() || ",;:)&".contains(c))
}

/// Starts with a surname and initials or a comma, as author lists do:
/// "Smith J,", "Smith JA.", "Smith, J.", "SMITH, J.", "O'Neil, K.".
fn starts_with_author(line: &str) -> bool {
    let surname_len = line
        .char_indices()
        .take_while(|(_, c)| c.is_alphabetic() || "'’-".contains(*c))
        .last()
        .map_or(0, |(i, c)| i + c.len_utf8());
    let surname = &line[..surname_len];
    if surname.chars().count() < 2 || !surname.starts_with(char::is_uppercase) {
        return false;
    }
    let rest = &line[surname_len..];
    // "Smith, J." or "Smith, John": a capital after the comma, never a
    // word in lower case ("National, regional, and global trends").
    if let Some(after) = rest.strip_prefix(',') {
        return after.trim_start().starts_with(char::is_uppercase);
    }
    // Initials: one to three capitals, maybe dotted, then a break.
    let Some(rest) = rest.strip_prefix(' ') else {
        return false;
    };
    let initials: String = rest
        .chars()
        .take_while(|c| c.is_uppercase() || *c == '.')
        .collect();
    let capitals = initials.chars().filter(|c| c.is_uppercase()).count();
    let after = rest[initials.len()..].chars().next();
    (1..=3).contains(&capitals) && after.is_none_or(|c| ", .;".contains(c))
}

/// A word broken with a hyphen at the end of `text`: letters before it.
fn hyphenated_word(text: &str) -> bool {
    let word = text.rsplit(char::is_whitespace).next().unwrap_or_default();
    !in_link(text) && word.trim_end_matches('-').chars().all(char::is_alphabetic)
}

/// `next` continues a DOI or URL that `text` ends with, broken where PDFs
/// break them: after `/`, `-` or `_` (`10.1038/s41586-` + `020-2649-2`),
/// after a dot before a digit or lower case (`j.cell.` + `2020.01.001`),
/// or mid-word before more of the link (`j.ce` + `ll.2020.01.001`, not
/// `2015` or `2015.`).
/// Hyphens are kept as printed: DOIs have real ones, and typesetters
/// rarely add their own inside a link.
pub(crate) fn link_continues(text: &str, next: &str) -> bool {
    let token = next.split_whitespace().next().unwrap_or_default();
    let Some(first) = token.chars().next() else {
        return false;
    };
    if !in_link(text) || !first.is_alphanumeric() || starts_link(token) || is_number(token) {
        return false;
    }
    // A line that is one word of a DOI suffix's characters, digits among
    // them, is the rest of the link, however the line broke: PDFs wrap a
    // suffix anywhere (`…/HJH.0b013e` then `3282f35c67`). Not a number,
    // nor an identifier of its own.
    let suffix = |c: char| c.is_ascii_alphanumeric() || "._;()/:-<>".contains(c);
    if next.trim() == token
        && !text.ends_with('.')
        && token.chars().all(suffix)
        && token.bytes().any(|b| b.is_ascii_digit())
        && !plain_number(token.trim_end_matches(|c: char| !c.is_alphanumeric()))
        && crate::id::scan(token).is_empty()
    {
        return true;
    }
    let lower_or_digit = first.is_lowercase() || first.is_ascii_digit();
    // A DOI of its own, unless the line broke right before it
    // (`https://doi.org/` then `10.…`).
    let doi = token.starts_with("10.") && token.contains('/');
    if text.ends_with(['/', '-', '_']) {
        true
    } else if text.ends_with('.') {
        lower_or_digit && !doi
    } else {
        // Mid-word: only before more of a link, never a word, a
        // hyphenated word or a number.
        let inner = token.trim_end_matches(|c: char| !c.is_alphanumeric());
        lower_or_digit
            && !doi
            && inner.parse::<f64>().is_err()
            && (inner.contains(['.', '/', '_']) || (first.is_ascii_digit() && inner.contains('-')))
    }
}

/// A number that reads as itself ("3.14", "2019", "797"), not a run of a
/// DOI's digits ("0000659", "00489").
fn plain_number(token: &str) -> bool {
    let digits = token.bytes().all(|b| b.is_ascii_digit());
    (token.contains('.') && token.parse::<f64>().is_ok())
        || (digits && token.len() < 5 && !token.starts_with('0'))
}

/// `token` is a link of its own, not the rest of one.
fn starts_link(token: &str) -> bool {
    let lower = token.get(..8).unwrap_or(token).to_ascii_lowercase();
    ["http://", "https://", "www."]
        .iter()
        .any(|p| lower.starts_with(p))
        || token
            .split_once(':')
            .is_some_and(|(head, _)| crate::id::IdKind::from_prefix(head).is_some())
}

/// `token` numbers a list item (`18.`).
fn is_number(token: &str) -> bool {
    token
        .strip_suffix('.')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// `text` ends inside a DOI or URL.
fn in_link(text: &str) -> bool {
    let word = text.rsplit(char::is_whitespace).next().unwrap_or_default();
    let starts = |p: &str| {
        word.get(..p.len())
            .is_some_and(|h| h.eq_ignore_ascii_case(p))
    };
    starts("http") || starts("doi:") || word.starts_with("10.")
}

fn is_heading(text: &str) -> bool {
    let t = text.trim_end_matches(':').trim().to_lowercase();
    matches!(
        t.as_str(),
        "references"
            | "reference list"
            | "bibliography"
            | "works cited"
            | "literature cited"
            | "sources"
            | "citations"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(list: &str) -> Vec<String> {
        split_references(list).into_iter().map(|r| r.text).collect()
    }

    #[test]
    fn numbered_lists_split_at_each_number_in_sequence() {
        let list = "References\n\
            1. LeCun Y, Bengio Y, Hinton G. Deep learning. Nature.\n\
            2015;521:436-44.\n\
            2. Vaswani A, et al. Attention is all you need. NeurIPS 2017.\n";
        assert_eq!(
            texts(list),
            [
                "LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015;521:436-44.",
                "Vaswani A, et al. Attention is all you need. NeurIPS 2017.",
            ]
        );
    }

    #[test]
    fn bracketed_and_parenthesised_numbers() {
        let list = "[1] Smith J. One. 2020.\n[2]Jones K. Two. 2021.\n";
        assert_eq!(texts(list), ["Smith J. One. 2020.", "Jones K. Two. 2021."]);
        let list = "(1) Smith J. One. 2020.\n(2) Jones K. Two. 2021.\n";
        assert_eq!(texts(list), ["Smith J. One. 2020.", "Jones K. Two. 2021."]);
    }

    #[test]
    fn numbers_out_of_sequence_continue_the_reference() {
        let list = "1. Smith J. A study of things. Journal\n\
            2015. 12(3):1-9.\n\
            2. Jones K. Another. 2021.\n";
        assert_eq!(
            texts(list),
            [
                "Smith J. A study of things. Journal 2015. 12(3):1-9.",
                "Jones K. Another. 2021.",
            ]
        );
    }

    #[test]
    fn bullets_mark_references() {
        let list = "• Smith J. One. 2020.\n• Jones K. Two,\n  continued. 2021.\n";
        assert_eq!(
            texts(list),
            ["Smith J. One. 2020.", "Jones K. Two, continued. 2021."]
        );
    }

    #[test]
    fn blank_lines_separate_wrapped_references() {
        let list = "Vaswani A, Shazeer N. Attention is all you need.\n\
            Advances in Neural Information Processing Systems. 2017.\n\
            \n\
            LeCun Y, Bengio Y, Hinton G. Deep learning.\n\
            Nature. 2015;521:436-44.\n";
        assert_eq!(
            texts(list),
            [
                "Vaswani A, Shazeer N. Attention is all you need. Advances in Neural Information Processing Systems. 2017.",
                "LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015;521:436-44.",
            ]
        );
    }

    #[test]
    fn one_per_line() {
        let list = "Smith J. One. 2020.\n\
            World Health Organization. Two. 2021.\n\
            Jones K. Three. https://example.org/three\n";
        assert_eq!(texts(list).len(), 3);
    }

    #[test]
    fn wrapped_lines_without_blank_lines_split_at_authors() {
        let list = "Smith, J. (2020). A study of\n\
            things. Journal of Studies, 12, 1-9.\n\
            Jones, K. (2021). Another study. Nature,\n\
            521, 436.\n";
        assert_eq!(
            texts(list),
            [
                "Smith, J. (2020). A study of things. Journal of Studies, 12, 1-9.",
                "Jones, K. (2021). Another study. Nature, 521, 436.",
            ]
        );
    }

    #[test]
    fn hyphenation_and_broken_links_are_mended() {
        let list = "1. Smith J. Neuro-\nlogical find-\nings. doi:10.1038/\nnature14539\n\
            2. Jones K. https://example.org/a-\nb/c\n";
        assert_eq!(
            texts(list),
            [
                "Smith J. Neurological findings. doi:10.1038/nature14539",
                "Jones K. https://example.org/a-b/c",
            ]
        );
    }

    #[test]
    fn links_broken_by_pdf_extraction_are_rejoined_as_printed() {
        let list = "[1] Smith J. Cells. Cell. 2020. https://doi.org/10.1016/j.cell.\n\
            2020.01.001\n\
            [2] Jones K. Folds. Nature. 2020. doi:10.1038/s41586-\n\
            020-2649-2\n\
            [3] Brown L. Stems. https://doi.org/10.1016/j.ce\n\
            ll.2021.02.003\n";
        assert_eq!(
            texts(list),
            [
                "Smith J. Cells. Cell. 2020. https://doi.org/10.1016/j.cell.2020.01.001",
                "Jones K. Folds. Nature. 2020. doi:10.1038/s41586-020-2649-2",
                "Brown L. Stems. https://doi.org/10.1016/j.cell.2021.02.003",
            ]
        );
        // Unnumbered, the continuation isn't a reference of its own.
        let list = "Smith J. Cells. Cell. 2020. doi:10.1016/j.cell.\n2020.01.001\n\
            Jones K. Folds. Nature. 2020.\n";
        assert_eq!(texts(list).len(), 2);
        // A link that ends the line, then a new sentence, stays apart.
        assert_eq!(
            texts("1. Smith J. doi:10.1038/x.\nNature. 2020.\n2. Jones K. 2021.")[0],
            "Smith J. doi:10.1038/x. Nature. 2020."
        );
    }

    #[test]
    fn a_single_numbered_reference_loses_its_number() {
        assert_eq!(
            texts("1. LeCun Y. Deep learning. Nature. 2015."),
            ["LeCun Y. Deep learning. Nature. 2015."]
        );
        // A number further down is part of the reference.
        assert_eq!(
            texts("Smith J. A book.\n2. ed. Berlin: Springer; 2010."),
            ["Smith J. A book. 2. ed. Berlin: Springer; 2010."]
        );
    }

    #[test]
    fn references_numbered_inline_split_at_each_number() {
        // Dogfood: a PDF's list copied by Preview onto one line.
        let list = "Sanchez A, et al. Title one. J Thromb Haemost. 2011;9:1-8. doi:10.1111/j.1538-7836.2010.01159.x. \
            [21] Finucane T, Smith B. Title two. Lancet. 2012;379:10-12. \
            [22] Ortel TL. Title three. Blood Adv. 2020;4:4693. \
            [23] Konstantinides SV. Title four. Eur Heart J. 2020;41:543.";
        assert_eq!(
            texts(list),
            [
                "Sanchez A, et al. Title one. J Thromb Haemost. 2011;9:1-8. doi:10.1111/j.1538-7836.2010.01159.x.",
                "Finucane T, Smith B. Title two. Lancet. 2012;379:10-12.",
                "Ortel TL. Title three. Blood Adv. 2020;4:4693.",
                "Konstantinides SV. Title four. Eur Heart J. 2020;41:543.",
            ]
        );
        // Undated, as styles write it ("n.d."), or one piece of four
        // without a date.
        let undated = "Sanchez A. One. Blood. 2011;9:1-8. \
            [21] Finucane T. Two. Lancet. 2012;379:10. \
            [22] Defense Health Board. Three. Falls Church, VA, n.d. \
            [23] Konstantinides SV. Four. Eur Heart J. 2020;41:543.";
        assert_eq!(texts(undated).len(), 4);
        assert_eq!(texts(&undated.replace("n.d.", "undated.")).len(), 4);
        // Two, when the first starts the line.
        assert_eq!(
            texts("[1] Smith J. One. 2019. [2] Jones K. Two. 2020.").len(),
            2
        );
        // In-text citations: out of sequence, not after a full stop, or
        // pieces without a year.
        for prose in [
            "Shown before [3] and since. [4] The trial. [5] Then 2019.",
            "Smith J. Title. 2019. [3] Jones. 2020. [7] Brown. 2021. [9] Lee. 2022.",
            "As shown. [1] The first. [2] The second. [3] The third 2019.",
        ] {
            assert_eq!(texts(prose).len(), 1, "{prose}");
        }
    }

    #[test]
    fn unnumbered_vancouver_lists_split_however_lines_break() {
        // #40: copied from a journal PDF; Preview's copies break lines
        // with `\r` or Unicode separators.
        let list = "Benjamin EJ, Muntner P, Alonso A, et al. Heart disease\n\
            and stroke statistics-2019 update. Circulation\n\
            2019;139:e56-528. https://doi.org/10.1161/CIR.0000000000000659\n\
            Schmieder RE, Kjeldsen SE, Julius S, et al. Reduced incidence\n\
            of new-onset atrial fibrillation: the VALUE trial. J\n\
            Hypertens 2008;26:403-11. https://doi.org/10.1097/HJH.0b013e3282f35c67\n\
            Chokesuwattanaskul R, Thongprayoon C, Bathini T, Sharma K, Watthanasuntorn K, Lertjitbanjong\n\
            P, et al. Incident atrial fibrillation in patients undergoing bariatric sur-\n\
            gery: a systematic review and meta-analysis. Intern Med J 2020;50:810-7. https://doi.org/10.1111/imj.14436";
        for sep in ["\n", "\r\n", "\r", "\u{2028}", "\u{2029}"] {
            let found = texts(&list.replace('\n', sep));
            assert_eq!(found.len(), 3, "{sep:?}: {found:?}");
            assert!(
                found[2].contains("bariatric surgery: a systematic"),
                "{found:?}"
            );
        }
    }

    #[test]
    fn a_doi_wrapped_onto_a_line_of_its_own_ends_its_reference() {
        // #40, as copied: each DOI's end alone on the next line.
        let list = "Benjamin EJ, Muntner P, et al. Heart disease\n\
            and stroke statistics-2019 update. Circulation\n\
            2019;139:e56-528. https://doi.org/10.1161/CIR.000000000\n\
            0000659\n\
            Schmieder RE, Kjeldsen SE, et al. Reduced incidence\n\
            of new-onset atrial fibrillation: the VALUE trial. J\n\
            Hypertens 2008;26:403-11. https://doi.org/10.1097/HJH.0b013e\n\
            3282f35c67";
        let found = texts(list);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(
            found[0].ends_with("https://doi.org/10.1161/CIR.0000000000000659"),
            "{found:?}"
        );
        assert!(
            found[1].ends_with("https://doi.org/10.1097/HJH.0b013e3282f35c67"),
            "{found:?}"
        );
        let dois: Vec<String> = crate::id::scan(list)
            .iter()
            .map(|m| m.id.to_string())
            .collect();
        assert_eq!(
            dois,
            [
                "doi:10.1161/cir.0000000000000659",
                "doi:10.1097/hjh.0b013e3282f35c67"
            ]
        );
    }

    #[test]
    fn indents_never_start_a_reference_mid_word_or_in_lower_case() {
        // Hanging indents, with lines a PDF's layout put at the margin
        // although they continue the reference.
        let lines = [
            IndentedLine::new(
                "Chokesuwattanaskul R, et al. Incident atrial fibrillation in bariatric sur-",
                0.0,
            ),
            IndentedLine::new(
                "gery: a systematic review. Intern Med J 2020;50:810-7.",
                0.0,
            ),
            IndentedLine::new("Sardana M, et al. Obstructive sleep apnea (the SAVE", 0.0),
            IndentedLine::new("trial). Circ Arrhythm Electrophysiol 2017;10:e005463.", 0.0),
            IndentedLine::new("Smith J, et al. Another reference about", 0.0),
            IndentedLine::new("the same thing. Lancet 2019;393:10-12.", 12.0),
        ];
        let found: Vec<String> = split_lines(&lines).into_iter().map(|r| r.text).collect();
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(
            found[0].contains("bariatric surgery: a systematic"),
            "{found:?}"
        );
        assert!(found[1].contains("(the SAVE trial)."), "{found:?}");
    }

    #[test]
    fn an_author_line_is_not_a_reference_of_its_own() {
        // #44: the authors on a line of their own, ending "et al.", then a
        // title starting "National, regional, …".
        let reference = "Finucane MM, Stevens GA, Cowan MJ, Danaei G, Lin JK, Paciorek CJ, et al.\n\
            National, regional, and global trends in body-mass index since 1980: system-\n\
            atic analysis of health examination surveys and epidemiological studies with\n\
            960 country-years and 9 1 million participants. Lancet 2011;377:557\u{2013}67.\n\
            http://dx.doi.org/10.1016/S0140-6736(10)62037-5.";
        for text in [reference.to_owned(), reference.replacen('F', "", 1)] {
            let found = texts(&text);
            assert_eq!(found.len(), 1, "{found:?}");
            assert!(found[0].contains("et al. National, regional"), "{found:?}");
        }
        // A list of such references still splits.
        let two =
            format!("{reference}\nSmith J, Jones K, et al.\nA second title. BMJ 2019;365:l123.");
        assert_eq!(texts(&two).len(), 2, "{:?}", texts(&two));
    }

    #[test]
    fn markers_alone_on_their_line_number_the_next() {
        assert_eq!(
            texts("[1]\nSmith J. One. 2020.\n[2]\nJones K. Two. 2021.\n[3] "),
            ["Smith J. One. 2020.", "Jones K. Two. 2021."]
        );
    }

    #[test]
    fn pdf_text_is_cleaned_and_spans_point_into_it() {
        let list = "1. Smith J. Neuro\u{AD}\nlogical ﬁndings. doi:10.1038/s41586\u{2010}020-2649-2\n\
            2. Jones K. Two. 2021.";
        let refs = split_references(list);
        assert_eq!(
            refs[0].text,
            "Smith J. Neurological findings. doi:10.1038/s41586-020-2649-2"
        );
        assert!(list[refs[0].span.clone()].starts_with("Smith J. Neuro\u{AD}"));
        assert!(list[refs[0].span.clone()].ends_with("\u{2010}020-2649-2"));
    }

    #[test]
    fn dois_broken_at_a_hyphen_carry_the_reading_without_it() {
        let refs = split_references("1. Smith J. doi:10.1038/nat-\nure14539\n2. Jones K. 2021.");
        assert_eq!(refs[0].text, "Smith J. doi:10.1038/nat-ure14539");
        let alt = &refs[0].alternatives[0];
        assert_eq!(alt.claimed.to_string(), "doi:10.1038/nat-ure14539");
        assert_eq!(alt.alternative.to_string(), "doi:10.1038/nature14539");
        assert!(refs[1].alternatives.is_empty());
    }

    #[test]
    fn hanging_indents_mark_starts_that_text_cannot() {
        // Each line ends a sentence and the next starts with a capital,
        // so the text alone reads three references.
        let lines = [
            IndentedLine::new("References", 72.4),
            IndentedLine::new("World Health Organization. Global report.", 72.0),
            IndentedLine::new("Geneva: WHO; 2020.", 90.2),
            IndentedLine::new("Smith J. A study. Nature. 2021.", 71.8),
        ];
        let refs = split_lines(&lines);
        assert_eq!(
            refs.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(),
            [
                "World Health Organization. Global report. Geneva: WHO; 2020.",
                "Smith J. A study. Nature. 2021.",
            ]
        );
        assert_eq!(refs[0].lines, 1..3);
        assert_eq!(refs[1].lines, 3..4);
    }

    #[test]
    fn first_line_indents_and_flat_lists() {
        let lines = [
            IndentedLine::new("Smith J. A study of", 90.0),
            IndentedLine::new("Things. Nature. 2021.", 72.0),
            IndentedLine::new("Jones K. Another. 2022.", 90.0),
        ];
        assert_eq!(split_lines(&lines).len(), 2);
        // One indent throughout: the text rules decide.
        let flat = [
            IndentedLine::new("1. Smith J. One.", 72.0),
            IndentedLine::new("2020.", 72.0),
            IndentedLine::new("2. Jones K. Two. 2021.", 72.0),
        ];
        let refs = split_lines(&flat);
        assert_eq!(refs[0].text, "Smith J. One. 2020.");
        assert_eq!(refs[1].lines, 2..3);
    }

    #[test]
    fn spans_cover_the_reference_without_its_marker() {
        let list = "1. Smith J. One.\r\n   2020.\r\n2. Jones K. Two. 2021.";
        let refs = split_references(list);
        assert_eq!(&list[refs[0].span.clone()], "Smith J. One.\r\n   2020.");
        assert_eq!(&list[refs[1].span.clone()], "Jones K. Two. 2021.");
    }

    #[test]
    fn a_single_reference_is_one() {
        assert_eq!(
            texts("Smith J. A study.\nJournal 2020;1:2."),
            ["Smith J. A study. Journal 2020;1:2."]
        );
        assert!(texts("  \n\n").is_empty());
    }

    #[test]
    fn a_year_running_on_in_a_link_is_undated() {
        let mut joined = Joined::new("DOI:10/2019");
        assert!(joined.dated);
        joined.push("05");
        assert_eq!(joined.text, "DOI:10/201905");
        assert!(!joined.dated);
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(4000))]
        /// Joining line by line agrees with joining the lines afresh.
        #[test]
        fn joined_as_it_grows_matches_join(
            lines in proptest::collection::vec(proptest::collection::vec(
                proptest::sample::select(vec![
                    "a", "Smith", " ", "-", "/", ".", "é", "\u{3a3}", "\u{212a}", "\u{130}",
                    "2019", "0", "5", "n.", "d.", "no", "date", "available", "from", "[cited",
                    "DOI:", "HTTP", "10.", "10/",
                ]),
                1..5,
            ), 1..12),
        ) {
            let text = lines.iter().map(|l| l.concat()).collect::<Vec<_>>().join("\n");
            let list = List::new(&text);
            let ranges: Vec<Range<usize>> = list.lines.iter().flatten().cloned().collect();
            let Some((first, rest)) = ranges.split_first() else { return Ok(()) };
            let mut joined = Joined::new(list.line(first));
            for (i, line) in rest.iter().enumerate() {
                joined.push(list.line(line));
                let fresh = list.join(&ranges[..i + 2]);
                proptest::prop_assert_eq!(&joined.text, &fresh);
                proptest::prop_assert_eq!(joined.dated, dated(&fresh));
            }
        }
    }
}
