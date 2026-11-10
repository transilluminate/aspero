use std::borrow::Cow;
use std::fmt;

use super::IdKind;
use super::syntax::{IdSyntax, impl_id_traits, parse_positive};
use super::url::Url;

/// An arXiv identifier, optionally pinned to a version.
///
/// - New style (April 2007 on): `YYMM.NNNN` up to 1412, `YYMM.NNNNN` from 1501.
/// - Old style: `archive/YYMMNNN`, e.g. `hep-th/9901001`. A subject class
///   (`math.GT/0309136`) is accepted and dropped, as arXiv itself does.
///
/// Equality includes the version; use [`ArxivId::unversioned`] to compare
/// works rather than specific versions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ArxivId {
    id: String,
    version: Option<u32>,
}

impl ArxivId {
    /// The identifier without any version suffix.
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn version(&self) -> Option<u32> {
        self.version
    }

    pub fn unversioned(&self) -> ArxivId {
        ArxivId {
            id: self.id.clone(),
            version: None,
        }
    }

    pub fn is_old_style(&self) -> bool {
        self.id.contains('/')
    }
}

/// Splits a trailing `vN` off, if `s` ends with one after a digit.
fn split_version(s: &str) -> Result<(&str, Option<u32>), &'static str> {
    let Some(pos) = s.rfind(['v', 'V']) else {
        return Ok((s, None));
    };
    let (base, ver) = (&s[..pos], &s[pos + 1..]);
    let follows_digit = base.ends_with(|c: char| c.is_ascii_digit());
    if !follows_digit || ver.is_empty() || !ver.bytes().all(|b| b.is_ascii_digit()) {
        return Ok((s, None));
    }
    let n = parse_positive(ver).map_err(|_| "version must be v1 or later")?;
    let n = u32::try_from(n).map_err(|_| "version number too large")?;
    Ok((base, Some(n)))
}

fn valid_month(yymm: &str) -> bool {
    matches!(yymm[2..].parse::<u8>(), Ok(1..=12))
}

fn parse_new_style(s: &str) -> Result<String, &'static str> {
    let (yymm, seq) = s.split_once('.').ok_or("unrecognised arXiv identifier")?;
    let digits = |t: &str| t.bytes().all(|b| b.is_ascii_digit());
    if yymm.len() != 4 || !digits(yymm) || !digits(seq) {
        return Err("expected YYMM.NNNNN");
    }
    if !valid_month(yymm) {
        return Err("month must be 01-12");
    }
    let expected = match yymm.parse::<u16>().expect("four digits") {
        ..704 => return Err("new-style identifiers start at 0704"),
        704..=1412 => 4,
        _ => 5,
    };
    if seq.len() != expected {
        return Err("wrong number of digits after YYMM. for this month");
    }
    Ok(s.to_owned())
}

fn parse_old_style(s: &str) -> Result<String, &'static str> {
    let (archive, num) = s.split_once('/').ok_or("unrecognised arXiv identifier")?;
    let archive = archive.split_once('.').map_or(archive, |(a, _subject)| a);
    let archive = archive.to_ascii_lowercase();
    if archive.is_empty() || !archive.bytes().all(|b| b.is_ascii_lowercase() || b == b'-') {
        return Err("archive must be letters and '-', e.g. hep-th");
    }
    if num.len() != 7 || !num.bytes().all(|b| b.is_ascii_digit()) {
        return Err("expected archive/YYMMNNN");
    }
    if !valid_month(&num[..4]) {
        return Err("month must be 01-12");
    }
    Ok(format!("{archive}/{num}"))
}

impl IdSyntax for ArxivId {
    const KIND: IdKind = IdKind::Arxiv;

    fn parse_bare(s: &str) -> Result<Self, &'static str> {
        let (base, version) = split_version(s)?;
        let id = if base.contains('/') {
            parse_old_style(base)?
        } else {
            parse_new_style(base)?
        };
        Ok(ArxivId { id, version })
    }

    fn from_url<'a>(url: &Url<'a>) -> Option<Result<Cow<'a, str>, &'static str>> {
        if !url.is_host(&["arxiv.org", "export.arxiv.org"]) {
            return None;
        }
        let (view, rest) = url.path.split_once('/')?;
        if !matches!(view, "abs" | "pdf" | "html") {
            return None;
        }
        let rest = rest.trim_end_matches('/');
        Some(Ok(Cow::Borrowed(rest.strip_suffix(".pdf").unwrap_or(rest))))
    }
}

impl_id_traits!(ArxivId);

impl fmt::Display for ArxivId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.id)?;
        match self.version {
            Some(v) => write!(f, "v{v}"),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::IdError;

    fn arxiv(s: &str) -> Result<ArxivId, IdError> {
        s.parse()
    }

    #[test]
    fn parses_new_and_old_styles() {
        let cases = [
            ("1706.03762", "1706.03762", None),
            ("arXiv:1706.03762V5", "1706.03762", Some(5)),
            ("0704.0001", "0704.0001", None),
            ("1412.6980v9", "1412.6980", Some(9)),
            ("hep-th/9901001", "hep-th/9901001", None),
            ("math.GT/0309136v2", "math/0309136", Some(2)),
            ("HEP-TH/9901001", "hep-th/9901001", None),
            ("https://arxiv.org/abs/1706.03762v7", "1706.03762", Some(7)),
            ("https://arxiv.org/pdf/1706.03762.pdf", "1706.03762", None),
            ("arxiv.org/abs/hep-th/9901001", "hep-th/9901001", None),
            (
                "https://export.arxiv.org/abs/2101.00001/",
                "2101.00001",
                None,
            ),
        ];
        for (input, id, version) in cases {
            let a = arxiv(input).unwrap_or_else(|e| panic!("{input:?}: {e}"));
            assert_eq!((a.id(), a.version()), (id, version), "{input:?}");
        }
    }

    #[test]
    fn version_is_part_of_equality_but_not_of_unversioned() {
        let a = arxiv("1706.03762v5").unwrap();
        let b = arxiv("1706.03762").unwrap();
        assert_ne!(a, b);
        assert_eq!(a.unversioned(), b);
        assert_eq!(a.to_string(), "1706.03762v5");
    }

    #[test]
    fn rejects_malformed() {
        for input in [
            "1706.0376",  // too short for 2017
            "1412.69800", // too long for 2014
            "0703.0001",  // predates new style
            "1713.00001", // month 13
            "1706.03762v0",
            "hep_th/9901001",
            "hep-th/990100",
            "https://arxiv.org/list/cs.AI/recent",
        ] {
            assert!(
                matches!(
                    arxiv(input),
                    Err(IdError::Invalid {
                        kind: IdKind::Arxiv,
                        ..
                    })
                ),
                "{input:?} -> {:?}",
                arxiv(input)
            );
        }
    }
}
