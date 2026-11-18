//! Comparing journal names: one name abbreviating another ("N Engl J Med"),
//! initials ("NEJM"), a journal clearly another, sister journals and sections.

use crate::matching::normalise;

/// Words a journal abbreviation leaves out.
pub(super) const JOURNAL_STOPWORDS: &[&str] = &[
    "the", "of", "and", "for", "in", "on", "de", "der", "des", "du", "la", "le", "und", "fur",
    "fuer", "et", "y", "di", "del", "della", "voor",
];

/// Whether two journal names are the same journal: equal, or one
/// abbreviates the other.
pub fn same_journal(a: &str, b: &str) -> bool {
    let (a, b) = (significant(a), significant(b));
    !a.is_empty() && (a == b || abbreviates(&a, &b) || abbreviates(&b, &a))
}

/// Whether `cited` is clearly another journal than `actual`, not just
/// named in a way [`same_journal`] doesn't see: neither name, in full or
/// abbreviated (spelling aside: "Anaesth" for "Anesthesia"), runs inside
/// the other ("Proc Natl Acad Sci U S A" ↔ "Proceedings of the National
/// Academy of Sciences", "BMJ" ↔ "BMJ (Clinical research ed.)"), nor
/// abbreviates the other's words in order, leaving some out ("Proc Biol
/// Sci" ↔ "Proceedings of the Royal Society B: Biological Sciences"), nor
/// is either the initials of a run of the other's words ("PNAS"). "Environ
/// Sci Technol" is not "Environment International". A sister journal is
/// another, though one name runs inside the other ([`sister_journal`]:
/// "JAMA" for JAMA Internal Medicine).
pub fn other_journal(cited: &str, actual: &str) -> bool {
    other_journal_as(cited, actual, false)
}

/// [`other_journal`] for a journal read from a reference's text, which
/// may be cut short ([`sister_journal_as`]).
pub(crate) fn other_journal_read(cited: &str, actual: &str) -> bool {
    other_journal_as(cited, actual, true)
}

fn other_journal_as(cited: &str, actual: &str, read: bool) -> bool {
    let (c, a) = (significant(cited), significant(actual));
    if c.is_empty() || a.is_empty() || same_journal(cited, actual) {
        return false;
    }
    if sister_journal_as(cited, actual, read) {
        return true;
    }
    let inside = |short: &[String], long: &[String]| {
        long.windows(short.len()).any(|run| {
            run.iter()
                .zip(short)
                .all(|(f, s)| loosely_abbreviates(s, f) || loosely_abbreviates(f, s))
        }) || abbreviates_in_order(short, long)
            || short.iter().any(|w| {
                (1..=long.len())
                    .flat_map(|n| long.windows(n))
                    .any(|run| run.len() > 1 && is_initials(w, run))
            })
    };
    !(inside(&c, &a) || inside(&a, &c))
}

/// Words a journal's name may carry that say where or how it's published,
/// not what it is: NLM's place qualifiers as Zotero writes them without
/// their brackets ("Lancet Lond Engl", "Proceedings … of the United
/// States of America"), an edition, and articles in other languages ("Das
/// Gesundheitswesen").
const QUALIFIER_WORDS: &[&str] = &[
    "london", "lond", "england", "engl", "united", "states", "america", "usa", "uk", "york",
    "online", "print", "ed", "edn", "edition", "das", "die", "les", "los", "las", "il", "lo", "el",
    "gli", "do", "da", "dos", "a", "an",
];

/// Words saying a subtitle describes the journal rather than naming a
/// subject of its own ("BJOG: An International Journal of Obstetrics &
/// Gynaecology", "Industrial Relations: A Journal of Economy and Society",
/// "Nihon yakurigaku zasshi. Folia pharmacologica Japonica").
const DESCRIPTIVE_WORDS: &[&str] = &[
    "journal",
    "journals",
    "international",
    "official",
    "revista",
    "revue",
    "zeitschrift",
    "rivista",
    "folia",
    "acta",
    "bulletin",
    "organ",
    "publication",
    "society",
    "association",
    "interdisciplinary",
];

/// Whether `cited` is a sister journal of `actual`'s, of the same family
/// and publisher: one name is the other's with subject words of its own
/// ("The Lancet" for The Lancet Oncology, "JAMA" for JAMA Internal
/// Medicine, "Nature" for Nature Medicine, "Diabetes" for Diabetes Care,
/// "Circulation" for "Circulation: Heart Failure"; "JCO Oncol Pract" for
/// the Journal of Clinical Oncology, "NEJM Evid" for the New England
/// Journal of Medicine). The words the two share abbreviate each other,
/// or are a name's initials ("JCO", "JACC"); what's left over on either
/// side tells the journal apart, unless it's what an acronym stands for
/// ("JAMA: The Journal of the American Medical Association", "MMWR Morb
/// Mortal Wkly Rep"), a series letter, a structural word ("Section",
/// "Royal Society"), a place, edition or article (`QUALIFIER_WORDS`),
/// in brackets ("BMJ (Clinical research ed.)"), or a subtitle that
/// describes the journal (`DESCRIPTIVE_WORDS`, or one after a series
/// letter: "Physical review. B, Condensed matter"). An abbreviation
/// leaving words out in order ("Proc Biol Sci" for "Proceedings of the
/// Royal Society B: Biological Sciences") is the same journal, not a
/// sister.
pub fn sister_journal(cited: &str, actual: &str) -> bool {
    sister_journal_as(cited, actual, false)
}

/// [`sister_journal`], with `read` for a journal read from a reference's
/// text: a reading may run on from the title's end ("… Smoker and in
/// COPD. PLOS One"), so any part of it after a stop that is the journal
/// is; and it may be cut short after a dotted abbreviation ("Theory" of
/// "IEEE Trans. Inform. Theory."), so the record's name ending in it is no
/// sister ("Energy" for Applied Energy goes untold).
pub(crate) fn sister_journal_as(cited: &str, actual: &str, read: bool) -> bool {
    let readings: Vec<&str> = if read {
        std::iter::once(cited)
            .chain(cited.match_indices(". ").map(|(i, _)| &cited[i + 2..]))
            .collect()
    } else {
        vec![cited]
    };
    readings.iter().all(|cited| {
        sister_names(cited, actual, read)
            && main_name(actual).is_none_or(|main| sister_names(cited, main, read))
            && main_name(cited).is_none_or(|main| sister_names(main, actual, read))
    })
}

/// A journal's name before a subtitle that describes it, if it has one:
/// "BJOG" of "BJOG: An International Journal of Obstetrics &
/// Gynaecology", "Physical review. B" of "Physical review. B, Condensed
/// matter".
fn main_name(name: &str) -> Option<&str> {
    let (at, rest) = [": ", " : ", ". ", ", "]
        .iter()
        .filter_map(|sep| name.find(sep).map(|i| (i, &name[i + sep.len()..])))
        .min_by_key(|(i, _)| *i)?;
    let series_letter = |s: &str| {
        let s = s.trim();
        s.chars().count() == 1 && s.chars().all(char::is_alphabetic)
    };
    // A series letter, then its subject: "Physical review. B, Condensed
    // matter", "Proceedings of the Royal Society B: Biological Sciences".
    let head_len = rest.find([',', ':']).unwrap_or(rest.len());
    if series_letter(&rest[..head_len]) {
        return Some(&name[..name.len() - rest.len() + head_len]);
    }
    if name[..at].rsplit(' ').next().is_some_and(series_letter) {
        return Some(&name[..at]);
    }
    // Abbreviated too: "Injury, Int. J. Care Injured".
    let descriptive = significant(rest).iter().any(|w| {
        DESCRIPTIVE_WORDS
            .iter()
            .any(|d| w == d || abbreviates_word(w, d))
    }) || normalise(rest)
        .split(' ')
        .next()
        .is_some_and(|w| matches!(w, "a" | "an" | "the"));
    descriptive.then(|| &name[..at])
}

/// [`sister_journal_as`] for two names as they stand.
fn sister_names(cited: &str, actual: &str, read: bool) -> bool {
    let unbracketed = |name: &str| {
        let mut depth = 0usize;
        name.chars()
            .filter(|&c| {
                match c {
                    '(' | '[' => depth += 1,
                    ')' | ']' => depth = depth.saturating_sub(1),
                    _ => return depth == 0,
                }
                false
            })
            .collect::<String>()
    };
    let (c, a) = (
        significant(&unbracketed(cited)),
        significant(&unbracketed(actual)),
    );
    if c.is_empty() || a.is_empty() || c == a || abbreviates(&c, &a) || abbreviates(&a, &c) {
        return false;
    }
    let pair = |x: &str, y: &str| loosely_abbreviates(x, y) || loosely_abbreviates(y, x);
    let shared_in = |name: &[String], other: &[String]| -> Vec<usize> {
        (0..name.len())
            .filter(|&i| {
                other.iter().any(|o| pair(&name[i], o))
                    || initials_of_run(&name[i], other).is_some()
                    || other
                        .iter()
                        .filter_map(|o| initials_of_run(o, name))
                        .any(|run| run.contains(&i))
            })
            .collect()
    };
    let shared = shared_in(&a, &c);
    if shared.is_empty() {
        return false;
    }
    let (own_c, own_a) = (distinguishing(&c, &a), distinguishing(&a, &c));
    // Read from text, the cited name may be the record's cut short: the
    // record's own words all before the words they share.
    if read && own_c.is_empty() && own_a.iter().all(|&i| i < shared[0]) {
        return false;
    }
    !(own_c.is_empty() && own_a.is_empty())
}

/// Words naming a section a journal may number in its own volumes.
const SECTION_WORDS: &[&str] = &[
    "letters",
    "letter",
    "lett",
    "supplement",
    "supplements",
    "suppl",
    "supp",
];

/// Whether `cited` names a section of `actual`: its name with section
/// words only ([`SECTION_WORDS`]: "The Astrophysical Journal Letters" of
/// The Astrophysical Journal, whose letters its volumes numbered until
/// 2010). Only at the record's own volume and page is it the journal.
pub(crate) fn section_of(cited: &str, actual: &str) -> bool {
    let (c, a) = (significant(cited), significant(actual));
    let own_c = distinguishing(&c, &a);
    !own_c.is_empty()
        && distinguishing(&a, &c).is_empty()
        && own_c
            .iter()
            .all(|&i| SECTION_WORDS.contains(&c[i].as_str()))
}

/// The run of `full`'s words whose initials `word` is ("jco" in "journal
/// clinical oncology"), as a range, if any.
fn initials_of_run(word: &str, full: &[String]) -> Option<std::ops::Range<usize>> {
    let n = word.chars().count();
    (n > 1 && n <= full.len())
        .then(|| (0..=full.len() - n).find(|&i| is_initials(word, &full[i..i + n])))
        .flatten()
        .map(|i| i..i + n)
}

/// The positions of `name`'s subject words of its own beside `other` (both
/// [`significant`]): words that neither abbreviate nor are abbreviated by
/// any of `other`'s, aren't in a run whose initials are one of `other`'s
/// words (or of its own: "JAMA" beside "Journal of the American Medical
/// Association"), and are no initials of a run of `other`'s; a series
/// letter, structural word or qualifier is none.
fn distinguishing(name: &[String], other: &[String]) -> Vec<usize> {
    let pair = |x: &str, y: &str| loosely_abbreviates(x, y) || loosely_abbreviates(y, x);
    let spelt_out: Vec<std::ops::Range<usize>> = other
        .iter()
        .chain(name)
        .filter_map(|w| initials_of_run(w, name))
        .collect();
    // Two words run together on one side ("Cardiothorac" for "Cardio-
    // Thoracic").
    let joined = |i: usize| {
        [i.checked_sub(1).map(|p| (p, i)), Some((i, i + 1))]
            .into_iter()
            .flatten()
            .filter(|&(_, y)| y < name.len())
            .any(|(x, y)| {
                let both = format!("{}{}", name[x], name[y]);
                other
                    .iter()
                    .any(|o| o.len() > name[x].len() && loosely_abbreviates(o, &both))
            })
    };
    (0..name.len())
        .filter(|&i| {
            let w = &name[i];
            w.chars().count() > 1
                && !w.chars().all(|c| c.is_ascii_digit())
                && !STRUCTURAL_WORDS.contains(&w.as_str())
                && !QUALIFIER_WORDS.contains(&w.as_str())
                && !other.iter().any(|o| pair(w, o))
                && initials_of_run(w, other).is_none()
                && !spelt_out.iter().any(|run| run.contains(&i))
                && !joined(i)
        })
        .collect()
}

/// Words of a full name an abbreviation may leave out, besides stopwords
/// and series letters: its structure, not its subject ("Am J Med" is not
/// the American Journal of Emergency Medicine).
const STRUCTURAL_WORDS: &[&str] = &["section", "part", "series", "royal", "society"];

/// Whether each of `short`'s words abbreviates one of `full`'s, in order,
/// with structural words of `full`'s left out between ("Acta Crystallogr
/// D" for "Acta Crystallographica Section D", "Proc Biol Sci" for
/// "Proceedings of the Royal Society B: Biological Sciences") or two run
/// together ("Cardiothorac" for "Cardio-Thoracic"). A long abbreviation
/// may carry a word the full name doesn't ("Lond" in "Philos Trans R Soc
/// Lond B Biol Sci"): one, among four or more that agree.
fn abbreviates_in_order(short: &[String], full: &[String]) -> bool {
    let structural = |w: &String| w.chars().count() == 1 || STRUCTURAL_WORDS.contains(&w.as_str());
    let mut rest = full;
    let mut missed = 0;
    for s in short {
        // As far as the first word that can't be left out.
        let reach = rest
            .iter()
            .position(|f| !structural(f))
            .map_or(rest.len(), |k| k + 1);
        let next = (0..reach).find_map(|i| {
            let joined = rest.get(i + 1).map(|next| format!("{}{next}", rest[i]));
            if loosely_abbreviates(s, &rest[i]) {
                Some(i + 1)
            } else {
                joined
                    .is_some_and(|j| loosely_abbreviates(s, &j))
                    .then_some(i + 2)
            }
        });
        match next {
            Some(next) => rest = &rest[next..],
            None => missed += 1,
        }
    }
    missed == 0 || (missed == 1 && short.len() > 4)
}

/// [`abbreviates_word`], or the same with vowels dropped after the first
/// letter, for spellings that differ ("anaesth" ↔ "anesthesia",
/// "paediatr" ↔ "pediatrics"): for four letters or more, so that a short
/// word ("Gut") isn't any word with its consonants ("Gastroenterology").
fn loosely_abbreviates(short: &str, full: &str) -> bool {
    if short.chars().count() < 4 {
        return abbreviates_word(short, full);
    }
    let skeleton = |w: &str| -> String {
        let mut chars = w.chars();
        chars
            .next()
            .into_iter()
            .chain(chars.filter(|c| !"aeiouy".contains(*c)))
            .collect()
    };
    abbreviates_word(short, full) || abbreviates_word(&skeleton(short), &skeleton(full))
}

/// Whether `journal`, in full or abbreviated, appears in `words` (a
/// normalised reference, split into words). Not as a reference's other
/// words may: a one-word journal is named in full or by a prefix ("Circ"),
/// not any word with its letters ("mice" for "Medicine"), and a name's
/// initials ("SR", "N") are not a journal's.
pub(crate) fn journal_in(words: &[String], journal: &str) -> bool {
    let full = significant(journal);
    if full.is_empty() {
        return false;
    }
    let text: Vec<&str> = words
        .iter()
        .map(String::as_str)
        .filter(|w| !JOURNAL_STOPWORDS.contains(w))
        .collect();
    if let [one] = &full[..] {
        return text.iter().any(|w| w.len() >= 3 && one.starts_with(w));
    }
    text.windows(full.len()).any(|run| {
        run.iter().any(|w| w.len() > 1)
            && run.iter().zip(&full).all(|(s, f)| abbreviates_word(s, f))
    }) || text.iter().any(|w| w.len() > 2 && is_initials(w, &full))
}

/// Whether `word` (normalised) is one of a journal's significant words
/// (from [`significant`]), or abbreviates one, or is its initials ("res"
/// or "erj" in "ERJ Open Research").
pub(crate) fn journal_word(word: &str, full: &[String]) -> bool {
    full.iter().any(|f| abbreviates_word(word, f)) || (full.len() > 1 && is_initials(word, full))
}

/// A journal name's words that tell it apart, normalised.
/// The article "A" inside a name ("CA: A Cancer Journal for Clinicians",
/// abbreviated "CA Cancer J Clin") is left out too; a series letter
/// ("Physical Review A", "Acta Crystallographica Section A: …") isn't.
pub(crate) fn significant(name: &str) -> Vec<String> {
    let norm = normalise(name);
    let words: Vec<&str> = norm.split(' ').filter(|w| !w.is_empty()).collect();
    let article = |i: usize| {
        words[i] == "a"
            && i + 1 < words.len()
            && !(i > 0 && STRUCTURAL_WORDS.contains(&words[i - 1]))
    };
    words
        .iter()
        .enumerate()
        .filter(|&(i, w)| !JOURNAL_STOPWORDS.contains(w) && !article(i))
        .map(|(_, w)| (*w).to_owned())
        .collect()
}

/// `short` abbreviates `full` word by word, or is its initials.
fn abbreviates(short: &[String], full: &[String]) -> bool {
    match short {
        [one] if full.len() > 1 => is_initials(one, full),
        _ => {
            short.len() == full.len() && short.iter().zip(full).all(|(s, f)| abbreviates_word(s, f))
        }
    }
}

/// "med" for "medicine", "natl" for "national": same first letter, the
/// rest in order.
fn abbreviates_word(short: &str, full: &str) -> bool {
    let mut full_chars = full.chars();
    short.chars().next() == full.chars().next() && short.chars().all(|c| full_chars.any(|f| f == c))
}

fn is_initials(word: &str, full: &[String]) -> bool {
    word.chars().count() == full.len() && word.chars().zip(full).all(|(c, f)| f.starts_with(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journals_match_their_abbreviations_and_initials() {
        for (a, b) in [
            ("N Engl J Med", "New England Journal of Medicine"),
            ("N. Engl. J. Med.", "The New England journal of medicine"),
            ("Lancet", "The Lancet"),
            ("NEJM", "New England Journal of Medicine"),
            ("JAMA", "Journal of the American Medical Association"),
            (
                "Am J Respir Crit Care Med",
                "American Journal of Respiratory and Critical Care Medicine",
            ),
            (
                "Proc Natl Acad Sci U S A",
                "Proceedings of the National Academy of Sciences of the United States of America",
            ),
            (
                "Cochrane Database Syst Rev",
                "Cochrane Database of Systematic Reviews",
            ),
            ("Circulation", "Circulation"),
            // The article "A" inside a name (app pre-test, #102).
            ("CA Cancer J Clin", "CA: A Cancer Journal for Clinicians"),
            ("CA Cancer J Clin", "CA A Cancer Journal for Clinicians"),
            ("Phys Rev A", "Physical Review A"),
        ] {
            assert!(same_journal(a, b), "{a} ↔ {b}");
        }
        for (a, b) in [
            ("Lancet", "New England Journal of Medicine"),
            ("N Engl J Med", "Nature Medicine"),
            ("Ann Emerg Med", "Annals of Internal Medicine"),
            ("JAMA", "JAMA Internal Medicine"),
        ] {
            assert!(!same_journal(a, b), "{a} ≠ {b}");
        }
    }

    #[test]
    fn journals_are_found_inside_references() {
        let words =
            |s: &str| -> Vec<String> { normalise(s).split(' ').map(str::to_owned).collect() };
        let text = words("Daum RS, et al. N Engl J Med. 2017;376(26):2545-55.");
        assert!(journal_in(&text, "New England Journal of Medicine"));
        assert!(!journal_in(&text, "The Lancet"));
        assert!(journal_in(&words("Lancet 2010;376:23"), "The Lancet"));
        assert!(journal_in(&words("Circ 2010;121:23"), "Circulation"));
        assert!(journal_in(&words("Smith J. NEJM 2010"), "N Engl J Med"));
        // Names, initials and title words are not the journal.
        for (text, journal) in [
            ("Ragusa A, Rossi SR. Title. 2021", "Scientific Reports"),
            ("Doe J. Sepsis in mice. 2020", "Medicine"),
            ("Smith N. Title. 2020", "Nature"),
            ("Smith J. A. Title. 2020", "Journal of Anaesthesia"),
        ] {
            assert!(!journal_in(&words(text), journal), "{text}: {journal}");
        }
    }

    #[test]
    fn another_journal_is_clearly_another() {
        for (cited, actual) in [
            ("Environ Sci Technol", "Environment International"),
            (
                "Environmental Science & Technology",
                "Environment International",
            ),
            ("Lancet", "New England Journal of Medicine"),
            ("N Engl J Med", "Nature Medicine"),
            ("Ann Intern Med", "Annals of Emergency Medicine"),
            // Sister journals: the full name has a subject word the
            // abbreviation lacks.
            ("Am J Med", "American Journal of Emergency Medicine"),
            ("Ann Surg", "Annals of Thoracic Surgery"),
            // A series letter is not an article.
            ("Phys Rev A", "Physical Review B"),
            (
                "J Am Coll Cardiol",
                "Journal of the American College of Surgeons",
            ),
            // Sister journals of one family: a name inside the other with
            // subject words of its own (holdout-4's `w:sister-journal`).
            // Until round 5 "not clearly another".
            ("Lancet Infect Dis", "The Lancet"),
            ("The Lancet", "The Lancet Oncology"),
            ("Lancet", "The Lancet Infectious Diseases"),
            ("JAMA", "JAMA Internal Medicine"),
            ("JAMA Intern Med", "JAMA"),
            ("Nature", "Nature Medicine"),
            ("Cell", "Cell Reports"),
            ("BMJ", "BMJ Open"),
            ("Diabetes", "Diabetes Care"),
            ("Energy", "Applied Energy"),
            ("Pediatric Critical Care Medicine", "Critical Care Medicine"),
            ("JCO Oncol Pract", "Journal of Clinical Oncology"),
            ("NEJM Evid", "New England Journal of Medicine"),
            (
                "JACC Heart Fail",
                "Journal of the American College of Cardiology",
            ),
            ("JACS Au", "Journal of the American Chemical Society"),
            ("AJPM Focus", "American Journal of Preventive Medicine"),
            (
                "International Journal of Nursing Studies Advances",
                "International Journal of Nursing Studies",
            ),
            ("Circulation", "Circulation: Heart Failure"),
            // A word with a name's consonants isn't its abbreviation.
            ("BMJ Open Gastroenterology", "Gut"),
        ] {
            assert!(other_journal(cited, actual), "{cited} ≠ {actual}");
        }
        for (cited, actual) in [
            ("Environ Int", "Environment International"),
            ("N Engl J Med", "New England Journal of Medicine"),
            ("New England Journal of Medicine", "N Engl J Med"),
            ("Lancet", "The Lancet"),
            ("The Lancet", "Lancet"),
            ("Br Med J", "BMJ"),
            ("BMJ", "BMJ (Clinical research ed.)"),
            (
                "Proc Natl Acad Sci U S A",
                "Proceedings of the National Academy of Sciences",
            ),
            (
                "PNAS",
                "Proceedings of the National Academy of Sciences of the United States of America",
            ),
            (
                "MMWR Morb Mortal Wkly Rep",
                "Morbidity and Mortality Weekly Report",
            ),
            ("Can J Anaesth", "Canadian Journal of Anesthesia"),
            ("Z Gastroenterol", "Zeitschrift für Gastroenterologie"),
            // NLM abbreviations that leave words out, add one ("Lond") or
            // run two together.
            (
                "Proc Biol Sci",
                "Proceedings of the Royal Society B: Biological Sciences",
            ),
            (
                "Philos Trans R Soc Lond B Biol Sci",
                "Philosophical Transactions of the Royal Society B: Biological Sciences",
            ),
            (
                "Acta Crystallogr D Biol Crystallogr",
                "Acta Crystallographica Section D Biological Crystallography",
            ),
            (
                "Eur J Cardiothorac Surg",
                "European Journal of Cardio-Thoracic Surgery",
            ),
            // What an acronym stands for, a place, a bracketed qualifier.
            (
                "JAMA",
                "JAMA: The Journal of the American Medical Association",
            ),
            (
                "JAMA: The Journal of the American Medical Association",
                "JAMA",
            ),
            ("Lancet Lond Engl", "The Lancet"),
            ("Science", "Science (New York, N.Y.)"),
            ("Lancet Infect Dis", "The Lancet Infectious Diseases"),
            ("", "The Lancet"),
        ] {
            assert!(!other_journal(cited, actual), "{cited} ~ {actual}");
            assert!(!sister_journal(cited, actual), "{cited} ~ {actual}");
        }
    }
}
