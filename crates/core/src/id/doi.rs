use std::borrow::Cow;
use std::fmt;

use super::IdKind;
use super::syntax::{IdSyntax, clean_pasted, impl_id_traits, strip_kind_prefix, strip_prefix_ci};
use super::url::Url;

const INFO_PREFIX: &str = "info:doi/";

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
        let digits = |g: &str| !g.is_empty() && g.bytes().all(|b| b.is_ascii_digit());
        if !(4..=9).contains(&first.len()) || !digits(first) || !groups.all(digits) {
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

    fn from_url<'a>(url: &Url<'a>) -> Option<Result<Cow<'a, str>, &'static str>> {
        url.is_host(&["doi.org", "dx.doi.org"]).then(|| {
            percent_encoding::percent_decode_str(url.path)
                .decode_utf8()
                .map_err(|_| "not valid UTF-8 after percent-decoding")
        })
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
    fn rejects_malformed() {
        for input in [
            "10.1145",
            "11.1145/x",
            "10.12/x",
            "10.abcd/x",
            "10.1145/",
            "10.1145/a b",
            "https://example.org/10.1145/x",
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
