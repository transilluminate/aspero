use std::borrow::Cow;
use std::fmt;

use super::IdKind;
use super::syntax::{IdSyntax, impl_id_traits, strip_prefix_ci};
use super::url::Url;

/// Elsevier's Publisher Item Identifier, canonically compact:
/// `S0140673620301835`, an `S`, the journal's ISSN, the year's last two
/// digits, a five-digit item number and a check character. Printed and in
/// Cell's and the Lancet's URLs it is punctuated (`S0140-6736(20)30183-5`);
/// the punctuation is dropped. Crossref holds PIIs in the compact form only
/// (upper case), and they are found there, not by any other source.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Pii(String);

impl Pii {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Hosts that print a PII after one of [`PII_MARKERS`] in their paths:
/// Cell Press and The Lancet.
const PII_HOSTS: &[&str] = &["cell.com", "thelancet.com"];

/// The path segments a PII follows on [`PII_HOSTS`]: `…/fulltext/S…`,
/// `…/abstract/S…`, `…/pdf/S….pdf`, `…/article/PIIS…/fulltext`.
const PII_MARKERS: &[&str] = &["fulltext", "abstract", "pdf", "article"];

impl IdSyntax for Pii {
    const KIND: IdKind = IdKind::Pii;

    fn parse_bare(s: &str) -> Result<Self, &'static str> {
        let compact: String = s
            .chars()
            .filter(|c| !matches!(c, '-' | '(' | ')'))
            .map(|c| c.to_ascii_uppercase())
            .collect();
        let b = compact.as_bytes();
        // The ISSN's check character (the ninth) and the PII's own (the
        // last) may be an X.
        let valid = b.len() == 17
            && b[0] == b'S'
            && b[1..]
                .iter()
                .enumerate()
                .all(|(i, &c)| c.is_ascii_digit() || (c == b'X' && (i == 7 || i == 15)));
        if !valid {
            return Err("expected S and 16 digits, e.g. S0140-6736(20)30183-5");
        }
        Ok(Pii(compact))
    }

    /// ScienceDirect's `science/article/pii/<PII>` (and `/abs/pii/`, `/am/pii/`),
    /// Elsevier's `linkinghub.elsevier.com/retrieve/pii/<PII>`, and Cell
    /// Press's and The Lancet's article pages (`cell.com/cell/fulltext/S…`,
    /// `thelancet.com/journals/lancet/article/PIIS…/fulltext`).
    fn from_url<'a>(url: &Url<'a>) -> Option<Result<Cow<'a, str>, &'static str>> {
        let mut segs = url.segments();
        let seg = if url.is_host(&["sciencedirect.com"]) {
            if (segs.next()?, segs.next()?) != ("science", "article") {
                return None;
            }
            segs.find(|s| *s == "pii")?;
            segs.next()?
        } else if url.is_host(&["linkinghub.elsevier.com"]) {
            if (segs.next()?, segs.next()?) != ("retrieve", "pii") {
                return None;
            }
            segs.next()?
        } else if url.is_host(PII_HOSTS) {
            // The first segment after a marker that is a PII: a journal's
            // name or a page's kind isn't.
            let mut prev = "";
            return segs.find_map(|seg| {
                let after_marker = PII_MARKERS.contains(&prev);
                prev = seg;
                let bare = decode(seg)?;
                let bare = strip_prefix_ci(&bare, "pii").unwrap_or(&bare);
                let bare = bare.strip_suffix(".pdf").unwrap_or(bare);
                (after_marker && Pii::parse_bare(bare).is_ok()).then(|| Ok(Cow::Owned(bare.into())))
            });
        } else {
            return None;
        };
        Some(decode(seg).ok_or("not valid UTF-8 after percent-decoding"))
    }

    fn strip_prefix(s: &str) -> &str {
        super::syntax::strip_kind_prefix(s, "pii")
    }
}

/// A path segment percent-decoded (browsers encode a PII's brackets).
fn decode(seg: &str) -> Option<Cow<'_, str>> {
    percent_encoding::percent_decode_str(seg).decode_utf8().ok()
}

impl_id_traits!(Pii);

impl fmt::Display for Pii {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::IdError;

    fn pii(s: &str) -> Result<Pii, IdError> {
        s.parse()
    }

    #[test]
    fn punctuated_and_compact_forms_are_one_pii() {
        for input in [
            "S0140673620301835",
            "s0140673620301835",
            "S0140-6736(20)30183-5",
            "pii:S0140-6736(20)30183-5",
            "PII: S0140673620301835",
        ] {
            assert_eq!(
                pii(input).unwrap().as_str(),
                "S0140673620301835",
                "{input:?}"
            );
        }
        // An X checks the ISSN or the PII itself.
        assert_eq!(
            pii("S2214-109X(20)30102-X").unwrap().as_str(),
            "S2214109X2030102X"
        );
    }

    #[test]
    fn elsevier_urls_give_their_pii() {
        for (url, want) in [
            (
                "https://www.sciencedirect.com/science/article/pii/S2772940026000032",
                "S2772940026000032",
            ),
            (
                "https://www.sciencedirect.com/science/article/abs/pii/S0140673620301835?via%3Dihub",
                "S0140673620301835",
            ),
            (
                "https://www.sciencedirect.com/science/article/pii/S0092867420302294/pdfft?md5=x",
                "S0092867420302294",
            ),
            (
                "https://linkinghub.elsevier.com/retrieve/pii/S0140673620301835",
                "S0140673620301835",
            ),
            (
                "https://www.cell.com/cell/fulltext/S0092-8674(20)30229-4",
                "S0092867420302294",
            ),
            (
                "https://www.cell.com/fulltext/S0092867420302294",
                "S0092867420302294",
            ),
            (
                "https://www.cell.com/cell/pdf/S0092-8674(20)30229-4.pdf",
                "S0092867420302294",
            ),
            (
                "https://www.cell.com/cell/fulltext/S0092-8674%2820%2930229-4#secsectitle0010",
                "S0092867420302294",
            ),
            (
                "https://www.thelancet.com/journals/lancet/article/PIIS0140-6736(20)30183-5/fulltext",
                "S0140673620301835",
            ),
            (
                "thelancet.com/journals/langlo/article/PIIS2214-109X(20)30102-X/fulltext",
                "S2214109X2030102X",
            ),
        ] {
            assert_eq!(pii(url).unwrap().as_str(), want, "{url}");
        }
    }

    #[test]
    fn other_pages_on_their_hosts_are_not_piis() {
        for url in [
            "https://www.sciencedirect.com/journal/the-lancet",
            "https://www.sciencedirect.com/science/journal/01406736",
            "https://www.sciencedirect.com/search?qs=S0140673620301835",
            "https://www.cell.com/cell/home",
            "https://www.cell.com/cell/fulltext",
            "https://www.thelancet.com/journals/lancet/issue/current",
            "https://www.thelancet.com/action/showPdf?pii=S0140-6736(20)30183-5",
            "https://example.com/science/article/pii/S0140673620301835",
        ] {
            assert!(pii(url).is_err(), "{url}");
        }
        // Not a PII's shape.
        for input in [
            "S014067362030183",
            "X0140673620301835",
            "S01406736203018X5",
            "S",
        ] {
            assert!(
                matches!(
                    pii(input),
                    Err(IdError::Invalid {
                        kind: IdKind::Pii,
                        ..
                    })
                ),
                "{input:?}"
            );
        }
    }
}
