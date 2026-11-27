//! Whether a work's free copy can be fetched by a program, or only read in
//! a browser (#204).
//!
//! A free copy is often only an article page on PubMed Central or Europe
//! PMC: their PDF links answer a script with a challenge, a 403 or a 404
//! (Europe PMC's `?pdf=render` and `ptpmcrender.fcgi`, PMC's
//! `/articles/PMC…/pdf/`; tried 2026-11-22). [`FreeCopyKind`] says which
//! case a client is in, as data.

use serde::{Deserialize, Serialize};

use crate::id::{Doi, WorkId};
use crate::work::OpenAccess;

/// What kind of free copy is known: a PDF a program can fetch, a copy to
/// read only in a browser, or none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FreeCopyKind {
    /// `pdf_url` is a PDF not known to turn programs away.
    Pdf,
    /// Free to read, but only as a page (`url`) or through a PDF link that
    /// turns programs away: PMC's and Europe PMC's article pages, their
    /// author manuscripts and articles outside PMC's open-access subset.
    BrowserOnly,
    /// No free copy is known (`is_oa` false).
    #[serde(rename = "none")]
    NoneKnown,
}

impl FreeCopyKind {
    pub const ALL: &[FreeCopyKind] = &[
        FreeCopyKind::Pdf,
        FreeCopyKind::BrowserOnly,
        FreeCopyKind::NoneKnown,
    ];

    /// As serialized: `pdf`, `browser_only` or `none`.
    pub fn as_str(self) -> &'static str {
        match self {
            FreeCopyKind::Pdf => "pdf",
            FreeCopyKind::BrowserOnly => "browser_only",
            FreeCopyKind::NoneKnown => "none",
        }
    }
}

impl OpenAccess {
    /// What kind of free copy this is: [`Pdf`](FreeCopyKind::Pdf) for a
    /// `pdf_url` that [`pdf_is_fetchable`], else
    /// [`BrowserOnly`](FreeCopyKind::BrowserOnly) when the work is free,
    /// else [`NoneKnown`](FreeCopyKind::NoneKnown).
    pub fn free_copy(&self) -> FreeCopyKind {
        if self.pdf_url.as_deref().is_some_and(pdf_is_fetchable) {
            FreeCopyKind::Pdf
        } else if self.is_oa {
            FreeCopyKind::BrowserOnly
        } else {
            FreeCopyKind::NoneKnown
        }
    }
}

impl OpenAccess {
    /// This open access without the copies that are another version's
    /// than the work with `ids` ([`another_versions_copy`]): those links
    /// dropped and, when no link is left, the work not known to be free
    /// (`is_oa` false, its status, licence and version, which were that
    /// copy's, unknown). Unchanged when no link is another version's.
    #[must_use]
    pub fn without_other_versions(mut self, ids: &[WorkId]) -> Self {
        let other = |url: &Option<String>| {
            url.as_deref()
                .is_some_and(|u| another_versions_copy(u, ids))
        };
        let (page, pdf) = (other(&self.url), other(&self.pdf_url));
        if !page && !pdf {
            return self;
        }
        if page {
            self.url = None;
        }
        if pdf {
            self.pdf_url = None;
        }
        if self.url.is_none() && self.pdf_url.is_none() {
            self.is_oa = false;
            self.status = None;
            self.license = None;
            self.version = None;
        }
        self
    }
}

/// Whether `url` is the copy of another version of the work with `ids`:
/// it names a DOI (at doi.org, or in a publisher's path,
/// `cochranelibrary.com/cdsr/doi/10.1002/14651858.CD001955.pub4/pdf/full`)
/// that is another numbered version of one of the work's DOIs
/// ([`Doi::other_version`]). OpenAlex merges a Cochrane review's versions,
/// and offers one version's copy for another's. Other DOIs aren't held
/// against a link: a repository's copy has the repository's own DOI, and a
/// work can have two (checked on 48,416 OpenAlex records, 2026-11-27).
pub fn another_versions_copy(url: &str, ids: &[WorkId]) -> bool {
    let Ok(WorkId::Doi(named)) = url.trim().parse::<WorkId>() else {
        return false;
    };
    let own = ids.iter().filter_map(|id| match id {
        WorkId::Doi(d) => Some(d),
        _ => None,
    });
    let mut versions = false;
    for doi in own {
        if names(&named, doi) {
            return false;
        }
        versions |= doi.other_version(&named);
    }
    versions
}

/// Whether a link's DOI is `doi`, maybe with its page's path after it.
fn names(link: &Doi, doi: &Doi) -> bool {
    link == doi
        || link
            .as_str()
            .strip_prefix(doi.as_str())
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Whether a PDF link can be fetched by a program, as far as is known: not
/// PubMed Central's site (`pmc.ncbi.nlm.nih.gov`, `ncbi.nlm.nih.gov/pmc/`),
/// whose PDF links serve a browser challenge, nor Europe PMC's rendering
/// of a PMC article (`?pdf=render`, `ptpmcrender.fcgi`), which answers
/// scripts with 403. Every other address is taken as fetchable: publishers
/// may still turn a script away, which can't be known without fetching.
pub fn pdf_is_fetchable(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .unwrap_or(&lower);
    let (host, path) = rest.split_at(rest.find(['/', '?', '#']).unwrap_or(rest.len()));
    let host = host.rsplit('@').next().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);
    let host = host.strip_prefix("www.").unwrap_or(host);
    let pmc_site =
        host == "pmc.ncbi.nlm.nih.gov" || (host == "ncbi.nlm.nih.gov" && path.starts_with("/pmc/"));
    let europe_pmc = matches!(host, "europepmc.org" | "ebi.ac.uk");
    let rendered = path.contains("pdf=render") || path.contains("ptpmcrender.fcgi");
    !(pmc_site || (europe_pmc && rendered))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pmc_and_europe_pmc_renders_are_browser_only() {
        for url in [
            "https://europepmc.org/articles/PMC8371605?pdf=render",
            "https://europepmc.org/backend/ptpmcrender.fcgi?accid=PMC6439620&blobtype=pdf",
            "https://www.ebi.ac.uk/europepmc/webservices/rest/PMC1/ptpmcrender.fcgi",
            "https://pmc.ncbi.nlm.nih.gov/articles/PMC10772984/pdf/bbad467.pdf",
            "https://www.ncbi.nlm.nih.gov/pmc/articles/PMC13752167/pdf/?tool=EBI",
            "HTTP://PMC.NCBI.NLM.NIH.GOV/articles/PMC1/pdf/",
        ] {
            assert!(!pdf_is_fetchable(url), "{url}");
        }
        for url in [
            "https://pmc-oa-opendata.s3.amazonaws.com/PMC8114386.1/PMC8114386.1.pdf",
            "https://europepmc.org/api/fulltextRepo?pprId=PPR454062&type=FILE&fileName=x.pdf",
            "https://arxiv.org/pdf/1706.03762",
            "https://www.ncbi.nlm.nih.gov/books/NBK1/pdf/x.pdf",
            "https://example.org/render?pdf=render",
        ] {
            assert!(pdf_is_fetchable(url), "{url}");
        }
    }

    fn ids(ids: &[&str]) -> Vec<WorkId> {
        ids.iter().map(|s| s.parse().unwrap()).collect()
    }

    /// OpenAlex's record of a Cochrane review's `.pub5` (W1949163494)
    /// offers `.pub4`'s PDF; a repository's copy with its own DOI, or the
    /// work's own page, stand.
    #[test]
    fn another_versions_copy_is_dropped() {
        let pub5 = ids(&["doi:10.1002/14651858.cd001955.pub5", "pmcid:PMC9831289"]);
        for url in [
            "https://doi.org/10.1002/14651858.cd001955.pub4",
            "https://www.cochranelibrary.com/cdsr/doi/10.1002/14651858.CD001955.pub4/pdf/full",
            "https://doi.org/10.1002/14651858.CD001955",
        ] {
            assert!(another_versions_copy(url, &pub5), "{url}");
        }
        for url in [
            "https://doi.org/10.1002/14651858.cd001955.pub5",
            "https://www.cochranelibrary.com/cdsr/doi/10.1002/14651858.CD001955.pub5/pdf/full",
            "https://pmc.ncbi.nlm.nih.gov/articles/PMC9831289/",
            "https://doi.org/10.25405/ncl.34560306.v1",
            "https://doi.org/10.1002/14651858.cd001956.pub4",
        ] {
            assert!(!another_versions_copy(url, &pub5), "{url}");
        }
        let mut oa = OpenAccess {
            is_oa: true,
            status: Some("bronze".into()),
            url: Some("https://doi.org/10.1002/14651858.cd001955.pub4".into()),
            pdf_url: Some(
                "https://www.cochranelibrary.com/cdsr/doi/10.1002/14651858.CD001955.pub4/pdf/full"
                    .into(),
            ),
            ..OpenAccess::default()
        };
        let gone = oa.clone().without_other_versions(&pub5);
        assert!(!gone.is_oa);
        assert_eq!(gone.free_copy(), FreeCopyKind::NoneKnown);
        assert_eq!((gone.url, gone.pdf_url, gone.status), (None, None, None));
        oa.url = Some("https://pmc.ncbi.nlm.nih.gov/articles/PMC9831289/".into());
        let kept = oa.clone().without_other_versions(&pub5);
        assert!(kept.is_oa && kept.pdf_url.is_none());
        assert_eq!(kept.url, oa.url);
        let pub4 = ids(&["doi:10.1002/14651858.cd001955.pub4"]);
        assert_eq!(oa.clone().without_other_versions(&pub4), oa);
    }

    #[test]
    fn kinds_follow_the_pdf_link_then_is_oa() {
        let mut oa = OpenAccess::default();
        assert_eq!(oa.free_copy(), FreeCopyKind::NoneKnown);
        oa.is_oa = true;
        oa.url = Some("https://pmc.ncbi.nlm.nih.gov/articles/PMC2918907/".into());
        assert_eq!(oa.free_copy(), FreeCopyKind::BrowserOnly);
        oa.pdf_url = Some("https://europepmc.org/articles/PMC2918907?pdf=render".into());
        assert_eq!(oa.free_copy(), FreeCopyKind::BrowserOnly);
        oa.pdf_url = Some("https://journal.example/article.pdf".into());
        assert_eq!(oa.free_copy(), FreeCopyKind::Pdf);
        let names: Vec<String> = FreeCopyKind::ALL
            .iter()
            .map(|k| {
                serde_json::to_value(k)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        let strs: Vec<&str> = FreeCopyKind::ALL.iter().map(|k| k.as_str()).collect();
        assert_eq!(names, strs);
        assert_eq!(strs, ["pdf", "browser_only", "none"]);
    }
}
