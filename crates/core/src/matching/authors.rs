//! Author names: family names from records and from claims.

use super::text::{normalise, words};
use crate::work::Person;

/// A person's family name, normalised: the `family` field, or the last
/// word of a literal name.
pub(super) fn family_name(p: &Person) -> Option<String> {
    let name = p.family.as_deref().or(p.literal.as_deref())?;
    let last = if p.family.is_some() {
        normalise(name)
    } else {
        words(name).pop()?
    };
    (!last.is_empty()).then_some(last)
}

/// The family name in a claimed author string: "Vaswani, A." → "vaswani",
/// "Ashish Vaswani" → "vaswani", "A. Vaswani" → "vaswani", "Daum RS" →
/// "daum", "A. van der Berg" → "van der berg".
pub(super) fn claimed_family(name: &str) -> Option<String> {
    let family = match name.split_once(',') {
        Some((before, _)) => normalise(before),
        None => {
            // Vancouver initials after the name ("Daum RS") go first.
            let mut tokens: Vec<&str> = name.split_whitespace().collect();
            while tokens.len() > 1 && tokens.last().is_some_and(|t| is_initials(t)) {
                tokens.pop();
            }
            let name = tokens.join(" ");
            let literal = Person {
                literal: Some(name.clone()),
                ..Person::default()
            };
            match literal.parts() {
                Some((family, _)) => normalise(&family),
                // The last word that isn't an initial.
                None => words(&name).into_iter().rfind(|w| w.chars().count() > 1)?,
            }
        }
    };
    (!family.is_empty()).then_some(family)
}

/// Initials as Vancouver writes them: "RS", "J", "T.D.N.".
fn is_initials(token: &str) -> bool {
    let letters: Vec<char> = token
        .trim_end_matches([',', ';'])
        .chars()
        .filter(|c| *c != '.')
        .collect();
    (1..=3).contains(&letters.len()) && letters.iter().all(|c| c.is_uppercase())
}

/// The same family, give or take particles: "van der berg" and "berg".
fn same_family(a: &str, b: &str) -> bool {
    let ends = |long: &str, short: &str| {
        long.strip_suffix(short)
            .is_some_and(|rest| rest.ends_with(' '))
    };
    a == b || ends(a, b) || ends(b, a)
}

/// Claimed authors vs the record's family names: half for the first author
/// matching, half for the share of claimed authors found anywhere.
pub(super) fn author_overlap(claimed: &[String], families: &[String]) -> f64 {
    let claimed: Vec<String> = claimed.iter().filter_map(|n| claimed_family(n)).collect();
    let Some(first) = claimed.first() else {
        return 0.0;
    };
    let first_matches = families.first().is_some_and(|f| same_family(f, first));
    let found = claimed
        .iter()
        .filter(|c| families.iter().any(|f| same_family(f, c)))
        .count();
    0.5 * f64::from(u8::from(first_matches)) + 0.5 * found as f64 / claimed.len() as f64
}

/// Whether a reference names authors: "et al", or a family name followed
/// by initials ("Laan DV", "Smith, J."). A family name is never followed by
/// a full stop, so "Int. J. Care Injured" in a running header isn't one.
pub(super) fn names_given(text: &str) -> bool {
    if normalise(text).contains("et al") {
        return true;
    }
    let tokens: Vec<&str> = text.split_whitespace().collect();
    tokens.windows(2).any(|w| {
        let family = w[0].trim_end_matches(',');
        let initials: String = w[1]
            .trim_end_matches([',', ';', '.'])
            .chars()
            .filter(|c| *c != '.')
            .collect();
        !w[0].ends_with('.')
            && family.chars().count() >= 2
            && family.chars().next().is_some_and(char::is_uppercase)
            && family
                .chars()
                .all(|c| c.is_alphabetic() || matches!(c, '-' | '\''))
            && (1..=3).contains(&initials.chars().count())
            && initials.chars().all(|c| c.is_uppercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claimed_families() {
        for (name, family) in [
            ("Vaswani, A.", "vaswani"),
            ("Ashish Vaswani", "vaswani"),
            ("A. Vaswani", "vaswani"),
            ("Daum RS", "daum"),
            ("Daum R.S.", "daum"),
            ("LeCun Y", "lecun"),
            ("A. van der Berg", "van der berg"),
            ("van der Berg, A.", "van der berg"),
        ] {
            assert_eq!(claimed_family(name).as_deref(), Some(family), "{name}");
        }
    }

    #[test]
    fn particles_dont_part_families() {
        let families = ["berg".to_owned(), "smith".to_owned()];
        let claimed = ["A. van der Berg".to_owned(), "Smith J".to_owned()];
        assert_eq!(author_overlap(&claimed, &families), 1.0);
        assert!(!same_family("goldsmith", "smith"));
    }
}
