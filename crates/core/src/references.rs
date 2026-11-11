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

/// One reference found in a list.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Reference {
    /// On one line, without its number or bullet.
    pub text: String,
    /// Byte range in the list: from after the marker to the end of its
    /// last line.
    pub span: Range<usize>,
}

/// The references in `text`, in order.
pub fn split_references(text: &str) -> Vec<Reference> {
    let list = List::new(text);
    let groups = list.numbered().unwrap_or_else(|| {
        let paragraphs: Vec<Vec<Range<usize>>> = list
            .lines
            .split(Option::is_none)
            .filter(|p| !p.is_empty())
            .map(|p| p.iter().flatten().cloned().collect())
            .collect();
        let alone = paragraphs.len() == 1;
        paragraphs
            .into_iter()
            .flat_map(|p| list.unnumbered(p, alone))
            .collect()
    });
    groups
        .into_iter()
        .map(|g| Reference {
            text: list.join(&g),
            span: g[0].start..g[g.len() - 1].end,
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
                    let first = line.start + used..line.end;
                    groups.push(vec![first]);
                }
                (None, Some(group)) => group.push(line.clone()),
                (None, None) => {}
            }
        }
        (groups.len() >= 2).then_some(groups)
    }

    /// Groups lines without markers: each its own reference if `alone`
    /// (the whole list) and they read as one per line; otherwise split
    /// where an author list follows a finished sentence.
    fn unnumbered(&self, lines: Vec<Range<usize>>, alone: bool) -> Vec<Vec<Range<usize>>> {
        let one_per_line = alone
            && lines.iter().all(|l| dated(self.line(l)))
            && lines
                .windows(2)
                .all(|w| ends_reference(self.line(&w[0])) && !continues(self.line(&w[1])));
        if one_per_line {
            return lines.into_iter().map(|l| vec![l]).collect();
        }
        let mut groups: Vec<Vec<Range<usize>>> = Vec::new();
        for line in lines {
            let text = self.line(&line);
            match groups.last_mut() {
                Some(group)
                    if !(ends_sentence(self.line(&group[group.len() - 1]))
                        && starts_with_author(text)) =>
                {
                    group.push(line)
                }
                _ => groups.push(vec![line]),
            }
        }
        groups
    }

    /// A group's lines as one: joined with a space, or with none where a
    /// word, DOI or URL was broken across lines.
    fn join(&self, group: &[Range<usize>]) -> String {
        let mut out = String::new();
        for line in group {
            let next = self.line(line);
            let first = next.chars().next().unwrap_or(' ');
            if out.is_empty() {
                // Nothing to join to.
            } else if out.ends_with('-') && first.is_lowercase() && hyphenated_word(&out) {
                out.pop();
            } else if !(out.ends_with('/') || out.ends_with('-') && in_link(&out)) {
                out.push(' ');
            }
            out.push_str(next);
        }
        out
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
    year || line.contains("doi") || line.contains("10.") || line.contains("http")
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
    if rest.starts_with(',') {
        return true;
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

/// `text` ends inside a DOI or URL.
fn in_link(text: &str) -> bool {
    let word = text.rsplit(char::is_whitespace).next().unwrap_or_default();
    word.starts_with("http") || word.starts_with("doi:") || word.starts_with("10.")
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
}
