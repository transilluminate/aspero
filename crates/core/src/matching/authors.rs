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
