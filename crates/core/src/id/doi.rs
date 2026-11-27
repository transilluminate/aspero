use std::borrow::Cow;
use std::fmt;

use super::syntax::{
    IdSyntax, clean_pasted, impl_id_traits, is_digits, strip_kind_prefix, strip_prefix_ci,
};
use super::url::Url;
use super::{ArxivId, IdKind};

const INFO_PREFIX: &str = "info:doi/";

/// arXiv's DataCite prefix.
const ARXIV_DOI_PREFIX: &str = "10.48550";

/// Sentence punctuation that clings to DOIs copied from prose ("see
/// 10.1145/3368089."). Not `)`: SICI-style suffixes can legitimately end in one.
const TRAILING_PUNCTUATION: &[char] = &['.', ',', ';', ':'];

/// A Digital Object Identifier, e.g. `10.1145/3368089`.
///
/// Canonical form is ASCII-lowercased (DOIs are case-insensitive over ASCII;
/// non-ASCII characters are left untouched) with no `doi:` or URL wrapper.
/// URL forms are percent-decoded; bare and prefixed forms are taken literally.
///
/// Trailing `.` `,` `;` `:` are stripped. The DOI spec technically permits a
/// suffix ending in one, but in practice that is far rarer than a DOI copied
/// with the end of its sentence, and a wrong DOI is caught later by lookup.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Doi(String);

impl Doi {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The registrant part, e.g. `10.1145`.
    pub fn prefix(&self) -> &str {
        self.split().0
    }

    /// Everything after the first `/`.
    pub fn suffix(&self) -> &str {
        self.split().1
    }

    fn split(&self) -> (&str, &str) {
        self.0.split_once('/').expect("canonical DOI contains '/'")
    }

    /// Whether this is under arXiv's DataCite prefix.
    pub fn is_arxiv(&self) -> bool {
        self.prefix() == ARXIV_DOI_PREFIX
    }

    /// The arXiv ID behind an arXiv DataCite DOI, version kept if it has one.
    pub fn arxiv(&self) -> Option<ArxivId> {
        if !self.is_arxiv() {
            return None;
        }
        self.suffix().strip_prefix("arxiv.")?.parse().ok()
    }
}

impl Doi {
    /// The DOI without a `.pubN` version suffix, and that version (1 for
    /// a DOI without one, the first version). Read up to the suffix's
    /// first `/`: a link's DOI may keep its page's path
    /// (`….pub4/pdf/full`).
    fn publication(&self) -> (&str, u32) {
        let end = self.prefix().len() + 1 + self.suffix().find('/').unwrap_or(self.suffix().len());
        let doi = &self.0[..end];
        match doi.rsplit_once(".pub") {
            Some((work, n)) if !n.is_empty() && is_digits(n) => (work, n.parse().unwrap_or(0)),
            _ => (doi, 1),
        }
    }

    /// Whether `other` is another numbered version of this DOI's work:
    /// the same DOI but for a different `.pubN` suffix, the DOI without one
    /// being the first version. Cochrane reviews are versioned so
    /// (`….CD001955.pub4`, superseded by `.pub5`), each version its own
    /// work. A suffix's path after a `/` is not compared.
    pub fn other_version(&self, other: &Doi) -> bool {
        let (a, b) = (self.publication(), other.publication());
        a.0 == b.0 && a.1 != b.1
    }
}

/// What the Cochrane Database of Systematic Reviews' DOIs begin with,
/// before the review's id (`10.1002/14651858.CD001955.pub5`).
const COCHRANE_REVIEWS: &str = "10.1002/14651858.";

impl Doi {
    /// The DOI of a Cochrane review given by its id alone, as the
    /// database's citations write it: `CD001955` (the first version) or
    /// `CD001955.pub5`. Two letters `CD`, six digits, and a version.
    pub(crate) fn cochrane_review(s: &str) -> Option<Doi> {
        let s = s.trim();
        let digits = s.get(..2)?.eq_ignore_ascii_case("cd").then(|| &s[2..])?;
        let (number, version) = match digits.split_once('.') {
            Some((number, version)) => (number, Some(version)),
            None => (digits, None),
        };
        let version_ok = version.is_none_or(|v| {
            v.get(..3).is_some_and(|p| p.eq_ignore_ascii_case("pub"))
                && v.len() > 3
                && is_digits(&v[3..])
        });
        (number.len() == 6 && is_digits(number) && version_ok)
            .then(|| Doi(format!("{COCHRANE_REVIEWS}{}", s.to_ascii_lowercase())))
    }
}

impl ArxivId {
    /// arXiv's DataCite DOI, `10.48550/arxiv.<id>`, which has no version.
    pub fn doi(&self) -> Doi {
        // Already canonical: arXiv IDs are lowercase.
        Doi(format!("{ARXIV_DOI_PREFIX}/arxiv.{}", self.id()))
    }
}

impl Doi {
    /// The DOI as written *with* the trailing punctuation that parsing
    /// strips, if `input` had any (`"10.1145/x."` → `10.1145/x.`). DOIs may
    /// legally end in `.`; callers retry with this when the stripped form
    /// isn't found.
    ///
    /// The one exception to the parse/display round trip: parsing this
    /// value's display strips the punctuation again. Use it for lookups only.
    pub fn with_trailing_punctuation(input: &str) -> Option<Doi> {
        let stripped: Doi = input.parse().ok()?;
        let lower = input.trim().to_ascii_lowercase();
        let at = lower.rfind(stripped.as_str())?;
        let tail = &lower[at + stripped.as_str().len()..];
        let only_punctuation =
            !tail.is_empty() && tail.chars().all(|c| TRAILING_PUNCTUATION.contains(&c));
        only_punctuation.then(|| Doi(format!("{}{tail}", stripped.as_str())))
    }
}

/// What publishers append to a DOI in their page URLs. Checked longest
/// first; only ever stripped from publisher URLs, never from a DOI as given.
const LANDING_SUFFIXES: &[&str] = &[
    "/full.pdf",
    ".full.pdf",
    "/fulltext",
    "/abstract",
    "/pdfdirect",
    ".abstract",
    "/epdf",
    "/full",
    "/html",
    "/meta",
    "/pdf",
    ".full",
    ".pdf",
];

/// `doi` without a trailing landing-page suffix (`/full`, `.pdf`, …).
fn without_landing_suffix(doi: &str) -> &str {
    let lower = doi.to_ascii_lowercase();
    LANDING_SUFFIXES
        .iter()
        .find(|s| lower.ends_with(*s))
        .map_or(doi, |s| &doi[..doi.len() - s.len()])
}

/// The DOI a `nature.com/articles/<suffix>` page is: Springer Nature's
/// prefix and the page's name (`nature14539`, `s41586-020-2649-2`,
/// `ncomms1234`, `nbt.3122`, `35057062`, `171737a0`), under `10.1038`
/// but for the Palgrave journals it hosts (`s41599-…`, `palcomms…`:
/// `10.1057`, `palcomms20162` naming `palcomms.2016.2`). Older pages
/// whose name is only part of the DOI aren't read: `/articles/6605004`
/// is `10.1038/sj.bjc.6605004`, `/articles/ncb0200_57` is
/// `10.1038/35000001` (checked against Crossref 2026-11-15).
fn nature_doi(url: &Url<'_>) -> Option<String> {
    let mut segs = url.segments();
    if segs.next()? != "articles" {
        return None;
    }
    let name = segs.next()?.to_ascii_lowercase();
    let name = name.strip_suffix(".pdf").unwrap_or(&name);
    let shaped = name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-'));
    // Nature's own numbers (1997 to 2001) have eight digits; seven are
    // another journal's `sj.…`.
    if !shaped || (is_digits(name) && name.len() != 8) {
        return None;
    }
    if let Some(rest) = name.strip_prefix("palcomms") {
        let rest = match rest.get(..4) {
            Some(year) if is_digits(rest) && rest.len() > 4 => format!(".{year}.{}", &rest[4..]),
            _ => rest.to_owned(),
        };
        return Some(format!("10.1057/palcomms{rest}"));
    }
    let prefix = if name.starts_with("s41599-") {
        "10.1057"
    } else {
        "10.1038"
    };
    Some(format!("{prefix}/{name}"))
}

pub(crate) fn has_info_prefix(s: &str) -> bool {
    strip_prefix_ci(s, INFO_PREFIX).is_some()
}

impl IdSyntax for Doi {
    const KIND: IdKind = IdKind::Doi;

    fn parse_bare(s: &str) -> Result<Self, &'static str> {
        // Also after URL decoding: canonical DOIs never end in what pasting
        // adds, so display → parse is the identity.
        let doi = clean_pasted(s)
            .trim_end_matches(TRAILING_PUNCTUATION)
            .to_ascii_lowercase();
        let (prefix, suffix) = doi
            .split_once('/')
            .ok_or("missing '/' between prefix and suffix")?;
        let registrant = prefix
            .strip_prefix("10.")
            .ok_or("must start with \"10.\"")?;
        let mut groups = registrant.split('.');
        let first = groups.next().unwrap_or_default();
        if !(4..=9).contains(&first.len()) || !is_digits(first) || !groups.all(is_digits) {
            return Err("registrant must be digits, e.g. 10.1234 or 10.1234.5");
        }
        if suffix.is_empty() {
            return Err("empty suffix");
        }
        if suffix.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err("suffix contains whitespace or control characters");
        }
        Ok(Doi(doi))
    }

    /// doi.org URLs, whose path is the DOI, and publishers' pages with the
    /// DOI in the path (`frontiersin.org/articles/10.3389/…/full`), whose
    /// landing-page suffix is dropped.
    fn from_url<'a>(url: &Url<'a>) -> Option<Result<Cow<'a, str>, &'static str>> {
        let decode = |path: &'a str| {
            percent_encoding::percent_decode_str(path)
                .decode_utf8()
                .map_err(|_| "not valid UTF-8 after percent-decoding")
        };
        if url.is_host(&["doi.org", "dx.doi.org"]) {
            return Some(decode(url.path));
        }
        if url.is_host(&["nature.com"])
            && let Some(doi) = nature_doi(url)
        {
            return Some(Ok(Cow::Owned(doi)));
        }
        // A path segment starting "10.<digits>/" begins the DOI.
        let at = url.path.match_indices("10.").map(|(i, _)| i).find(|&i| {
            let registrant = url.path[i + 3..].split('/').next().unwrap_or("");
            (i == 0 || url.path[..i].ends_with('/'))
                && registrant.starts_with(|c: char| c.is_ascii_digit())
                && registrant.bytes().all(|b| b.is_ascii_digit() || b == b'.')
                && url.path[i + 3 + registrant.len()..].len() > 1
        })?;
        Some(decode(&url.path[at..]).map(|doi| Cow::Owned(without_landing_suffix(&doi).to_owned())))
    }

    fn strip_prefix(s: &str) -> &str {
        strip_prefix_ci(s, INFO_PREFIX).unwrap_or_else(|| strip_kind_prefix(s, "doi"))
    }
}

impl_id_traits!(Doi);

impl fmt::Display for Doi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::IdError;

    fn doi(s: &str) -> Result<Doi, IdError> {
        s.parse()
    }

    #[test]
    fn normalises_surface_forms() {
        for input in [
            "10.1145/3368089.3409741",
            "  DOI:10.1145/3368089.3409741 ",
            "doi: 10.1145/3368089.3409741",
            "https://doi.org/10.1145/3368089.3409741",
            "http://dx.doi.org/10.1145/3368089.3409741",
            "doi.org/10.1145/3368089.3409741",
            "info:doi/10.1145/3368089.3409741",
        ] {
            assert_eq!(
                doi(input).unwrap().as_str(),
                "10.1145/3368089.3409741",
                "{input:?}"
            );
        }
    }

    #[test]
    fn lowercases_ascii_only() {
        assert_eq!(doi("10.1002/ANIE.É").unwrap().as_str(), "10.1002/anie.É");
    }

    #[test]
    fn percent_decodes_urls_only() {
        assert_eq!(
            doi("https://doi.org/10.1002/%28SICI%291097-4636")
                .unwrap()
                .as_str(),
            "10.1002/(sici)1097-4636"
        );
        assert_eq!(doi("10.1002/a%20b").unwrap().as_str(), "10.1002/a%20b");
    }

    #[test]
    fn decoded_url_dois_are_cleaned_too() {
        assert_eq!(
            doi("https://doi.org/10.1234/x%29").unwrap().as_str(),
            "10.1234/x"
        );
        assert_eq!(
            doi("https://doi.org/10.1016/S0140-6736%2810%2960835-5")
                .unwrap()
                .as_str(),
            "10.1016/s0140-6736(10)60835-5"
        );
    }

    #[test]
    fn query_and_fragment_are_not_part_of_url_doi() {
        assert_eq!(
            doi("https://doi.org/10.1145/3368089?foo=bar#x")
                .unwrap()
                .as_str(),
            "10.1145/3368089"
        );
    }

    #[test]
    fn strips_trailing_sentence_punctuation() {
        for input in [
            "10.1145/3368089.",
            "10.1145/3368089.,",
            "doi:10.1145/3368089;",
            "https://doi.org/10.1145/3368089.",
            "10.1145/3368089:",
        ] {
            assert_eq!(doi(input).unwrap().as_str(), "10.1145/3368089", "{input:?}");
        }
        assert_eq!(
            doi("10.1002/(sici)1097-4636(199601)").unwrap().suffix(),
            "(sici)1097-4636(199601)"
        );
        assert!(doi("10.1145/.").is_err());
    }

    #[test]
    fn a_cochrane_reviews_id_alone_is_its_doi() {
        let review = |s| Doi::cochrane_review(s).map(|d| d.to_string());
        assert_eq!(
            review("CD001955.pub5").as_deref(),
            Some("10.1002/14651858.cd001955.pub5")
        );
        assert_eq!(
            review(" cd001955 ").as_deref(),
            Some("10.1002/14651858.cd001955")
        );
        for not in [
            "CD00195",
            "CD0019551",
            "CD001955.pub",
            "CD001955.v2",
            "XD001955",
            "CD",
        ] {
            assert_eq!(review(not), None, "{not}");
        }
    }

    #[test]
    fn pub_suffixes_number_one_works_versions() {
        let v = |a: &str, b: &str| doi(a).unwrap().other_version(&doi(b).unwrap());
        assert!(v(
            "10.1002/14651858.cd001955.pub5",
            "10.1002/14651858.CD001955.pub4"
        ));
        assert!(v(
            "10.1002/14651858.cd007491",
            "10.1002/14651858.cd007491.pub2"
        ));
        // A link's DOI keeps its page's path.
        assert!(v(
            "10.1002/14651858.cd001955.pub5",
            "10.1002/14651858.cd001955.pub4/pdf"
        ));
        assert!(!v(
            "10.1002/14651858.cd001955.pub5",
            "10.1002/14651858.cd001955.pub5/pdf"
        ));
        assert!(!v(
            "10.1002/14651858.cd001955.pub5",
            "10.1002/14651858.cd001956.pub5"
        ));
        // Other numbering schemes name one work's versions, or the work.
        assert!(!v("10.3886/icpsr28141.v1", "10.3886/icpsr28141"));
        assert!(!v(
            "10.12688/f1000research.51912.2",
            "10.12688/f1000research.51912.1"
        ));
    }

    #[test]
    fn publisher_urls_give_their_doi_without_page_suffixes() {
        for (url, want) in [
            (
                "https://www.frontiersin.org/articles/10.3389/fneur.2019.01343/full",
                "10.3389/fneur.2019.01343",
            ),
            (
                "https://storage.imrpress.com/IMR/hmed/application/10.12968/hmed.2007.68.sup2.22842.pdf",
                "10.12968/hmed.2007.68.sup2.22842",
            ),
            (
                "https://onlinelibrary.wiley.com/doi/epdf/10.1002/14651858.CD000567.pub7",
                "10.1002/14651858.cd000567.pub7",
            ),
            (
                "https://www.nejm.org/doi/full/10.1056/NEJMoa1302097?articleTools=true",
                "10.1056/nejmoa1302097",
            ),
            (
                "https://www.thelancet.com/journals/lancet/article/PIIS0140-6736(10)60835-5/fulltext",
                "",
            ),
        ] {
            match doi(url) {
                Ok(d) => assert_eq!(d.as_str(), want, "{url}"),
                Err(_) => assert_eq!(want, "", "{url}"),
            }
        }
        // At doi.org the path is the DOI, suffix-like endings included.
        assert_eq!(
            doi("https://doi.org/10.1234/report.pdf").unwrap().as_str(),
            "10.1234/report.pdf"
        );
    }

    #[test]
    fn nature_pages_give_the_doi_their_name_ends() {
        // Each checked against Crossref (2026-11-15).
        for (url, want) in [
            (
                "https://www.nature.com/articles/nature14539",
                "10.1038/nature14539",
            ),
            (
                "https://www.nature.com/articles/s41586-020-2649-2.pdf",
                "10.1038/s41586-020-2649-2",
            ),
            (
                "https://www.nature.com/articles/ncomms1234?error=cookies_not_supported",
                "10.1038/ncomms1234",
            ),
            ("http://nature.com/articles/srep01234/", "10.1038/srep01234"),
            ("nature.com/articles/nbt.3122#Sec2", "10.1038/nbt.3122"),
            (
                "https://www.nature.com/articles/35057062",
                "10.1038/35057062",
            ),
            (
                "https://www.nature.com/articles/171737a0",
                "10.1038/171737a0",
            ),
            (
                "https://www.nature.com/articles/d41586-020-00502-w",
                "10.1038/d41586-020-00502-w",
            ),
            (
                "https://www.nature.com/articles/nrg2484/figures/1",
                "10.1038/nrg2484",
            ),
            (
                "https://www.nature.com/articles/s41599-020-0499-z",
                "10.1057/s41599-020-0499-z",
            ),
            (
                "https://www.nature.com/articles/palcomms20162",
                "10.1057/palcomms.2016.2",
            ),
            // An old link by DOI.
            (
                "http://www.nature.com/doifinder/10.1038/ncomms1234",
                "10.1038/ncomms1234",
            ),
        ] {
            assert_eq!(doi(url).unwrap().as_str(), want, "{url}");
        }
        for url in [
            "https://www.nature.com/articles?type=article",
            "https://www.nature.com/articles/",
            "https://www.nature.com/subjects/machine-learning",
            "https://www.nature.com/nature/articles?year=2020",
            "https://www.nature.com/nature",
            // Older pages named by only part of their DOI.
            "https://www.nature.com/articles/6605004",
            "https://www.nature.com/articles/ncb0200_57",
            "https://example.com/articles/nature14539",
        ] {
            assert!(doi(url).is_err(), "{url}");
        }
    }

    #[test]
    fn springer_bmc_and_other_publisher_paths_hold_the_doi() {
        for (url, want) in [
            (
                "https://link.springer.com/article/10.1007/s00134-020-06294-x",
                "10.1007/s00134-020-06294-x",
            ),
            (
                "https://link.springer.com/content/pdf/10.1007/s00134-020-06294-x.pdf",
                "10.1007/s00134-020-06294-x",
            ),
            (
                "https://bmcmedicine.biomedcentral.com/articles/10.1186/s12916-020-01726-3",
                "10.1186/s12916-020-01726-3",
            ),
            (
                "https://onlinelibrary.wiley.com/doi/10.1111/anae.15000",
                "10.1111/anae.15000",
            ),
        ] {
            assert_eq!(doi(url).unwrap().as_str(), want, "{url}");
        }
    }

    #[test]
    fn keeps_trailing_punctuation_on_request() {
        let kept = Doi::with_trailing_punctuation("doi:10.1145/3368089.").unwrap();
        assert_eq!(kept.as_str(), "10.1145/3368089.");
        assert_eq!(
            Doi::with_trailing_punctuation("https://doi.org/10.1145/3368089;")
                .unwrap()
                .as_str(),
            "10.1145/3368089;"
        );
        assert_eq!(Doi::with_trailing_punctuation("10.1145/3368089"), None);
    }

    #[test]
    fn splits_prefix_and_suffix() {
        let d = doi("10.48550/arXiv.1706.03762").unwrap();
        assert_eq!(d.prefix(), "10.48550");
        assert_eq!(d.suffix(), "arxiv.1706.03762");
    }

    #[test]
    fn arxiv_doi_to_arxiv_id() {
        let arxiv = |s: &str| doi(s).unwrap().arxiv().map(|a| a.to_string());
        assert_eq!(
            arxiv("10.48550/arXiv.1706.03762").as_deref(),
            Some("1706.03762")
        );
        assert_eq!(
            arxiv("10.48550/arxiv.1706.03762v2").as_deref(),
            Some("1706.03762v2")
        );
        assert_eq!(
            arxiv("10.48550/arXiv.hep-th/9901001").as_deref(),
            Some("hep-th/9901001")
        );
        assert!(doi("10.48550/arxiv.x").unwrap().is_arxiv());
        assert_eq!(arxiv("10.48550/arxiv.x"), None);
        assert_eq!(arxiv("10.48550/zenodo.1706.03762"), None);
        assert!(!doi("10.1038/nature14539").unwrap().is_arxiv());
        assert_eq!(arxiv("10.1038/nature14539"), None);
    }

    #[test]
    fn arxiv_id_to_doi() {
        let to_doi = |s: &str| s.parse::<ArxivId>().unwrap().doi();
        for (id, expected) in [
            ("1706.03762", "10.48550/arxiv.1706.03762"),
            ("1706.03762v5", "10.48550/arxiv.1706.03762"),
            ("hep-th/9901001", "10.48550/arxiv.hep-th/9901001"),
            ("Hep-Th/9901001v2", "10.48550/arxiv.hep-th/9901001"),
        ] {
            let d = to_doi(id);
            assert_eq!(d.as_str(), expected, "{id}");
            assert_eq!(doi(expected).unwrap(), d, "{id} is canonical");
            assert_eq!(
                d.arxiv(),
                Some(id.parse::<ArxivId>().unwrap().unversioned())
            );
        }
    }

    #[test]
    fn rejects_malformed() {
        for input in [
            "10.1145",
            "11.1145/x",
            "10.12/x",
            "10.abcd/x",
            "10.1145/",
            "10.1145/a b",
            "https://example.org/papers/x",
        ] {
            assert!(
                matches!(
                    doi(input),
                    Err(IdError::Invalid {
                        kind: IdKind::Doi,
                        ..
                    })
                ),
                "{input:?} -> {:?}",
                doi(input)
            );
        }
    }
}
