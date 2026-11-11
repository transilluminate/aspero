//! Just enough URL handling to recognise identifier URLs. Not a general URL
//! parser: no ports, userinfo or IDNA.

use std::borrow::Cow;

use super::syntax::strip_prefix_ci;

/// Hosts accepted without a scheme (`arxiv.org/abs/…`). With a scheme, any
/// host parses, and the identifier types decide whether it is theirs.
const SCHEMELESS_HOSTS: &[&str] = &[
    "doi.org",
    "dx.doi.org",
    "arxiv.org",
    "export.arxiv.org",
    "openalex.org",
    "api.openalex.org",
    "pubmed.ncbi.nlm.nih.gov",
    "ncbi.nlm.nih.gov",
    "pmc.ncbi.nlm.nih.gov",
    "semanticscholar.org",
    "api.semanticscholar.org",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Url<'a> {
    /// Lowercased, with any leading `www.` removed.
    pub host: Cow<'a, str>,
    /// Raw (still percent-encoded) path without the leading `/`, query or
    /// fragment.
    pub path: &'a str,
}

impl<'a> Url<'a> {
    pub fn parse(s: &'a str) -> Option<Self> {
        let (rest, had_scheme) =
            match strip_prefix_ci(s, "https://").or_else(|| strip_prefix_ci(s, "http://")) {
                Some(rest) => (rest, true),
                None => (s, false),
            };
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        let host = strip_prefix_ci(host, "www.").unwrap_or(host);
        let host = if host.bytes().any(|b| b.is_ascii_uppercase()) {
            Cow::Owned(host.to_ascii_lowercase())
        } else {
            Cow::Borrowed(host)
        };
        if host.is_empty() || (!had_scheme && !SCHEMELESS_HOSTS.contains(&&*host)) {
            return None;
        }
        let path = path.split(['?', '#']).next().unwrap_or_default();
        Some(Url { host, path })
    }

    pub fn is_host(&self, hosts: &[&str]) -> bool {
        hosts.contains(&&*self.host)
    }

    pub fn segments(&self) -> impl Iterator<Item = &'a str> {
        self.path.split('/').filter(|s| !s.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scheme_host_and_path() {
        let url = Url::parse("HTTPS://WWW.ArXiv.org/abs/1706.03762?context=cs#x").unwrap();
        assert_eq!(url.host, "arxiv.org");
        assert_eq!(url.path, "abs/1706.03762");
    }

    #[test]
    fn schemeless_only_for_known_hosts() {
        assert!(Url::parse("doi.org/10.1/x").is_some());
        assert!(Url::parse("example.com/10.1/x").is_none());
        assert!(Url::parse("10.1234/abc").is_none());
    }
}
