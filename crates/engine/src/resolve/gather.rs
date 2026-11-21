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
            registry: self.registry().map(|e| e.provider.id()),
            registered: Vec::new(),
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
        // The matchers boxed, so an early stop drops them where it stops.
        let (lookup, matched) = (std::pin::pin!(lookup), Box::pin(matched));
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
            Either::Left(pending) if done(&acc) => {
                crate::observe::drop_for(crate::observe::Why::Done, pending);
                Vec::new()
            }
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
        if claim.title.is_none()
            && let Some(text) = claim.text.as_deref()
        {
            // Only a reference that a lookup or its place can't find
            // (a book's) is worth a search beside a fragment.
            let bookish = claim.ids.is_empty() && claim.place().is_none();
            titles = with_guess(titles, text, bookish);
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
                crate::observe::drop_for(crate::observe::Why::Done, pending);
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

/// `titles`, the runs a matcher's hits share with `text` (distinct, in hit
/// order), with the title `text`'s shape suggests, where it's wanted:
/// - first, when no hit pointed to it (a conference paper without a DOI,
///   a book Crossref lacks), or the hits point past it, to other words the
///   reference shares with them (a publisher's: "addison wesley
///   longman"), or only to part of a title the reference marks as one
///   (BibTeX's, a quoted one: "graph convolutional networks" of
///   "Semi-Supervised Classification with Graph Convolutional Networks").
///   A part of a title guessed from its sentences may be the title;
/// - beside a hit's only run when that is a fragment of it, under half its
///   words ("of the british army" of "A Contribution to the Sanitary
///   History of the British Army …", a one-word review's "Wilding" of
///   "Wilding: The return of nature to a British farm"): in the slot no
///   other run takes, so nothing searched before is put out; and only for
///   a reference with no identifier or place to look it up by
///   (`bookish`), as a book's: otherwise the search finds what those
///   already do, or nothing (an invented article's).
///
/// A hit's run that is the guess itself is searched however far down the
/// hits it came, before runs that only hold it and run on.
fn with_guess(mut titles: Vec<String>, text: &str, bookish: bool) -> Vec<String> {
    let Some(title) = matching::probable_title(text) else {
        return titles;
    };
    let given = matching::given_title(text).is_some();
    let guess = matching::normalise(&title);
    let equal = titles.iter().position(|t| matching::normalise(t) == guess);
    if let Some(at) = equal {
        // Cut, behind runs that only hold it and run on ("pidgin
        // and creole linguistics oxford", "1997 pidgin and creole
        // linguistics"): first, in their place.
        if at >= MAX_TITLE_SEARCHES
            && titles[..at]
                .iter()
                .all(|t| beside_title(t, &title, text) == Beside::Longer)
        {
            let found = titles.remove(at);
            titles.insert(0, found);
        }
    } else if titles.iter().all(|t| match beside_title(t, &title, text) {
        Beside::Part => given,
        Beside::Apart | Beside::Longer => true,
        Beside::Same => false,
    }) || (bookish
        && titles.len() < MAX_TITLE_SEARCHES
        && titles.iter().all(|t| fragment(t, &title, text)))
    {
        titles.insert(0, title);
    }
    titles
}

/// How a title a matcher's hit shares with the reference stands to the
/// title the reference's shape suggests.
#[derive(Debug, PartialEq, Eq)]
enum Beside {
    /// A part of it, shorter.
    Part,
    /// It and more of the reference: a book review's title, the book's
    /// whole citation ("Images, Iconoclasm, and the Carolingians.
    /// Philadelphia: University of Pennsylvania Press, 2009").
    Longer,
    /// None of it: elsewhere in the reference (a publisher's words,
    /// "Ethiopian Public Health Association"), or, where either can't be
    /// placed in it, no word of four letters or more in common.
    Apart,
    /// It, or overlapping it otherwise: the title found.
    Same,
}

/// How `shared`, a title a matcher's hit shares with `reference`, stands
/// to `probable`, the title the reference's shape suggests: by where each
/// stands in the reference, else by the words they share.
fn beside_title(shared: &str, probable: &str, reference: &str) -> Beside {
    let (shared, probable) = (matching::normalise(shared), matching::normalise(probable));
    if shared == probable {
        return Beside::Same;
    }
    let reference = matching::normalise(reference);
    let words = |s: &str| -> Vec<String> { s.split(' ').map(str::to_owned).collect() };
    let text = words(&reference);
    let at = |run: &[String]| {
        text.windows(run.len())
            .position(|w| w == run)
            .map(|i| i..i + run.len())
    };
    if let (Some(s), Some(p)) = (at(&words(&shared)), at(&words(&probable))) {
        return if p.start <= s.start && s.end <= p.end {
            Beside::Part
        } else if s.start <= p.start && p.end <= s.end {
            Beside::Longer
        } else if s.end <= p.start || p.end <= s.start {
            Beside::Apart
        } else {
            Beside::Same
        };
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

/// Joining words, which a fragment of a title may be all of ("of the").
const JOINING: &[&str] = &[
    "a", "an", "the", "of", "in", "on", "for", "and", "or", "to", "with", "by", "at", "from",
];

/// Whether `shared`, a title a matcher's hit shares with `reference`,
/// holds under half of `probable`'s own words (joining words aside) where
/// both stand in it: a fragment of the title the reference's shape
/// suggests, not that title found. Not when either can't be placed.
fn fragment(shared: &str, probable: &str, reference: &str) -> bool {
    let words = |s: &str| -> Vec<String> {
        matching::normalise(s)
            .split(' ')
            .filter(|w| !w.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let (text, shared, probable) = (words(reference), words(shared), words(probable));
    let at = |run: &[String]| {
        (!run.is_empty())
            .then(|| text.windows(run.len()).position(|w| w == run))
            .flatten()
            .map(|i| i..i + run.len())
    };
    let (Some(s), Some(p)) = (at(&shared), at(&probable)) else {
        return false;
    };
    let own = |range: std::ops::Range<usize>| {
        text[range]
            .iter()
            .filter(|w| !JOINING.contains(&w.as_str()))
            .count()
    };
    let overlap = s.start.max(p.start)..s.end.min(p.end).max(s.start.max(p.start));
    own(overlap) * 2 < own(p)
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
        let bibtex = "@inproceedings{kipf2017, title = {Semi-Supervised Classification with \
                      Graph Convolutional Networks}, booktitle = {ICLR}, year = {2017}}";
        assert_eq!(
            beside_title("graph convolutional networks", probable, bibtex),
            Beside::Part
        );
        let brooks = "Brooks FP. The Mythical Man-Month. Addison Wesley Longman; 1995.";
        assert_eq!(
            beside_title("addison wesley longman", "The Mythical Man-Month", brooks),
            Beside::Apart
        );
        // The title found, or one overlapping it otherwise: the hits'.
        assert_eq!(
            beside_title(
                "semi supervised classification with graph convolutional networks",
                probable,
                bibtex
            ),
            Beside::Same
        );
        assert_eq!(
            beside_title(
                "Graph Convolutional Networks for Text Classification",
                probable,
                bibtex
            ),
            Beside::Same
        );
    }

    #[test]
    fn the_guess_is_searched_where_a_hit_points_to_a_fragment_of_it() {
        let owned = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        // wild-4 #799: a one-word review's title, alone.
        let tree = "Tree, I. (2018). Wilding: The return of nature to a British farm. Picador.";
        assert_eq!(
            with_guess(owned(&["wilding"]), tree, true),
            owned(&["Wilding: The return of nature to a British farm", "wilding"])
        );
        // Not for an article cited by its place.
        assert_eq!(
            with_guess(owned(&["wilding"]), tree, false),
            owned(&["wilding"])
        );
        // Beside two runs, nothing put out.
        assert_eq!(
            with_guess(owned(&["wilding", "british farm picador"]), tree, true),
            owned(&["wilding", "british farm picador"])
        );
        // wild-4 #574: the guess itself, third, behind runs that hold it
        // and run on.
        let pidgin = "Mühlhäusler, Peter (1986/1997). Pidgin and Creole Linguistics. Oxford: \
                      Blackwell (2nd ed. London: Battlebridge, 1997).";
        let runs = owned(&[
            "pidgin and creole linguistics oxford",
            "1997 pidgin and creole linguistics",
            "pidgin and creole linguistics",
        ]);
        assert_eq!(
            with_guess(runs, pidgin, true)[0],
            "pidgin and creole linguistics"
        );
        // Not behind a run of the title's own: the guess may be wrong
        // (wild-3 #555's is its publisher).
        let poole = "Poole, Robert, ed. \"Peterloo\": The English Uprising. Oxford: Oxford \
                     University Press, 2019.";
        let runs = owned(&[
            "peterloo the english uprising oxford oxford university press 2019",
            "peterloo the english uprising",
            "oxford oxford university press 2019",
        ]);
        assert_eq!(with_guess(runs.clone(), poole, true), runs);
    }

    #[test]
    fn a_hits_fragment_of_the_guessed_title_is_searched_beside_it() {
        // wild-4 #799, #231, #230: a one-word review's title, a phrase of
        // the title, a run straddling the title's end and the publisher.
        let tree = "Tree, I. (2018). Wilding: The return of nature to a British farm. Picador.";
        let guess = "Wilding: The return of nature to a British farm";
        assert_eq!(beside_title("wilding", guess, tree), Beside::Part);
        assert!(fragment("wilding", guess, tree));
        let army = "Notes on Matters Affecting the Health, Efficiency, and Hospital \
                    Administration of the British Army. London: Harrison, 1858.";
        let guess = "Notes on Matters Affecting the Health, Efficiency, and Hospital \
                     Administration of the British Army";
        assert!(fragment("of the british army london harrison", guess, army));
        // Most of the guess is the title found: the guess may run on past
        // it.
        let kitaw = "Kitaw, Y. (2006). The evolution of public health in Ethiopia 1950-2000.";
        assert!(!fragment(
            "the evolution of public health in ethiopia",
            "The evolution of public health in Ethiopia 1950-2000",
            kitaw
        ));
        // Unplaced, no fragment.
        assert!(!fragment("graph networks", guess, army));
    }

    #[test]
    fn a_hits_words_elsewhere_in_the_reference_are_no_title() {
        // holdout-5 #296, wild-2 #472: hits sharing a publisher's words,
        // one of them the title's ("public", "british"); wild-2 #405: a
        // book review titled with the book's whole citation.
        let kitaw = "Kitaw, Y. (2006). The evolution of public health in Ethiopia. Ethiopian \
                     Public Health Association.";
        assert_eq!(
            beside_title(
                "ethiopian public health association",
                "The evolution of public health in Ethiopia",
                kitaw
            ),
            Beside::Apart
        );
        let blackburn = "Blackburn, M.A.S. (2011) Viking coinage and currency in the British \
                         Isles. British Numismatic Society Special Publication no. 7. London: \
                         British Numismatic Society.";
        assert_eq!(
            beside_title(
                "british numismatic society",
                "Viking coinage and currency in the British Isles",
                blackburn
            ),
            Beside::Apart
        );
        let noble = "Noble, Thomas F. X. Images, Iconoclasm, and the Carolingians. \
                     Philadelphia: University of Pennsylvania Press, 2009.";
        assert_eq!(
            beside_title(
                "images iconoclasm and the carolingians philadelphia university of \
                 pennsylvania press 2009",
                "Images, Iconoclasm, and the Carolingians",
                noble
            ),
            Beside::Longer
        );
    }
}
