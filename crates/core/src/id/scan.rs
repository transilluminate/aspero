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
//! cells are split apart. A DOI broken across lines is not found.

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
    /// 123`), wrappers and trailing punctuation not.
    pub span: Range<usize>,
}

/// Every identifier mentioned in `text`, in order, repeats included.
pub fn scan(text: &str) -> Vec<Mention> {
    let mut found = Vec::new();
    let mut line_start = 0;
    for (n, line) in text.split('\n').enumerate() {
        for (id, span) in scan_line(line) {
            found.push(Mention {
                id,
                line: n + 1,
                column: line[..span.start].chars().count() + 1,
                span: line_start + span.start..line_start + span.end,
            });
        }
        line_start += line.len() + 1;
    }
    found
}

/// Identifiers in one line, with byte ranges within it.
fn scan_line(line: &str) -> Vec<(WorkId, Range<usize>)> {
    let mut found = Vec::new();
    // A label ("PMID:", "ISBN") waiting for its value: kind and start.
    let mut label: Option<(IdKind, usize)> = None;
    for piece in pieces(line) {
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

/// Whitespace-separated pieces of a line, also split at Markdown table
/// bars and between a link's text and its target (`[text](url)`).
fn pieces(line: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut chars = line.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let link = c == ']' && chars.peek().is_some_and(|&(_, next)| next == '(');
        if c.is_whitespace() || c == '|' || link {
            // A link's text keeps its `]`, so its brackets stay balanced.
            let end = if link { i + 1 } else { i };
            out.extend(start.take().map(|s| s..end));
        } else {
            start.get_or_insert(i);
        }
    }
    out.extend(start.map(|s| s..line.len()));
    out
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
