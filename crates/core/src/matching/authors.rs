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
/// "Ashish Vaswani" → "vaswani", "A. Vaswani" → "vaswani".
pub(super) fn claimed_family(name: &str) -> Option<String> {
    let family = match name.split_once(',') {
        Some((before, _)) => normalise(before),
        // The last word that isn't an initial.
        None => words(name).into_iter().rfind(|w| w.chars().count() > 1)?,
    };
    (!family.is_empty()).then_some(family)
}

/// Claimed authors vs the record's family names: half for the first author
/// matching, half for the share of claimed authors found anywhere.
pub(super) fn author_overlap(claimed: &[String], families: &[String]) -> f64 {
    let claimed: Vec<String> = claimed.iter().filter_map(|n| claimed_family(n)).collect();
    let Some(first) = claimed.first() else {
        return 0.0;
    };
    let first_matches = families.first() == Some(first);
    let found = claimed.iter().filter(|c| families.contains(c)).count();
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
