//! The seam each identifier type plugs into: it declares its bare grammar and
//! the URLs it recognises; [`parse`] handles everything else uniformly.

use std::borrow::Cow;

use super::url::Url;
use super::{IdError, IdKind};

pub(crate) trait IdSyntax: Sized {
    const KIND: IdKind;

    /// Parses an already-unwrapped value (no prefix, no URL) into canonical
    /// form. Returns a short human reason on failure.
    fn parse_bare(s: &str) -> Result<Self, &'static str>;

    /// If `url` is one of this kind's well-known URLs, extracts the bare value
    /// (decoded). `None` means "not my URL", not "invalid".
    fn from_url<'a>(url: &Url<'a>) -> Option<Result<Cow<'a, str>, &'static str>>;

    /// Removes this kind's textual prefix, if present.
    fn strip_prefix(s: &str) -> &str {
        strip_kind_prefix(s, Self::KIND.prefix())
    }
}

/// The full `FromStr` pipeline: trim, strip prefix, unwrap URL, parse bare.
pub(crate) fn parse<T: IdSyntax>(input: &str) -> Result<T, IdError> {
    let s = clean_pasted(input);
    if s.is_empty() {
        return Err(IdError::Empty);
    }
    let s = T::strip_prefix(s);
    let bare = match Url::parse(s) {
        Some(url) => match T::from_url(&url) {
            Some(bare) => bare,
            None => Err("not a recognised URL for this kind"),
        },
        None => Ok(Cow::Borrowed(s)),
    };
    finish(bare, input)
}

pub(crate) fn finish<T: IdSyntax>(
    bare: Result<Cow<'_, str>, &'static str>,
    input: &str,
) -> Result<T, IdError> {
    bare.and_then(|b| T::parse_bare(&b))
        .map_err(|reason| IdError::Invalid {
            kind: T::KIND,
            input: input.to_owned(),
            reason,
        })
}

/// Removes what clings to identifiers pasted from Markdown or prose:
/// surrounding brackets, emphasis (`*`, `_`), quotes and backticks, and
/// trailing sentence punctuation. A closing bracket goes only when it is
/// unbalanced, so DOIs like `10.1016/S0140-6736(10)60835-5` stay whole.
pub(crate) fn clean_pasted(input: &str) -> &str {
    const LEADING: &[char] = &[
        '(', '[', '{', '<', '*', '_', '"', '\'', '`', '\u{201c}', '\u{2018}',
    ];
    const TRAILING: &[char] = &[
        '*', '_', '"', '\'', '`', '>', '\u{201d}', '\u{2019}', '.', ',', ';', ':',
    ];
    let unbalanced = |s: &str, open: char, close: char| {
        s.ends_with(close) && s.matches(open).count() < s.matches(close).count()
    };
    let mut s = input.trim();
    loop {
        let before = s.len();
        s = s
            .trim_start_matches(LEADING)
            .trim_end_matches(TRAILING)
            .trim();
        for (open, close) in [('(', ')'), ('[', ']'), ('{', '}')] {
            if unbalanced(s, open, close) {
                s = &s[..s.len() - close.len_utf8()];
            }
        }
        if s.len() == before {
            return s;
        }
    }
}

/// Strips `prefix:` (ASCII case-insensitive) and any following whitespace.
pub(crate) fn strip_kind_prefix<'a>(s: &'a str, prefix: &str) -> &'a str {
    match s.split_once(':') {
        Some((head, rest)) if head.trim().eq_ignore_ascii_case(prefix) => rest.trim_start(),
        _ => s,
    }
}

pub(crate) fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

/// Parses a strictly positive decimal integer; leading zeros are accepted.
pub(crate) fn parse_positive(s: &str) -> Result<u64, &'static str> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err("expected digits");
    }
    match s.parse::<u64>() {
        Ok(0) => Err("must be greater than zero"),
        Ok(n) => Ok(n),
        Err(_) => Err("number too large"),
    }
}

/// `FromStr` via [`parse`] plus string-form serde, for an [`IdSyntax`] type.
macro_rules! impl_id_traits {
    ($ty:ty) => {
        impl ::std::str::FromStr for $ty {
            type Err = $crate::id::IdError;

            fn from_str(input: &str) -> Result<Self, Self::Err> {
                $crate::id::syntax::parse(input)
            }
        }

        $crate::id::syntax::impl_string_serde!($ty);
    };
}

/// Serialises via `Display`, deserialises via `FromStr` (so it is lenient
/// about input form and canonical on output).
macro_rules! impl_string_serde {
    ($ty:ty) => {
        impl ::serde::Serialize for $ty {
            fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.collect_str(self)
            }
        }

        impl<'de> ::serde::Deserialize<'de> for $ty {
            fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = <::std::borrow::Cow<'de, str>>::deserialize(d)?;
                s.parse().map_err(::serde::de::Error::custom)
            }
        }
    };
}

pub(crate) use {impl_id_traits, impl_string_serde};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasted_wrappers_come_off_but_balanced_brackets_stay() {
        for (input, want) in [
            ("10.1056/NEJMoa072006)", "10.1056/NEJMoa072006"),
            ("10.1056/NEJMoa072006))", "10.1056/NEJMoa072006"),
            ("10.1056/NEJMoa072006):", "10.1056/NEJMoa072006"),
            ("**10.1056/NEJMoa072006**.", "10.1056/NEJMoa072006"),
            ("(doi:10.1056/NEJMoa072006)", "doi:10.1056/NEJMoa072006"),
            ("`arXiv:1706.03762`", "arXiv:1706.03762"),
            (
                "10.1016/S0140-6736(10)60835-5",
                "10.1016/S0140-6736(10)60835-5",
            ),
            (
                "10.1016/S0140-6736(10)60835-5).",
                "10.1016/S0140-6736(10)60835-5",
            ),
            (
                "[10.1002/(SICI)1097-4636(199601)]",
                "10.1002/(SICI)1097-4636(199601)",
            ),
        ] {
            assert_eq!(clean_pasted(input), want, "{input:?}");
        }
    }
}
