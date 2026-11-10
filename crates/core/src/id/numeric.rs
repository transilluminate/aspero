//! Identifiers that are a positive integer, possibly behind a letter prefix.

use std::borrow::Cow;
use std::fmt;

use super::IdKind;
use super::syntax::{IdSyntax, impl_id_traits, parse_positive, strip_prefix_ci};
use super::url::Url;

macro_rules! numeric_id {
    ($(#[$meta:meta])* $name:ident, letter = $letter:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(u64);

        impl $name {
            /// `None` for zero, which is never a valid identifier.
            pub fn new(n: u64) -> Option<Self> {
                (n > 0).then_some(Self(n))
            }

            pub fn get(self) -> u64 {
                self.0
            }

            /// Strips the optional letter prefix and parses the number.
            fn parse_number(s: &str) -> Result<Self, &'static str> {
                let digits = strip_prefix_ci(s, $letter).unwrap_or(s);
                parse_positive(digits).map(Self)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}{}", $letter, self.0)
            }
        }

        impl_id_traits!($name);
    };
}

numeric_id!(
    /// PubMed ID, e.g. `31452104`.
    Pmid, letter = ""
);
numeric_id!(
    /// PubMed Central ID, canonically `PMC6716493`. The `PMC` letters are
    /// optional on input.
    Pmcid, letter = "PMC"
);
numeric_id!(
    /// OpenAlex work ID, canonically `W2741809807`. The `W` is optional when
    /// explicitly prefixed (`openalex:2741809807`).
    OpenAlexId, letter = "W"
);
numeric_id!(
    /// Semantic Scholar CorpusId, e.g. `13756489`.
    CorpusId, letter = ""
);
numeric_id!(
    /// NCBI Bookshelf ID, canonically `NBK526056`: a book or chapter, such
    /// as a StatPearls article. The `NBK` letters are optional on input.
    Nbk, letter = "NBK"
);

/// The segment after `marker` in the URL path, if present.
fn segment_after<'a>(url: &Url<'a>, marker: &str) -> Option<&'a str> {
    let mut segs = url.segments();
    segs.find(|s| *s == marker)?;
    segs.next()
}

impl IdSyntax for Pmid {
    const KIND: IdKind = IdKind::Pmid;

    fn parse_bare(s: &str) -> Result<Self, &'static str> {
        Self::parse_number(s)
    }

    fn from_url<'a>(url: &Url<'a>) -> Option<Result<Cow<'a, str>, &'static str>> {
        let seg = if url.is_host(&["pubmed.ncbi.nlm.nih.gov"]) {
            url.segments().next()
        } else if url.is_host(&["ncbi.nlm.nih.gov"]) {
            segment_after(url, "pubmed")
        } else {
            None
        };
        seg.map(|s| Ok(Cow::Borrowed(s)))
    }
}

impl IdSyntax for Pmcid {
    const KIND: IdKind = IdKind::Pmcid;

    fn parse_bare(s: &str) -> Result<Self, &'static str> {
        Self::parse_number(s)
    }

    fn from_url<'a>(url: &Url<'a>) -> Option<Result<Cow<'a, str>, &'static str>> {
        let seg = if url.is_host(&["pmc.ncbi.nlm.nih.gov", "ncbi.nlm.nih.gov"]) {
            segment_after(url, "articles")
        } else {
            None
        };
        seg.map(|s| Ok(Cow::Borrowed(s)))
    }
}

impl IdSyntax for OpenAlexId {
    const KIND: IdKind = IdKind::OpenAlex;

    fn parse_bare(s: &str) -> Result<Self, &'static str> {
        Self::parse_number(s)
    }

    /// `openalex.org/W…`, `openalex.org/works/W…`, and the same on `api.`.
    /// Other entity types (authors `A…`, sources `S…`) are not works.
    fn from_url<'a>(url: &Url<'a>) -> Option<Result<Cow<'a, str>, &'static str>> {
        if !url.is_host(&["openalex.org", "api.openalex.org"]) {
            return None;
        }
        let mut segs = url.segments();
        let seg = match segs.next()? {
            "works" => segs.next()?,
            s => s,
        };
        seg.starts_with(['W', 'w'])
            .then_some(Ok(Cow::Borrowed(seg)))
    }
}

impl IdSyntax for CorpusId {
    const KIND: IdKind = IdKind::CorpusId;

    fn parse_bare(s: &str) -> Result<Self, &'static str> {
        Self::parse_number(s)
    }

    /// Semantic Scholar's short link: `api.semanticscholar.org/CorpusID:123`.
    fn from_url<'a>(url: &Url<'a>) -> Option<Result<Cow<'a, str>, &'static str>> {
        if !url.is_host(&["api.semanticscholar.org"]) {
            return None;
        }
        let digits = strip_prefix_ci(url.segments().next()?, "corpusid:")?;
        Some(Ok(Cow::Borrowed(digits)))
    }
}

impl IdSyntax for Nbk {
    const KIND: IdKind = IdKind::Nbk;

    fn parse_bare(s: &str) -> Result<Self, &'static str> {
        Self::parse_number(s)
    }

    /// `ncbi.nlm.nih.gov/books/NBK526056/`.
    fn from_url<'a>(url: &Url<'a>) -> Option<Result<Cow<'a, str>, &'static str>> {
        if !url.is_host(&["ncbi.nlm.nih.gov"]) {
            return None;
        }
        segment_after(url, "books").map(|s| Ok(Cow::Borrowed(s)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::IdError;

    #[test]
    fn parses_and_displays() {
        assert_eq!(
            "PMID: 0031452104".parse::<Pmid>().unwrap().to_string(),
            "31452104"
        );
        assert_eq!(
            "https://www.ncbi.nlm.nih.gov/pubmed/31452104"
                .parse::<Pmid>()
                .unwrap()
                .get(),
            31452104
        );
        assert_eq!(
            "pmc6716493".parse::<Pmcid>().unwrap().to_string(),
            "PMC6716493"
        );
        assert_eq!(
            "pmcid:6716493".parse::<Pmcid>().unwrap().to_string(),
            "PMC6716493"
        );
        assert_eq!(
            "https://pmc.ncbi.nlm.nih.gov/articles/PMC6716493/"
                .parse::<Pmcid>()
                .unwrap()
                .get(),
            6716493
        );
        assert_eq!(
            "https://www.ncbi.nlm.nih.gov/pmc/articles/PMC6716493"
                .parse::<Pmcid>()
                .unwrap()
                .get(),
            6716493
        );
        assert_eq!(
            "https://api.openalex.org/works/w2741809807"
                .parse::<OpenAlexId>()
                .unwrap()
                .to_string(),
            "W2741809807"
        );
        assert_eq!(
            "openalex:2741809807".parse::<OpenAlexId>().unwrap().get(),
            2741809807
        );
        assert_eq!(
            "CorpusID:13756489".parse::<CorpusId>().unwrap().get(),
            13756489
        );
        assert_eq!(
            "https://www.ncbi.nlm.nih.gov/books/NBK526056/"
                .parse::<Nbk>()
                .unwrap()
                .to_string(),
            "NBK526056"
        );
        assert_eq!("nbk526056".parse::<Nbk>().unwrap().get(), 526056);
    }

    #[test]
    fn rejects_zero_non_digits_and_overflow() {
        for input in ["0", "PMID:abc", "12a", "99999999999999999999999"] {
            assert!(
                matches!(input.parse::<Pmid>(), Err(IdError::Invalid { .. })),
                "{input:?}"
            );
        }
        assert!(
            "https://openalex.org/A5023888391"
                .parse::<OpenAlexId>()
                .is_err()
        );
        assert_eq!(Pmid::new(0), None);
    }
}
