//! A verified book checked against its other records and its editions:
//! the year its citation says it first appeared, and the publisher it
//! names (holdout-6's `book-edition-year` and `book-wrong-publisher`).
//!
//! - **The first year.** A citation saying its book first appeared in a
//!   year ("(Original work published 1969)", "(1st ed.)") gives a later
//!   edition's year when a record of the same book among the candidates
//!   (same title and authors) holds that edition and one more than three
//!   years earlier. Free: no request.
//! - **The publisher.** A book cited with a publisher its record doesn't
//!   name is looked up among the book's editions at Open Library: the
//!   work among the candidates, or else found by its title and first
//!   author (one search), then one page of its editions (one request).
//!   Both are searches, so they are paced and cached as any search is.
//!   When none of the editions, nor the record, names the publisher or
//!   its group, an edition of the year cited is among them, and enough
//!   editions are known to tell
//!   ([`matching::check_publisher`]), the claim is plausible, the
//!   publisher a difference. When Open Library lacks the work, or fails,
//!   the verdict stands.

use aspero_core::matching::{self, PublisherCheck};
use aspero_core::{Agreement, Claim, ProviderId, SearchQuery, SourceRecord, Work, WorkType};

use crate::resolve::{Candidate, compare};
use crate::verdict::{CitedField, Difference, Verdict, Verification, demote};
use crate::{Engine, SearchReport};

/// The provider whose works list their editions (its `editions`
/// parameter).
const EDITIONS_SOURCE: &str = "openlibrary";
/// Works asked for when searching the book by title and author.
const WORKS_LIMIT: u32 = 5;
/// Editions read: one page.
const EDITIONS_LIMIT: u32 = 100;

impl Engine {
    /// Checks a verified book's first year and publisher (see the module
    /// docs), making it plausible when they disagree with its editions.
    pub(crate) async fn check_edition(&self, claim: &Claim, v: &mut Verification) {
        if v.verdict != Verdict::Verified {
            return;
        }
        let Some(best) = v.best_match else { return };
        if let Some(year) = first_year_differs(claim, &v.report.candidates, best) {
            demote(claim, v, vec![year]);
            return;
        }
        if let Some(publisher) = self.publisher_differs(claim, v, best).await {
            demote(claim, v, vec![publisher]);
        }
    }

    async fn publisher_differs(
        &self,
        claim: &Claim,
        v: &mut Verification,
        best: usize,
    ) -> Option<Difference> {
        let record = v.report.candidates[best].record.clone();
        if record.work.work_type != Some(WorkType::Book) {
            return None;
        }
        let cited = matching::cited_publisher(claim, &record)?;
        let text = claim.text.as_deref();
        let years = claim.cited_years_beside(record.work.title.as_deref());
        if matching::check_publisher(&cited, text, &years, &record.work, &[])
            == PublisherCheck::Named
        {
            return None;
        }
        let source = self
            .providers()
            .map(|p| p.id())
            .find(|p| p.as_str() == EDITIONS_SOURCE)?;
        let key = if let Some(key) = work_among(&v.report.candidates, &source) {
            key
        } else {
            let found = self.search_book(&source, &record, v).await?;
            work_among(&compare(claim, found), &source)?
        };
        let mut q = SearchQuery::new("");
        q.extra.insert("editions".into(), key);
        q.page.limit = EDITIONS_LIMIT;
        let editions: Vec<Work> = self
            .searched(&source, &q, v)
            .await?
            .into_iter()
            .map(|r| r.work)
            .collect();
        // Open Library's own record names one of its editions' publishers:
        // no second catalogue agreeing.
        let mut own = record.work.clone();
        if record.source == source {
            own.publisher = None;
        }
        match matching::check_publisher(&cited, text, &years, &own, &editions) {
            PublisherCheck::Unnamed(actual) => Some(Difference {
                field: CitedField::Publisher,
                actual: Some(actual),
                cited: Some(cited),
                kind: None,
            }),
            _ => None,
        }
    }

    /// The book's works at `source`, searched by its record's title and
    /// first author's family name.
    async fn search_book(
        &self,
        source: &ProviderId,
        record: &SourceRecord,
        v: &mut Verification,
    ) -> Option<Vec<SourceRecord>> {
        let mut q = SearchQuery::new("");
        q.title = Some(record.work.title.clone()?);
        q.author = record.work.authors.first().and_then(family);
        q.page.limit = WORKS_LIMIT;
        self.searched(source, &q, v).await
    }

    /// A search's records, its usage counted in the verification's;
    /// `None` when it fails (the verdict then stands).
    async fn searched(
        &self,
        source: &ProviderId,
        q: &SearchQuery,
        v: &mut Verification,
    ) -> Option<Vec<SourceRecord>> {
        let SearchReport { page, usage, .. } = self.search(Some(source.as_str()), q).await.ok()?;
        if let Some(usage) = usage {
            v.report.usage.push((source.clone(), usage));
        }
        Some(page.hits.into_iter().map(|h| h.record).collect())
    }
}

/// A person's family name, else the last word of their name as written.
fn family(p: &aspero_core::Person) -> Option<String> {
    p.family
        .clone()
        .or_else(|| {
            let name = p.literal.as_deref()?;
            // "Firth, J. R." is family first.
            match name.split_once(',') {
                Some((family, _)) => Some(family.trim().to_owned()),
                None => name.split_whitespace().last().map(str::to_owned),
            }
        })
        .filter(|f| !f.is_empty())
}

/// Whether a candidate is a record of the book cited: a book whose title
/// and authors match.
fn same_book(c: &Candidate) -> bool {
    c.record.work.work_type == Some(WorkType::Book)
        && c.assessment.title.agreement == Agreement::Match
        && c.assessment.authors.agreement == Agreement::Match
}

/// The key of the work a candidate is at `source` (`OL45804W`), when it
/// is a record there of the book cited.
fn work_of(c: &Candidate, source: &ProviderId) -> Option<String> {
    if c.record.source != *source || !same_book(c) {
        return None;
    }
    let url = c.record.work.url.as_deref()?;
    let key = url.split("/works/").nth(1)?.trim_end_matches('/');
    (key.starts_with("OL") && key.ends_with('W')).then(|| key.to_owned())
}

/// The work at `source` of the book cited, among `candidates`: of its
/// records there (a catalogue may hold one book as several works), one
/// whose years include the year cited, else the first.
fn work_among(candidates: &[Candidate], source: &ProviderId) -> Option<String> {
    let dated = |c: &&Candidate| c.assessment.year.agreement == Agreement::Match;
    let works = || candidates.iter().filter(|c| work_of(c, source).is_some());
    works()
        .find(dated)
        .or_else(|| works().next())
        .and_then(|c| work_of(c, source))
}

/// The year the citation says its book first appeared, against a record
/// of the book among the candidates (same title and authors) that holds
/// the edition cited and one more than a few years earlier
/// ([`matching::first_year_contradicted`]): a later edition's year given.
fn first_year_differs(claim: &Claim, candidates: &[Candidate], best: usize) -> Option<Difference> {
    if candidates[best].assessment.title.agreement != Agreement::Match {
        return None;
    }
    let (first, earliest) = candidates
        .iter()
        .filter(|c| same_book(c))
        .find_map(|c| matching::first_year_contradicted(claim, &c.record))?;
    Some(Difference {
        field: CitedField::Year,
        actual: Some(earliest.to_string()),
        cited: Some(first.to_string()),
        kind: None,
    })
}
