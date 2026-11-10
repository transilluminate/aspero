use std::borrow::Cow;
use std::fmt;

use super::IdKind;
use super::syntax::{IdSyntax, impl_id_traits};
use super::url::Url;

/// Semantic Scholar paperId: 40 hex characters, canonically lowercase.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct S2PaperId(String);

impl S2PaperId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl IdSyntax for S2PaperId {
    const KIND: IdKind = IdKind::S2Paper;

    fn parse_bare(s: &str) -> Result<Self, &'static str> {
        if s.len() != 40 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("expected 40 hex characters");
        }
        Ok(S2PaperId(s.to_ascii_lowercase()))
    }

    /// `semanticscholar.org/paper/<slug>/<id>` or `/paper/<id>`.
    fn from_url<'a>(url: &Url<'a>) -> Option<Result<Cow<'a, str>, &'static str>> {
        if !url.is_host(&["semanticscholar.org"]) {
            return None;
        }
        let mut segs = url.segments();
        if segs.next()? != "paper" {
            return None;
        }
        segs.last().map(|s| Ok(Cow::Borrowed(s)))
    }
}

impl_id_traits!(S2PaperId);

impl fmt::Display for S2PaperId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "204e3073870fae3d05bcbc2f6a8e263d9b72e776";

    #[test]
    fn parses_bare_prefixed_and_url() {
        for input in [
            "204E3073870FAE3D05BCBC2F6A8E263D9B72E776",
            "s2:204e3073870fae3d05bcbc2f6a8e263d9b72e776",
            "https://www.semanticscholar.org/paper/Attention-is-All-you-Need-Vaswani-Shazeer/204e3073870fae3d05bcbc2f6a8e263d9b72e776",
            "https://www.semanticscholar.org/paper/204e3073870fae3d05bcbc2f6a8e263d9b72e776",
        ] {
            assert_eq!(
                input.parse::<S2PaperId>().unwrap().as_str(),
                ID,
                "{input:?}"
            );
        }
    }

    #[test]
    fn rejects_wrong_length_or_non_hex() {
        assert!("204e3073".parse::<S2PaperId>().is_err());
        assert!(format!("{}g", &ID[..39]).parse::<S2PaperId>().is_err());
    }
}
