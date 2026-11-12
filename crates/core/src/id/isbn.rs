use std::borrow::Cow;
use std::fmt;

use super::IdKind;
use super::syntax::{IdSyntax, impl_id_traits, strip_prefix_ci};
use super::url::Url;

/// An International Standard Book Number, canonically ISBN-13 digits
/// (`9780262035613`).
///
/// Accepts ISBN-10 and ISBN-13, with or without hyphens or spaces, with an
/// `ISBN`, `ISBN-10` or `ISBN-13` label; both checksums are validated, and
/// ISBN-10s are converted, so `0-262-03561-8` equals `978-0-262-03561-3`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Isbn(String);

impl Isbn {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Digits (and a final `X`) with separators removed.
fn compact(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '-' | ' ' | '\u{2010}' | '\u{2011}'))
        .collect()
}

fn digit(c: u8) -> u32 {
    u32::from(c - b'0')
}

fn isbn13_check(first12: &[u8]) -> u8 {
    let sum: u32 = first12
        .iter()
        .enumerate()
        .map(|(i, &c)| digit(c) * if i % 2 == 0 { 1 } else { 3 })
        .sum();
    b'0' + ((10 - sum % 10) % 10) as u8
}

impl IdSyntax for Isbn {
    const KIND: IdKind = IdKind::Isbn;

    fn parse_bare(s: &str) -> Result<Self, &'static str> {
        let s = compact(s).to_ascii_uppercase();
        let b = s.as_bytes();
        match b.len() {
            13 => {
                if !b.iter().all(u8::is_ascii_digit) {
                    return Err("ISBN-13 must be 13 digits");
                }
                if !(s.starts_with("978") || s.starts_with("979")) {
                    return Err("ISBN-13 must start with 978 or 979");
                }
                if isbn13_check(&b[..12]) != b[12] {
                    return Err("ISBN-13 check digit is wrong");
                }
                Ok(Isbn(s))
            }
            10 => {
                let (body, check) = b.split_at(9);
                if !body.iter().all(u8::is_ascii_digit)
                    || !(check[0].is_ascii_digit() || check[0] == b'X')
                {
                    return Err("ISBN-10 must be 9 digits and a digit or X");
                }
                // Weighted 10 down to 2, then the check digit 1.
                let sum: u32 = body
                    .iter()
                    .zip((2..=10).rev())
                    .map(|(&c, weight)| digit(c) * weight)
                    .sum::<u32>()
                    + if check[0] == b'X' {
                        10
                    } else {
                        digit(check[0])
                    };
                if !sum.is_multiple_of(11) {
                    return Err("ISBN-10 check digit is wrong");
                }
                let mut isbn13 = format!("978{}", &s[..9]).into_bytes();
                isbn13.push(isbn13_check(&isbn13));
                Ok(Isbn(String::from_utf8(isbn13).expect("ASCII digits")))
            }
            _ => Err("expected 10 or 13 digits"),
        }
    }

    /// `openlibrary.org/isbn/<isbn>`.
    fn from_url<'a>(url: &Url<'a>) -> Option<Result<Cow<'a, str>, &'static str>> {
        if !url.is_host(&["openlibrary.org"]) {
            return None;
        }
        let mut segs = url.segments();
        (segs.next()? == "isbn").then(|| segs.next().map(Cow::Borrowed).ok_or("missing ISBN"))
    }

    /// `isbn:`, `ISBN `, `ISBN-13:`, `ISBN-10 `.
    fn strip_prefix(s: &str) -> &str {
        for label in ["isbn-13", "isbn-10", "isbn13", "isbn10", "isbn"] {
            if let Some(rest) = strip_prefix_ci(s, label) {
                return rest.trim_start_matches([':', ' ']).trim_start();
            }
        }
        s
    }
}

impl_id_traits!(Isbn);

impl fmt::Display for Isbn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::IdError;

    fn isbn(s: &str) -> Result<Isbn, IdError> {
        s.parse()
    }

    #[test]
    fn isbn_10_and_13_forms_are_the_same_book() {
        for input in [
            "9780262035613",
            "978-0-262-03561-3",
            "ISBN 978-0-262-03561-3",
            "ISBN-13: 9780262035613",
            "isbn:0262035618",
            "0-262-03561-8",
            "ISBN-10 0262035618",
            "https://openlibrary.org/isbn/9780262035613",
        ] {
            assert_eq!(isbn(input).unwrap().as_str(), "9780262035613", "{input:?}");
        }
    }

    #[test]
    fn x_check_digits_and_979_prefixes() {
        assert_eq!(isbn("0-8044-2957-X").unwrap().as_str(), "9780804429573");
        assert!(isbn("979-10-90636-07-1").is_ok());
    }

    #[test]
    fn rejects_bad_checksums_and_lengths() {
        for input in [
            "9780262035614",
            "0262035617",
            "12345",
            "9770262035613",
            "ABCDEFGHIJ",
        ] {
            assert!(
                matches!(
                    isbn(input),
                    Err(IdError::Invalid {
                        kind: IdKind::Isbn,
                        ..
                    })
                ),
                "{input:?}"
            );
        }
    }
}
