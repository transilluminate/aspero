//! A label beside an identifier: the short name a model's answer gives a
//! work it lists by identifier ("Gated SAEs: 2404.16014", "Lookahead
//! Decoding | 2402.02057", "DESI DR2 BAO (arXiv:2503.14738, 2025)"). Not
//! a title cited: a method's name, a nickname, a topic, every word of it
//! the record's title's. Another work's title or name beside the
//! identifier has words of its own ("DESI 2024 III … DR1" for "DESI 2024
//! VI: …").

use super::text::{STOPWORDS, words};
use crate::id::WorkId;
use crate::work::PLAUSIBLE_YEARS;

/// Words around an identifier that name no work: its server, its kind.
const FURNITURE: &[&str] = &["arxiv", "preprint", "doi", "pmid", "pmcid"];

/// Whether `text`, its identifiers taken out (those written out, and the
/// claimed `ids` given bare: "2404.16014"), is a label for the work
/// titled `title`: some words besides a year and furniture, and each of
/// them a word of the title, its plural or a form of its stem
/// ("sparsity" for "Sparse"). Compare `text` with its acronyms expanded
/// for the title ([`super::titles::expand_acronyms`]).
pub(super) fn labels(text: &str, title: &str, ids: &[WorkId]) -> bool {
    let mut rest = String::with_capacity(text.len());
    let mut at = 0;
    for mention in crate::id::scan(text) {
        if mention.span.start < at {
            continue;
        }
        rest.push_str(&text[at..mention.span.start]);
        rest.push(' ');
        at = mention.span.end;
    }
    rest.push_str(&text[at..]);
    let rest: Vec<&str> = rest
        .split(|c: char| c.is_whitespace() || c == '|')
        .filter(|token| {
            let token = token.trim_matches(|c: char| "()[]{}.,;:\"'“”‘’*_".contains(c));
            !ids.iter()
                .any(|id| WorkId::parse_as(id.kind(), token).is_ok_and(|t| t == *id))
        })
        .collect();
    let rest = rest.join(" ");
    let defined = defined_acronyms(&rest);
    let own = words(title);
    let label: Vec<String> = words(&rest)
        .into_iter()
        .filter(|w| !STOPWORDS.contains(&w.as_str()) && !FURNITURE.contains(&w.as_str()))
        .filter(|w| !defined.contains(w))
        .filter(|w| !(w.len() == 4 && w.parse().is_ok_and(|y| PLAUSIBLE_YEARS.contains(&y))))
        .collect();
    !label.is_empty() && label.iter().all(|w| own.iter().any(|o| same_word(w, o)))
}

/// The acronyms `text` gives with their words after them ("LATS:
/// Language Agent Tree Search"), lower case: no more words of a label.
fn defined_acronyms(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(char::is_alphanumeric) {
        rest = &rest[start..];
        let end = rest
            .find(|c: char| !c.is_alphanumeric())
            .unwrap_or(rest.len());
        let (token, after) = rest.split_at(end);
        rest = after;
        if token.chars().filter(char::is_ascii_uppercase).count() < 2 {
            continue;
        }
        let stem = token.strip_suffix('s').unwrap_or(token).to_lowercase();
        let after = words(after);
        let initials: String = after
            .iter()
            .filter(|w| !STOPWORDS.contains(&w.as_str()))
            .take(stem.chars().count())
            .filter_map(|w| w.chars().next())
            .collect();
        if initials == stem {
            found.push(token.to_lowercase());
        }
    }
    found
}

/// One word, or its plural, or two forms of one stem of five letters or
/// more that differ only in an ending, the shorter's last letter at most
/// ("sparse", "sparsity").
fn same_word(a: &str, b: &str) -> bool {
    if a == b || a.strip_suffix('s') == Some(b) || b.strip_suffix('s') == Some(a) {
        return true;
    }
    let shared = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
    let shorter = a.chars().count().min(b.chars().count());
    shared >= 5 && shared + 1 >= shorter
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arxiv(id: &str) -> Vec<WorkId> {
        vec![WorkId::parse_as(crate::id::IdKind::Arxiv, id).unwrap()]
    }

    #[test]
    fn a_label_is_the_titles_words() {
        let ids = arxiv("2403.10444");
        let title = "Block Verification Accelerates Speculative Decoding";
        assert!(labels("Block Verification | 2403.10444", title, &ids));
        assert!(labels(
            "Adaptive sparsity: 2411.02124",
            "Adaptive Sparse Allocation",
            &arxiv("2411.02124")
        ));
        // Another work's name, an id alone, a title of its own.
        assert!(!labels(
            "Blockwise Parallel Decoding | 2403.10444",
            title,
            &ids
        ));
        assert!(!labels("2403.10444", title, &ids));
        // An acronym its words follow defines them.
        assert!(labels(
            "LATS: Language Agent Tree Search (2023): 2310.04406",
            "Language Agent Tree Search Unifies Reasoning Acting and Planning",
            &arxiv("2310.04406")
        ));
        assert!(!labels(
            "DESI 2024 III (arXiv:2404.03002, DR1 baryon acoustic oscillations)",
            "DESI 2024 VI: cosmological constraints from the measurements of baryon acoustic \
             oscillations",
            &arxiv("2404.03002")
        ));
    }

    #[test]
    fn stems_differ_only_in_an_ending() {
        assert!(same_word("sparsity", "sparse"));
        assert!(same_word("features", "feature"));
        assert!(!same_word("transfer", "transformer"));
        assert!(!same_word("decoding", "decomposing"));
        assert!(!same_word("iii", "vi"));
    }
}
