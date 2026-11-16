//! Gathering candidates: identifier lookups, citation matchers, title and
//! keyword searches, publisher-prefix and volume-and-page searches.

use aspero_core::matching;
use aspero_core::{Claim, FieldSet, Filter, ProviderId, SearchQuery, YearRange, site};
use futures::future::{Either, join_all, select};
use futures::stream::{FuturesUnordered, StreamExt};

use super::merge::Accumulator;
use super::{ClaimExt, ResolveReport, Sighting, Via};
use crate::{Engine, EngineError, SearchReport};

/// Title searches per claim: the claim's own title or the matchers' top
/// distinct titles.
const MAX_TITLE_SEARCHES: usize = 2;

/// Results per search by volume and first page: those of every journal
/// that year, from which the journal picks.
const PLACE_LIMIT: u32 = 25;
/// The provider that searches a publisher's works by DOI prefix
/// (`filter=prefix:10.1001`), for a claim's site ([`Claim::site`]). Free.
const PREFIX_SEARCHER: &str = "crossref";

impl Engine {
    pub(super) async fn gather(
        &self,
        claim: &Claim,
        limit: u32,
        stop_when_verified: bool,
    ) -> ResolveReport {
        let mut acc = Accumulator {
            claim,
            report: ResolveReport::default(),
            counted: Vec::new(),
        };
        let done = |acc: &Accumulator| stop_when_verified && acc.verified();
        let query = |text: &str, citation: bool| {
            let mut q = if citation {
                SearchQuery::citation(text)
            } else {
                SearchQuery::new(text)
            };
            q.page.limit = limit;
            q
        };

        // Stage 1, concurrently (both free): lookups of the claimed
        // identifiers, and citation matchers on the whole reference.
        let reference = claim.reference();
        let matchers: Vec<ProviderId> = match &reference {
            Some(_) => self.searchers(true),
            None => Vec::new(),
        };
        let lookup = async {
            if claim.ids.is_empty() {
                None
            } else {
                Some(self.lookup(&claim.ids, FieldSet::DEFAULT).await)
            }
        };
        let matched = join_all(matchers.iter().map(|p| {
            let q = query(reference.as_deref().unwrap_or_default(), true);
            async move { self.search(Some(p.as_str()), &q).await }
        }));
        let (lookup, matched) = (std::pin::pin!(lookup), std::pin::pin!(matched));
        let (lookup, matched) = match select(lookup, matched).await {
            Either::Left((lookup, matched)) => (lookup, Either::Left(matched)),
            Either::Right((matched, lookup)) => (lookup.await, Either::Right(matched)),
        };

        if let Some(lookup) = lookup {
            for record in lookup.results.iter().filter_map(|r| r.record.as_ref()) {
                let sighting = Sighting {
                    provider: record.source.clone(),
                    via: Via::Lookup,
                    rank: 1,
                    score: None,
                };
                acc.add(record.clone(), sighting);
            }
            acc.usage(lookup.usage);
            acc.report.lookups = lookup.results;
        }
        // When the identifiers' lookup alone verifies the claim, the
        // matchers can't change the verdict: stop waiting for them (Crossref's
        // takes seconds).
        let matched = match matched {
            Either::Right(matched) => matched,
            Either::Left(_) if done(&acc) => Vec::new(),
            Either::Left(pending) => pending.await,
        };
        let mut titles: Vec<String> = claim.title.iter().cloned().collect();
        for (provider, result) in matchers.iter().zip(matched) {
            let hits = acc.add_search(provider, Via::CitationMatch, result);
            if let (None, Some(reference)) = (&claim.title, &reference) {
                // Query the title the reference cites, as found via each
                // hit: a commentary titled "Recommendation of X" yields X.
                titles.extend(
                    hits.iter()
                        .filter_map(|t| matching::shared_title(t, reference)),
                );
            }
        }
        // An identifier-only claim has nothing else to compare.
        if done(&acc) || !claim.is_descriptive() {
            return acc.finish();
        }

        // Stage 2, concurrently: title searches (or, with no title to go on,
        // cleaned-up keywords) on every keyword provider. Results are added
        // in provider-then-title order, so merging stays deterministic.
        titles = distinct(titles);
        // No hit pointed to the title (a conference paper without a DOI, a
        // book Crossref lacks): the title the reference's shape suggests,
        // else the cleaned-up keywords (a bare title ranks far better). So
        // too, first, when the hits point only to part of it ("graph
        // convolutional networks" of "Semi-Supervised Classification with
        // Graph Convolutional Networks") or past it, to other words the
        // reference shares with them (a publisher's: "addison wesley
        // longman").
        // A part only of a title the reference marks as one (BibTeX's, a
        // quoted one): a title guessed from its sentences may be the part.
        if claim.title.is_none()
            && let Some(text) = claim.text.as_deref()
            && let Some(title) = matching::probable_title(text)
        {
            let given = matching::given_title(text).is_some();
            if titles.iter().all(|t| match beside_title(t, &title) {
                Beside::Part => given,
                Beside::Apart => true,
                Beside::Same => false,
            }) {
                titles.insert(0, title);
            }
        }
        titles.truncate(MAX_TITLE_SEARCHES);
        let keywords = titles.is_empty();
        let mut jobs: Vec<(ProviderId, SearchQuery, Via)> = Vec::new();
        for provider in self.searchers(false) {
            if keywords && let Some(text) = &claim.text {
                let q = query(&matching::search_terms(text), false);
                jobs.push((provider.clone(), q, Via::KeywordSearch));
            }
            for title in &titles {
                jobs.push((provider.clone(), query(title, false), Via::TitleSearch));
            }
        }
        // A page's publisher's own works with its title (#98), so the work
        // is among the candidates however low the open searches rank it:
        // one free Crossref search per prefix.
        if let (Some(title), Some(searcher)) = (titles.first(), self.prefix_searcher())
            && !claim.site_prefixes().is_empty()
        {
            for prefix in claim.site_prefixes().iter().take(site::MAX_PREFIXES) {
                let mut q = query("", false);
                q.title = Some(title.clone());
                q.extra.insert("filter".into(), format!("prefix:{prefix}"));
                jobs.push((searcher.clone(), q, Via::TitleSearch));
            }
        }
        // A reference without a title ("Injury 47 (2016) 797–804") is
        // found by where it appeared, in the years it cites (and either
        // side: online first, then in print).
        if let Some(place) = claim.place() {
            let years = claim.cited_years();
            for provider in self.placers() {
                let mut q = query("", false);
                q.page.limit = PLACE_LIMIT;
                q.place = Some(place.clone());
                q.years = years
                    .iter()
                    .min()
                    .zip(years.iter().max())
                    .map(|(&a, &b)| YearRange {
                        from: Some(a.saturating_sub(1)),
                        to: Some(b.saturating_add(1)),
                    });
                jobs.push((provider, q, Via::PlaceSearch));
            }
        }
        // Each answer is added as it arrives, so a verified claim stops
        // waiting for slower providers. The report is rebuilt in job order
        // each time, so what's kept doesn't depend on who answered first.
        let mut pending: FuturesUnordered<_> = jobs
            .iter()
            .enumerate()
            .map(|(i, (p, q, _))| async move { (i, self.search(Some(p.as_str()), q).await) })
            .collect();
        let before = acc.report.clone();
        let mut answers: Vec<Option<Result<SearchReport, EngineError>>> = vec![None; jobs.len()];
        while let Some((i, result)) = pending.next().await {
            answers[i] = Some(result);
            acc.report = before.clone();
            for ((provider, _, via), answer) in jobs.iter().zip(&answers) {
                if let Some(result) = answer {
                    acc.add_search(provider, *via, result.clone());
                }
            }
            if done(&acc) {
                break;
            }
        }
        acc.finish()
    }

    /// [`PREFIX_SEARCHER`], when it's here and searches titles.
    fn prefix_searcher(&self) -> Option<ProviderId> {
        self.providers()
            .find(|p| {
                let caps = p.capabilities();
                p.id().as_str() == PREFIX_SEARCHER
                    && caps.search
                    && caps.search_filters.contains(&Filter::Title)
            })
            .map(|p| p.id())
    }

    /// Keyword engines that can search by volume and first page.
    fn placers(&self) -> Vec<ProviderId> {
        self.providers()
            .filter(|p| {
                let caps = p.capabilities();
                caps.search && !caps.citation_search && caps.search_filters.contains(&Filter::Place)
            })
            .map(|p| p.id())
            .collect()
    }

    /// Providers that search, either citation matchers or keyword engines.
    fn searchers(&self, matchers: bool) -> Vec<ProviderId> {
        self.providers()
            .filter(|p| p.capabilities().search && p.capabilities().citation_search == matchers)
            .map(|p| p.id())
            .collect()
    }
}

/// How a title a matcher's hit shares with the reference stands to the
/// title the reference's shape suggests.
#[derive(Debug, PartialEq, Eq)]
enum Beside {
    /// A part of it, shorter.
    Part,
    /// None of it: no word of four letters or more in common.
    Apart,
    /// It, or overlapping it otherwise: the title found.
    Same,
}

/// How `shared`, a title a matcher's hit shares with the reference,
/// stands to `probable`, the title the reference's shape suggests.
fn beside_title(shared: &str, probable: &str) -> Beside {
    let (shared, probable) = (matching::normalise(shared), matching::normalise(probable));
    if shared == probable {
        return Beside::Same;
    }
    if probable.len() > shared.len() && format!(" {probable} ").contains(&format!(" {shared} ")) {
        return Beside::Part;
    }
    let long = |s: &str| -> Vec<String> {
        s.split(' ')
            .filter(|w| w.chars().count() >= 4)
            .map(str::to_owned)
            .collect()
    };
    let p = long(&probable);
    if long(&shared).iter().any(|w| p.contains(w)) {
        Beside::Same
    } else {
        Beside::Apart
    }
}

/// Titles with duplicates (by normalised form) removed, order kept.
fn distinct(titles: Vec<String>) -> Vec<String> {
    let mut seen = Vec::new();
    titles
        .into_iter()
        .filter(|t| {
            let norm = matching::normalise(t);
            let new = !norm.is_empty() && !seen.contains(&norm);
            seen.push(norm);
            new
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_references_own_title_is_searched_beside_a_hits_part_of_it() {
        // wild-1 #337: Crossref's hits shared only "Graph Convolutional
        // Networks" of a BibTeX title; holdout-3's books, a publisher's
        // words.
        let probable = "Semi-Supervised Classification with Graph Convolutional Networks";
        assert_eq!(
            beside_title("graph convolutional networks", probable),
            Beside::Part
        );
        assert_eq!(
            beside_title("addison wesley longman", "The Mythical Man-Month"),
            Beside::Apart
        );
        // The title found, or one overlapping it otherwise: the hits'.
        assert_eq!(
            beside_title(
                "semi supervised classification with graph convolutional networks",
                probable
            ),
            Beside::Same
        );
        assert_eq!(
            beside_title(
                "Graph Convolutional Networks for Text Classification",
                probable
            ),
            Beside::Same
        );
    }
}
