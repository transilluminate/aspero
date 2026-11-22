//! Author names: family names from records and from claims.

use super::text::{STOPWORDS, normalise, spanned_words, words};
use crate::publication::{is_month, journal_word, significant};
use crate::work::{Person, is_body_word, is_particle};

/// Authors at which a source's byline may be cut short (OpenAlex lists at
/// most 100): a name missing from it proves nothing.
const FULL_BYLINE: usize = 100;

/// Lowercase words a byline runs on through ("et al.", "and", "and
/// others", "on behalf of"); any other ends it ("Huang et al. described
/// in The Lancet").
const BYLINE_WORDS: &[&str] = &[
    "et", "al", "and", "others", "y", "e", "u", "und", "on", "behalf", "of", "the",
];

/// Capitalised words before a title that aren't names: a reference's
/// furniture (a model's field, "Title: …"), a notice's label
/// ("Correction: …").
const NOT_NAMES: &[&str] = &[
    "title",
    "jr",
    "sr",
    "dr",
    "prof",
    "ed",
    "eds",
    "editor",
    "editors",
    "anonymous",
    "author",
    "authors",
    "see",
    "ref",
    "available",
    "accessed",
    "retrieved",
    "online",
    "correction",
    "corrections",
    "erratum",
    "errata",
    "corrigendum",
    "retraction",
    "reply",
    "comment",
    "response",
    "re",
    "editorial",
    "letter",
    "others",
];

/// People a byline naming an organisation too must list to be taken as
/// complete: "Meyer G, Vicaut E, …, PEITHO Investigators" is cited by its
/// people; "RECOVERY Collaborative Group" alone may be cited by its
/// members' names.
const PEOPLE_BESIDE_BODIES: usize = 3;

/// Whether a record's byline can be taken as complete, so that a name it
/// lacks is not an author: not cut short, and not an organisation's (a
/// consortium's members may be cited by name) unless people are listed
/// with it.
pub(super) fn complete_byline(authors: &[Person]) -> bool {
    let people = authors
        .iter()
        .filter(|p| {
            p.parts().is_some()
                || p.literal
                    .as_deref()
                    .is_some_and(|n| uncased_name(n) || shouted_name(n) || family_alone(n))
        })
        .count();
    !authors.is_empty()
        && authors.len() < FULL_BYLINE
        && (people == authors.len() || people >= PEOPLE_BESIDE_BODIES)
}

/// A person's name in capitals, as old records print them: two to four
/// words, one an initial ("JOHN R. JAENIKE"); an organisation's has none
/// ("WORLD HEALTH ORGANIZATION").
fn shouted_name(name: &str) -> bool {
    let parts: Vec<&str> = name.split_whitespace().collect();
    let letters = |w: &str| w.chars().filter(|c| c.is_alphabetic()).count();
    (2..=4).contains(&parts.len())
        && parts.iter().all(|w| {
            w.chars()
                .all(|c| c.is_uppercase() || matches!(c, '.' | '-' | '\'' | '’'))
        })
        && parts.iter().any(|w| letters(w) == 1)
        && parts.iter().any(|w| letters(w) >= 2)
}

/// A family name alone, as some records give a byline ("Silverman"): one
/// capitalised word, in lower case after its first letter but for a
/// prefix's capital (a Scottish or Dutch prefix's).
fn family_alone(name: &str) -> bool {
    let name = name.trim();
    !name.contains(char::is_whitespace)
        && name.chars().count() >= 3
        && name.starts_with(char::is_uppercase)
        && name.chars().filter(|c| c.is_lowercase()).count() >= 2
        && name.chars().filter(|c| c.is_uppercase()).count() <= 2
        && name
            .chars()
            .all(|c| c.is_alphabetic() || matches!(c, '-' | '\'' | '’'))
}

/// A name of two to five words in a script without capitals (Arabic,
/// Hebrew, Thai), which tells a person's name from an organisation's no
/// other way: "امحمد عثمان فيدان".
fn uncased_name(name: &str) -> bool {
    let parts: Vec<&str> = name.split_whitespace().collect();
    (2..=5).contains(&parts.len())
        && parts.iter().all(|w| {
            w.chars()
                .all(|c| c.is_alphabetic() && !c.is_uppercase() && !c.is_lowercase())
        })
}

/// Every word of every name on a byline, normalised.
pub(super) fn name_words(authors: &[Person]) -> Vec<String> {
    authors
        .iter()
        .flat_map(|p| [&p.family, &p.given, &p.literal])
        .flatten()
        .flat_map(|n| words(n))
        .collect()
}

/// The same name, give or take a letter (two in a long name):
/// transliteration ("schoelkopf" for "scholkopf"), a typo.
pub(super) fn near(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let allowed = match a.chars().count().min(b.chars().count()) {
        0..=3 => 0,
        4..=7 => 1,
        _ => 2,
    };
    allowed > 0 && edit_distance(a, b) <= allowed
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let substituted = diagonal + usize::from(ca != *cb);
            diagonal = row[j + 1];
            row[j + 1] = substituted.min(row[j] + 1).min(row[j + 1] + 1);
        }
    }
    row[b.len()]
}

/// Whether claimed authors (structured) name someone not on the record's
/// byline: an invented co-author beside a real first author. "Others", "et
/// al." and organisations aren't checked.
pub(super) fn claimed_strangers(claimed: &[String], authors: &[Person]) -> bool {
    let names = name_words(authors);
    claimed.iter().any(|name| {
        let Some(family) = claimed_family(name) else {
            return false;
        };
        let parts: Vec<&str> = family.split(' ').filter(|w| !is_particle(w)).collect();
        let placeholder = matches!(family.as_str(), "al" | "others" | "etal");
        !placeholder
            && !parts.iter().any(|w| is_body_word(w))
            && !parts.iter().all(|w| names.iter().any(|n| near(n, w)))
            && !shortened(name, &parts, authors)
    })
}

/// Whether a claimed double-barrelled name is an author's that the record
/// gives shorter ("Hirschi-Santelmo, Sandrine" for "Sandrine Hirschi"):
/// one of its parts is that author's family name, and the given name's
/// initial agrees.
fn shortened(name: &str, parts: &[&str], authors: &[Person]) -> bool {
    let Some((_, given)) = name.split_once(',') else {
        return false;
    };
    let Some(initial) = given
        .trim()
        .chars()
        .next()
        .and_then(|c| c.to_uppercase().next())
    else {
        return false;
    };
    parts.len() > 1
        && authors.iter().any(|p| {
            p.parts().is_some_and(|(family, _)| {
                let family = normalise(&family);
                parts.iter().any(|w| near(w, &family))
            }) && p.initials().is_some_and(|i| i.starts_with(initial))
        })
}

/// Whether a claimed author is an organisation ("Committee on Infectious
/// Diseases", "EVER-ILD investigators"): neither found nor missed among a
/// record's people.
pub(super) fn is_body(name: &str) -> bool {
    words(name).iter().any(|w| is_body_word(w))
}

/// Whether a reference's byline (its text before the title) names someone
/// not on the record's: a capitalised name word that is none of the
/// record's names (give or take a letter), nor a given name written out
/// beside its family name where the record has only initials, nor the
/// journal's or a reference's furniture. A byline naming an organisation
/// ("ATLAS Collaboration", "GBD 2015 … Collaborators") isn't checked; nor
/// initials and acronyms ("RS", "WHO"), which say too little. Prose ends
/// the byline at its first lowercase word ("Huang et al. described in The
/// Lancet").
pub(super) fn byline_strangers(byline: &str, authors: &[Person], journal: Option<&str>) -> bool {
    let mut tokens: Vec<(String, &str)> = Vec::new();
    let mut spans = Vec::new();
    for (word, span) in spanned_words(byline) {
        let raw = &byline[span.clone()];
        if raw.chars().next().is_some_and(char::is_lowercase)
            && !BYLINE_WORDS.contains(&word.as_str())
            && !is_particle(&word)
        {
            break;
        }
        tokens.push((word, raw));
        spans.push(span);
    }
    // A family name, its initials after a comma ("Harrowgate, O.") or,
    // two or three run together, after a space ("Thalubrecht HL"; one
    // after a space may be a middle initial: "John A. Smith").
    let initialled = |i: usize| {
        tokens.get(i + 1).is_some_and(|(_, raw)| {
            let gap = &byline[spans[i].end..spans[i + 1].start];
            (gap.contains(',') && is_initials(raw))
                || (gap.trim().is_empty()
                    && (2..=3).contains(&raw.chars().count())
                    && raw.chars().all(char::is_uppercase))
        })
    };
    if tokens.iter().any(|(w, _)| is_body_word(w)) {
        return false;
    }
    let names = name_words(authors);
    let people: Vec<(Vec<String>, String)> = authors
        .iter()
        .filter_map(|p| {
            let (family, _) = p.parts()?;
            Some((
                words(&family),
                p.initials().unwrap_or_default().to_lowercase(),
            ))
        })
        .collect();
    let journal = journal.map(significant);
    // The initials of the record's author whose family name is at `i`.
    let initials_at = |i: usize| {
        people
            .iter()
            .find(|(family, _)| family.iter().any(|f| near(f, &tokens[i].0)))
            .map(|(_, initials)| initials)
    };
    tokens.iter().enumerate().any(|(i, (word, raw))| {
        let name_like = word.chars().count() >= 2
            && raw.chars().next().is_some_and(char::is_uppercase)
            && raw.chars().any(char::is_lowercase)
            && !word.chars().any(|c| c.is_ascii_digit());
        if !name_like
            || STOPWORDS.contains(&word.as_str())
            || BYLINE_WORDS.contains(&word.as_str())
            || NOT_NAMES.contains(&word.as_str())
            || is_particle(word)
            || is_month(word)
            || journal
                .as_ref()
                .is_some_and(|j| word.len() >= 3 && journal_word(word, j))
            || names.iter().any(|n| near(n, word))
        {
            return false;
        }
        // A given name written out ("Jumper, John"), within a word or two
        // of its family name; not a family name with initials of its own.
        let first = word.chars().next().unwrap_or(' ');
        let given = !initialled(i)
            && (i.saturating_sub(2)..(i + 3).min(tokens.len()))
                .filter(|&j| j != i)
                .any(|j| initials_at(j).is_some_and(|ini| ini.is_empty() || ini.contains(first)));
        !given
    })
}

/// A person's family name, normalised: the `family` field, or the last
/// word of a literal name.
pub(super) fn family_name(p: &Person) -> Option<String> {
    let name = match (&p.family, &p.literal) {
        (Some(family), _) => family.as_str(),
        // Its suffixes and degrees aside: "James M. Ritter DPhil FRCP" is
        // Ritter.
        (None, Some(literal)) => crate::work::without_post_nominals(literal),
        (None, None) => return None,
    };
    let last = if p.family.is_some() {
        normalise(name)
    } else if let Some((family, _)) = crate::work::inverted(name) {
        // Inverted: "Carvalho, Schubert Ribeiro de" (DataCite's).
        words(family).pop()?
    } else {
        words(name).pop()?
    };
    (!last.is_empty()).then_some(last)
}

/// The family name in a claimed author string: "Vaswani, A." → "vaswani",
/// "Ashish Vaswani" → "vaswani", "A. Vaswani" → "vaswani", "Daum RS" →
/// "daum", "A. van der Berg" → "van der berg".
pub(super) fn claimed_family(name: &str) -> Option<String> {
    let family = if let Some((before, _)) = name.split_once(',') {
        normalise(before)
    } else {
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
pub(super) fn same_family(a: &str, b: &str) -> bool {
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

/// Words a journal's name puts before its series' letter ("Physics
/// Letters B", "Physical Review D", "Journal of Materials Chemistry A",
/// "Proceedings of the Royal Society A", "Transportation Research Part
/// B"): with one capital after them, no family name and an initial.
const SERIES_STEMS: &[&str] = &[
    "letters",
    "letter",
    "review",
    "reviews",
    "physics",
    "physica",
    "chemistry",
    "society",
    "series",
    "section",
    "part",
    "transactions",
    "proceedings",
    "journal",
    "science",
    "sciences",
    "research",
    "communications",
    "acta",
    "annals",
    "bulletin",
    "materials",
    "engineering",
    "mathematics",
    "statistics",
];

/// Whether a reference names authors: "et al", or a family name followed
/// by initials ("Laan DV", "Smith, J."). A family name is never followed by
/// a full stop, so "Int. J. Care Injured" in a running header isn't one;
/// nor is a journal's word before its series' letter ("Physics Letters
/// B", [`SERIES_STEMS`]). One in capitals is set off as a name ("SMITH
/// JA,", "DUPONT, J."), not two words of a title in capitals ("WOMEN ON
/// ORAL").
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
        let capitals = !family.chars().any(char::is_lowercase);
        let series =
            initials.chars().count() == 1 && SERIES_STEMS.contains(&family.to_lowercase().as_str());
        !w[0].ends_with('.')
            && !series
            && family.chars().count() >= 2
            && family.chars().next().is_some_and(char::is_uppercase)
            && family
                .chars()
                .all(|c| c.is_alphabetic() || matches!(c, '-' | '\''))
            && (1..=3).contains(&initials.chars().count())
            && initials.chars().all(char::is_uppercase)
            && (!capitals || w[1].ends_with(['.', ',', ';']))
    })
}

#[cfg(test)]
// Exact constants: scores and spends rounded or summed from binary-exact values.
#[allow(clippy::float_cmp)]
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

    #[test]
    fn an_inverted_literal_name_gives_its_family_name() {
        // holdout-2 #407, #408: DataCite's creators.
        for (name, family) in [
            ("Carvalho, Schubert Ribeiro de", "carvalho"),
            ("Lubrano, Emanuele", "lubrano"),
            ("Schubert Ribeiro de Carvalho", "carvalho"),
            // Open Library's, a degree after the comma: as before.
            ("Julio A., M.D. Ramirez", "ramirez"),
        ] {
            assert_eq!(
                family_name(&Person::literal(name)).as_deref(),
                Some(family),
                "{name}"
            );
        }
        // A family name in capitals doesn't make the record's names an
        // organisation's, and a double-barrelled name the record shortens is
        // the author's.
        let authors = [
            Person::literal("Hyuna Bae"),
            Person::literal("Seok-Bae LEE"),
            Person::literal("Sandrine Hirschi"),
        ];
        assert!(complete_byline(&authors));
        let claimed = [
            "Bae, H.".to_owned(),
            "Hirschi-Santelmo, Sandrine".to_owned(),
        ];
        assert!(!claimed_strangers(&claimed, &authors));
        let invented = ["Bae, H.".to_owned(), "Corvastine-Hirschi, Eamon".to_owned()];
        assert!(claimed_strangers(&invented, &authors));
        assert!(is_body("Committee on Infectious Diseases"));
    }
}
