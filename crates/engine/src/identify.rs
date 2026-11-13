//! Which work a document is, from its first page: not the works it cites
//! or links to.
//!
//! A first page prints the paper's own DOI, but often others too: the
//! journal's, data or code, a related article, the first references. So
//! no identifier is taken on trust. Each one on the page (those in the
//! header and footer first, at most [`MAX_IDENTIFIERS`]) is looked up, and
//! its work must be confirmed: by the title the caller gives (the largest
//! type at the top of the page, or the file's title metadata unless junk),
//! or by its own title and first author appearing on the page, as a
//! paper's do and the works it cites usually don't. With none confirmed,
//! the title and the top of the page are verified as a citation.

use aspero_core::id::{Mention, scan};
use aspero_core::matching::{self, Agreement};
use aspero_core::{Claim, FieldSet, SourceRecord, WorkId};

use crate::{Engine, Verdict, Verification};

/// Identifiers looked up per page.
pub const MAX_IDENTIFIERS: usize = 6;
/// The share of a page's lines at its top and at its bottom that count as
/// header and footer.
const EDGE: f64 = 0.2;
/// Characters from the top of the page verified as a citation when no
/// identifier is confirmed: title, authors and affiliations.
const TOP_CHARS: usize = 800;

/// What decided which work a document is, from surest to least sure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Decision {
    /// An identifier on the page, its work's title matching the title
    /// given.
    IdentifierAndTitle,
    /// An identifier on the page, its work's title and first author on the
    /// page too.
    IdentifierAndPage,
    /// The page's only identifier, with no title to check it against.
    OnlyIdentifier,
    /// No identifier confirmed: a search for the title and the top of the
    /// page.
    TitleSearch,
}

impl Decision {
    every_variant!(Decision: IdentifierAndTitle, IdentifierAndPage, OnlyIdentifier, TitleSearch);

    pub fn as_str(self) -> &'static str {
        match self {
            Decision::IdentifierAndTitle => "identifier_and_title",
            Decision::IdentifierAndPage => "identifier_and_page",
            Decision::OnlyIdentifier => "only_identifier",
            Decision::TitleSearch => "title_search",
        }
    }
}

/// The work a document is, and how that was decided.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Identified {
    pub decided_by: Decision,
    /// The claim verified: the identifier chosen, or the title and top of
    /// the page.
    pub claim: Claim,
    /// The work as a verified (or, by title search, plausible) citation.
    pub verification: Verification,
}

impl Identified {
    /// What was verified, on one line: the identifier, or the start of the
    /// page.
    pub fn input(&self) -> String {
        if !self.claim.ids.is_empty() {
            let ids: Vec<String> = self.claim.ids.iter().map(ToString::to_string).collect();
            return ids.join(" ");
        }
        let text = self.claim.reference().unwrap_or_default();
        let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
        match line.char_indices().nth(100) {
            Some((at, _)) => format!("{}…", &line[..at]),
            None => line,
        }
    }
}

impl Engine {
    /// The work a document is, from its first page's text and, optionally,
    /// its title: `title_hint` (the largest type at the top of the page)
    /// and `title` (the file's metadata, ignored when junk such as
    /// "Microsoft Word - draft3.docx"). `None` when nothing is sure enough.
    pub async fn identify(
        &self,
        first_page: &str,
        title: Option<&str>,
        title_hint: Option<&str>,
    ) -> Option<Identified> {
        // Metadata titles of a word or two are more often placeholders than
        // titles; the largest type on the page is the title, however short.
        let titles: Vec<&str> = [(title_hint, 1), (title, 3)]
            .into_iter()
            .filter_map(|(t, min_words)| t.map(str::trim).filter(|t| !junk_title(t, min_words)))
            .collect();
        let ids = candidates(first_page);
        let found = self.lookup(&ids, FieldSet::DEFAULT).await;
        let records: Vec<(&WorkId, &SourceRecord)> = ids
            .iter()
            .zip(&found.results)
            .filter_map(|(id, r)| Some((id, r.record.as_ref()?)))
            .collect();

        let by_title = records
            .iter()
            .filter_map(|&(id, r)| {
                let best = titles.iter().map(|t| title_score(t, r)).fold(0.0, f64::max);
                (best >= TITLE_MATCH).then_some((best, id))
            })
            // On a tie, the first: the page's header and footer come before
            // its body (a preprint's DOI and the published one both match).
            .rev()
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, id)| (id, Decision::IdentifierAndTitle));
        // A short letter's references may start on its first page, titles
        // and authors and all: the paper's own title is the one at the top.
        let on_page = || {
            let page = Claim::from_text(first_page);
            let norm = matching::normalise(first_page);
            records
                .iter()
                .filter(|(_, r)| {
                    let a = matching::assess(&page, r);
                    a.title.agreement == Agreement::Match && a.authors.agreement == Agreement::Match
                })
                .min_by_key(|(_, r)| {
                    let title = r.work.title.as_deref().map(matching::normalise);
                    title.and_then(|t| norm.find(&t)).unwrap_or(usize::MAX)
                })
                .map(|&(id, _)| (id, Decision::IdentifierAndPage))
        };
        let only = || match (&ids[..], titles.is_empty(), &records[..]) {
            ([_], true, [(id, _)]) => Some((*id, Decision::OnlyIdentifier)),
            _ => None,
        };
        if let Some((id, decided_by)) = by_title.or_else(on_page).or_else(only) {
            let mut claim = Claim::default();
            claim.ids = vec![id.clone()];
            let verification = self.verify(&claim, 5).await;
            return Some(Identified {
                decided_by,
                claim,
                verification,
            });
        }

        // No identifier confirmed: the top of the page as a citation, with
        // the identifiers already ruled out left aside. A title given is
        // there already when it's the largest type; one that isn't (a
        // banner, "PLOS ONE") mustn't be taken for the title, so it only
        // joins the text.
        let mut top: String = first_page.chars().take(TOP_CHARS).collect();
        let norm = matching::normalise(&top);
        for t in &titles {
            if !norm.contains(&matching::normalise(t)) {
                top = format!("{t}. {top}");
            }
        }
        let mut claim = Claim::default();
        claim.text = Some(top);
        let verification = self.verify(&claim, 5).await;
        matches!(verification.verdict, Verdict::Verified | Verdict::Plausible).then_some(
            Identified {
                decided_by: Decision::TitleSearch,
                claim,
                verification,
            },
        )
    }
}

/// Title similarity that counts as the same title.
const TITLE_MATCH: f64 = 0.9;

/// How well a record's title matches a title given for the document.
fn title_score(title: &str, record: &SourceRecord) -> f64 {
    record
        .work
        .title
        .as_deref()
        .map_or(0.0, |actual| matching::title_similarity(title, actual))
}

/// The page's identifiers to look up: distinct works, those in its header
/// and footer before those in its body, at most [`MAX_IDENTIFIERS`].
fn candidates(page: &str) -> Vec<WorkId> {
    let lines = page.lines().count().max(1);
    let edge = |m: &Mention| {
        let at = (m.line - 1) as f64 / lines as f64;
        at < EDGE || at >= 1.0 - EDGE
    };
    let mentions = scan(page);
    let (mut ordered, body): (Vec<&Mention>, Vec<&Mention>) =
        mentions.iter().partition(|m| edge(m));
    ordered.extend(body);
    let mut ids: Vec<WorkId> = Vec::new();
    for m in ordered {
        if !ids.iter().any(|id| matching::same_work_id(id, &m.id)) {
            ids.push(m.id.clone());
        }
    }
    ids.truncate(MAX_IDENTIFIERS);
    ids
}

/// A title that isn't one: empty, a placeholder, a word processor's file
/// name ("Microsoft Word - draft3.docx"), a file name, or under
/// `min_words` words.
fn junk_title(title: &str, min_words: usize) -> bool {
    let lower = title.trim().to_lowercase();
    let placeholder = [
        "untitled",
        "title",
        "document",
        "manuscript",
        "article",
        "pdf",
    ]
    .iter()
    .any(|p| lower == *p || lower.starts_with(&format!("{p} ")) && lower.len() < 16);
    let tool = [
        "microsoft word",
        "microsoft powerpoint",
        "untitled document",
        "doi:",
        "http",
    ]
    .iter()
    .any(|p| lower.starts_with(p));
    let file = [
        ".doc", ".docx", ".pdf", ".tex", ".dvi", ".indd", ".rtf", ".odt",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
        || (!lower.contains(' ') && lower.contains(['_', '-', '.']));
    // A margin stamp ("arXiv:1706.03762v7 [cs.CL] 2 Aug 2023") or a link.
    let identifier = !scan(title).is_empty();
    let words = lower
        .split_whitespace()
        .filter(|w| w.chars().any(char::is_alphabetic))
        .count();
    placeholder || tool || file || identifier || words < min_words.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_titles_that_are_not_titles() {
        for junk in [
            "",
            "untitled",
            "Microsoft Word - draft3.docx",
            "paper_final_v2",
            "manuscript.pdf",
            "Title",
            "arXiv:1706.03762v7 [cs.CL] 2 Aug 2023",
        ] {
            assert!(junk_title(junk, 1), "{junk:?}");
        }
        assert!(junk_title("Deep learning", 3));
        assert!(!junk_title("Deep learning", 1));
        assert!(!junk_title(
            "Chest wall thickness and decompression failure",
            3
        ));
    }

    #[test]
    fn header_and_footer_identifiers_come_first() {
        let mut page =
            String::from("Journal of Things 12 (2020) 1-9 https://doi.org/10.1234/jt.2020.1\n");
        for i in 0..20 {
            page.push_str(&format!("Body line {i} citing doi:10.5555/ref.{i}\n"));
        }
        page.push_str("© 2020 Elsevier. doi:10.1234/jt.2020.1\n");
        let ids = candidates(&page);
        assert_eq!(ids.len(), MAX_IDENTIFIERS);
        assert_eq!(ids[0].to_string(), "doi:10.1234/jt.2020.1");
        // The repeat in the footer is the same work, counted once.
        assert_eq!(
            ids.iter()
                .filter(|id| id.to_string() == "doi:10.1234/jt.2020.1")
                .count(),
            1
        );
    }
}
