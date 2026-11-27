//! Signs that OpenAlex merged two works into one (#183): two corrections
//! to one guideline, which share a title by design, became W4411450779,
//! with one's DOI and the other's date, volume, page and PDF. Its record
//! shows it: the works' locations name both DOIs and both PMIDs. A lookup
//! that sees them checks the record with the DOI's registry, which keeps
//! the works apart; others don't spend the request.
//!
//! Read from the record's raw response, which the answer cache keeps, so a
//! record served from the cache shows the same signs.

use std::collections::BTreeSet;

use aspero_core::csl::null_default;
use aspero_core::matching::preprint_server_of;
use aspero_core::{Doi, Pmid, SourceRecord, WorkId};
use serde::Deserialize;

use crate::ID;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WireMerged {
    doi: Option<String>,
    #[serde(deserialize_with = "null_default")]
    ids: WireIds,
    #[serde(deserialize_with = "null_default")]
    locations: Vec<WireLocation>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WireIds {
    doi: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WireLocation {
    landing_page_url: Option<String>,
}

/// Whether OpenAlex's `record`, found by `asked`, looks like two works
/// merged into one:
/// - its locations name two PubMed records (two articles PubMed keeps
///   apart; an erratum's notice merged into its article is one too);
/// - its locations name another DOI under its own DOI's prefix, not a
///   version or part of it (`versions`);
/// - or `asked`, a DOI, isn't the record's own: the record is another
///   work's that carries it.
///
/// A preprint's or a dataset's DOI beside the article's (`version_doi`)
/// is OpenAlex's usual merge of one work's versions, not a sign. False for
/// another provider's record.
pub fn looks_merged(record: &SourceRecord, asked: &WorkId) -> bool {
    signs(record, Some(asked))
}

/// Whether OpenAlex's `record` looks like two works merged into one by
/// its record alone: [`looks_merged`]'s signs but the DOI asked. Its date
/// may then be the other work's: a Cochrane review's `.pub5` (2023)
/// merged with its earlier versions has the 2011 version's date
/// (W1949163494), a book's reissue its first edition's.
pub fn merges_works(record: &SourceRecord) -> bool {
    signs(record, None)
}

fn signs(record: &SourceRecord, asked: Option<&WorkId>) -> bool {
    if record.source != ID {
        return false;
    }
    let Ok(wire) = serde_json::from_str::<WireMerged>(&record.raw.body) else {
        return false;
    };
    let urls = || {
        wire.locations
            .iter()
            .filter_map(|l| l.landing_page_url.as_deref())
    };
    let pmids: BTreeSet<Pmid> = urls()
        .filter(|u| u.contains("pubmed.ncbi.nlm.nih.gov/"))
        .filter_map(|u| u.parse().ok())
        .collect();
    if pmids.len() >= 2 {
        return true;
    }
    let Some(own) = wire
        .ids
        .doi
        .as_deref()
        .or(wire.doi.as_deref())
        .and_then(|d| d.parse::<Doi>().ok())
        .filter(|d| !version_doi(d))
    else {
        return false;
    };
    let another = |doi: &Doi| !version_doi(doi) && !versions(doi, &own);
    let asked_another = matches!(asked, Some(WorkId::Doi(doi)) if another(doi));
    asked_another
        || urls()
            .filter(|u| u.contains("doi.org/"))
            .filter_map(|u| u.parse::<Doi>().ok())
            .any(|doi| doi.prefix() == own.prefix() && another(&doi))
}

/// Whether `a` and `b` are one work's DOI: the same but for punctuation
/// (APA's `10.1037//0022-3514…` beside `10.1037/0022-3514…`, Oxford's
/// `clipsy.bpg016` beside `clipsy/bpg016`), one a part of the other (a
/// PLOS figure's `….t001`, a dataset's files), or two numbered versions
/// of one (`….v1`, `…/v2`). Not a Cochrane review's versions (`….pub2`,
/// [`Doi::other_version`]): each is its own work, with its own date,
/// PMID and copy.
fn versions(a: &Doi, b: &Doi) -> bool {
    if a.other_version(b) {
        return false;
    }
    let key = |d: &str| -> String { d.chars().filter(char::is_ascii_alphanumeric).collect() };
    let (a, b) = (a.as_str(), b.as_str());
    let part = |whole: &str, part: &str| {
        part.strip_prefix(whole)
            .is_some_and(|rest| rest.starts_with(['.', '/']))
    };
    key(a) == key(b) || part(a, b) || part(b, a) || unversioned(a) == unversioned(b)
}

/// `doi` without a trailing version number (`.v2`, `/v2`).
fn unversioned(doi: &str) -> &str {
    let Some(at) = doi.rfind(['.', '/']) else {
        return doi;
    };
    let tail = &doi[at + 1..];
    let numbered = tail
        .strip_prefix('v')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    if numbered { &doi[..at] } else { doi }
}

/// Repositories whose DOIs name a dataset or its files, beside the
/// article's: Zenodo, figshare (also under publishers' own prefixes),
/// Dryad.
const DATA_REPOSITORIES: &[&str] = &["10.5281/", "10.6084/", "10.5061/"];

/// Whether `doi` is another version's than an article's: a preprint's
/// (arXiv, bioRxiv, Research Square, SSRN, …, or a publisher's own, as
/// JMIR's `10.2196/preprints.54419`), a dataset's, or a
/// Copernicus discussion paper's (`10.5194/cp-2018-82`, before
/// `10.5194/cp-14-1529-2018`).
fn version_doi(doi: &Doi) -> bool {
    let s = doi.as_str();
    doi.is_arxiv()
        || preprint_server_of(&WorkId::Doi(doi.clone())).is_some()
        || doi.suffix().starts_with("preprints")
        || DATA_REPOSITORIES.iter().any(|p| s.starts_with(p))
        || ["figshare", "zenodo", "dryad"]
            .iter()
            .any(|r| s.contains(r))
        || (doi.prefix() == "10.5194" && discussion_paper(doi.suffix()))
}

/// A Copernicus discussion paper's suffix: journal, year, number
/// (`cp-2018-82`); the final paper's has volume, page and year.
fn discussion_paper(suffix: &str) -> bool {
    let parts: Vec<&str> = suffix.split('-').collect();
    matches!(parts.as_slice(), [journal, year, number]
        if journal.bytes().all(|b| b.is_ascii_lowercase())
            && year.len() == 4
            && [year, number].iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())))
}

#[cfg(test)]
mod tests {
    use aspero_core::{FieldSet, Raw, Work};
    use jiff::Timestamp;
    use serde_json::json;

    use super::*;

    /// OpenAlex's record of a work with `doi` and locations at `urls`.
    fn record(doi: &str, urls: &[&str]) -> SourceRecord {
        let locations: Vec<_> = urls
            .iter()
            .map(|u| json!({"landing_page_url": u}))
            .collect();
        let body = json!({"doi": format!("https://doi.org/{doi}"), "locations": locations});
        let ids = vec![doi.parse().unwrap()];
        let raw = Raw::json(&body);
        SourceRecord::new(
            ID,
            ids,
            Work::default(),
            FieldSet::ALL,
            raw,
            Timestamp::UNIX_EPOCH,
        )
    }

    fn merged(doi: &str, urls: &[&str]) -> bool {
        looks_merged(&record(doi, urls), &doi.parse().unwrap())
    }

    const LATER: &str = "10.1111/1471-0528.70298";
    const EARLIER: &str = "10.1111/1471-0528.18258";

    #[test]
    fn two_corrections_merged_into_one_work_show_it() {
        // W4411450779 (#183), each sign alone.
        let pubmed = |n: &str| format!("https://pubmed.ncbi.nlm.nih.gov/{n}");
        let (a, b) = (pubmed("40536112"), pubmed("42381193"));
        assert!(merged(LATER, &[&a, &b]));
        assert!(merged(LATER, &[&format!("https://doi.org/{EARLIER}")]));
        // Found by the other correction's DOI, which it doesn't list.
        assert!(looks_merged(&record(LATER, &[]), &EARLIER.parse().unwrap()));
    }

    #[test]
    fn one_works_versions_and_spellings_dont() {
        let doi = |d: &str| format!("https://doi.org/{d}");
        for (own, other) in [
            // Preprints beside the article.
            ("10.3389/fpls.2023.1185616", "10.1101/2022.12.11.519980"),
            ("10.1038/s41586-020-2649-2", "10.48550/arxiv.2006.10256"),
            ("10.2196/54419", "10.2196/preprints.54419"),
            ("10.5194/cp-14-1529-2018", "10.5194/cp-2018-82"),
            // Datasets, versions and parts.
            ("10.2337/dc20-1729", "10.2337/figshare.13260212.v1"),
            ("10.3886/icpsr04652.v6", "10.3886/icpsr04652.v1"),
            ("10.17632/cfs7mwh3rx", "10.17632/cfs7mwh3rx.1"),
            ("10.7910/dvn/tjclkp", "10.7910/dvn/tjclkp/0a2xtw"),
            // One DOI, spelled two ways.
            (
                "10.1037/0022-3514.77.6.1213",
                "10.1037//0022-3514.77.6.1213",
            ),
            ("10.1093/clipsy.bpg016", "10.1093/clipsy/bpg016"),
            // Another registrant's DOI (a JSTOR copy) isn't counted here.
            ("10.1111/j.1558-5646.1981.tb04919.x", "10.2307/2408204"),
        ] {
            assert!(!merged(own, &[&doi(own), &doi(other)]), "{own} {other}");
        }
        // Found by its preprint's DOI.
        let journal = record("10.3389/fpls.2023.1185616", &[]);
        assert!(!looks_merged(
            &journal,
            &"10.1101/2022.12.11.519980".parse().unwrap()
        ));
        // One PubMed record, at two addresses.
        let pm = "https://pubmed.ncbi.nlm.nih.gov/33355258";
        assert!(!merged("10.2337/dc20-1729", &[pm, pm]));
    }

    /// W1949163494, minimised: a Cochrane review's `.pub5` merged with its
    /// earlier versions, the first of them without a `.pub` suffix.
    #[test]
    fn a_cochrane_reviews_versions_merged_show_it() {
        let doi = |d: &str| format!("https://doi.org/{d}");
        for (own, other) in [
            (
                "10.1002/14651858.cd001955.pub5",
                "10.1002/14651858.cd001955.pub4",
            ),
            (
                "10.1002/14651858.cd007491.pub2",
                "10.1002/14651858.cd007491",
            ),
        ] {
            assert!(merged(own, &[&doi(own), &doi(other)]), "{own} {other}");
            assert!(merges_works(&record(own, &[&doi(other)])), "{own} {other}");
        }
        let alone = "10.1002/14651858.cd001955.pub5";
        assert!(!merges_works(&record(alone, &[&doi(alone)])));
        // Found by its earlier version's DOI.
        assert!(looks_merged(
            &record(alone, &[]),
            &"10.1002/14651858.cd001955".parse().unwrap()
        ));
        assert!(!merges_works(&record(alone, &[])));
    }

    #[test]
    fn another_providers_record_or_an_unreadable_one_shows_nothing() {
        let mut r = record(LATER, &[]);
        r.source = aspero_core::ProviderId::from_static("crossref");
        assert!(!looks_merged(&r, &EARLIER.parse().unwrap()));
        let mut r = record(LATER, &[]);
        r.raw.body = "not json".into();
        assert!(!looks_merged(&r, &EARLIER.parse().unwrap()));
    }
}
