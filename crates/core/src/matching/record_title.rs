//! A record's title as the work's own title, before it is compared: the
//! markup and the furniture a provider writes into it taken out. Tags
//! (`<em>…</em>`), math markup (`$$\sqrt{s}=13\,\text {Te}\text {V} $$`)
//! and a spacing overbar ("tt¯H") read as the letters they mark; an
//! edition (", 14th Edition"), a code in brackets ("(MPB-35)"), a
//! library's statement of who wrote it ("/ Elaine N. Marieb, Katja
//! Hoehn") and a book's series in brackets ("(Verso Classics, 10)") are no
//! part of the title, unless the citation gives them too.

use super::text::{STOPWORDS, words};

/// Ordinals an edition is written with ("Second Edition").
const ORDINALS: &[&str] = &[
    "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth", "tenth",
    "revised", "new", "updated", "expanded",
];

/// `title` as the work's own title, beside `cited` (the title a claim
/// gives, else its whole text): markup read as letters, and a record's
/// furniture that `cited` doesn't give dropped. `book` lets a bracketed
/// series go too: an article's title may end in a bracketed name of its
/// own ("… (Sepsis-3)").
pub fn record_title(title: &str, cited: &str, book: bool) -> String {
    // Markup the citation writes too ("$Q^2$") reads as it does.
    let marked = cited.contains('$') || cited.contains('\\');
    let title = untagged(title);
    let mut title = if marked {
        unbarred(&title)
    } else {
        unbarred(&demath(&title))
    };
    let cited = words(cited);
    let lacks = |extra: &str| {
        let extra: Vec<String> = words(extra)
            .into_iter()
            .filter(|w| !STOPWORDS.contains(&w.as_str()))
            .collect();
        !extra.is_empty() && !extra.iter().all(|w| cited.contains(w))
    };
    // Each extra is the title's tail: take them off from the end, in any
    // order ("…, 14th Edition (Series)").
    for _ in 0..3 {
        let Some(cut) = [
            edition(&title),
            responsibility(&title),
            bracket(&title, book),
        ]
        .into_iter()
        .flatten()
        .find(|&at| lacks(&title[at..])) else {
            break;
        };
        let head = title[..cut].trim_end_matches([' ', ',', ';', ':', '.', '-', '(', '[']);
        // Never the whole title.
        if words(head).iter().all(|w| STOPWORDS.contains(&w.as_str())) {
            break;
        }
        let keep = head.len();
        title.truncate(keep);
    }
    title
}

/// Math markup as the text it sets: `$…$` and `$$…$$` spans with their
/// commands read (`\text {Te}` "Te", `\sqrt` "√", `\bar{t}` "t", `\,` a
/// space), their braces and sub- and superscript marks dropped.
fn demath(title: &str) -> String {
    if !title.contains('$') && !title.contains('\\') {
        return title.to_owned();
    }
    let mut out = String::with_capacity(title.len());
    let mut chars = title.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                let mut name = String::new();
                while let Some(&n) = chars.peek().filter(|n| n.is_ascii_alphabetic()) {
                    name.push(n);
                    chars.next();
                }
                // Spaces after a command's name are no text (`\text {V}`).
                if !name.is_empty() {
                    while chars.next_if_eq(&' ').is_some() {}
                }
                match name.as_str() {
                    // A spacing command: `\,`, `\;`, `\ `.
                    "" => {
                        if chars
                            .next_if(|n| matches!(n, ',' | ';' | ':' | ' ' | '!'))
                            .is_some()
                        {
                            out.push(' ');
                        }
                    }
                    "sqrt" => out.push('√'),
                    "quad" | "qquad" => out.push(' '),
                    "pm" => out.push('±'),
                    "times" => out.push('×'),
                    "to" | "rightarrow" => out.push('→'),
                    // A command whose argument is its text (`\text`,
                    // `\mathrm`, `\bar`), or a Greek letter's name, kept
                    // as a word.
                    _ if GREEK.contains(&name.as_str()) => out.push_str(&name),
                    _ => {}
                }
            }
            '$' | '{' | '}' | '^' | '_' => {}
            '~' => out.push(' '),
            _ => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Inline formatting tags a provider keeps in a title: `Linda Hogan's
/// <em>People of the Whale</em>`, `<i>Drosophila</i>`, `H<sub>2</sub>O`.
fn untagged(title: &str) -> String {
    const TAGS: &[&str] = &[
        "i", "em", "b", "strong", "sup", "sub", "scp", "span", "u", "mml:math",
    ];
    if !title.contains('<') {
        return title.to_owned();
    }
    let mut out = String::with_capacity(title.len());
    let mut rest = title;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let tag = &rest[open..];
        let close = tag.find('>');
        let name = tag[1..close.unwrap_or(1).max(1)]
            .trim_start_matches('/')
            .split([' ', '/'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        match close {
            Some(end) if TAGS.contains(&name.as_str()) => rest = &tag[end + 1..],
            _ => {
                out.push('<');
                rest = &tag[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Greek letters' names, as math markup writes them (`\gamma`).
const GREEK: &[&str] = &[
    "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "kappa", "lambda", "mu",
    "nu", "xi", "pi", "rho", "sigma", "tau", "phi", "chi", "psi", "omega", "Gamma", "Delta",
    "Lambda", "Sigma", "Phi", "Psi", "Omega", "Upsilon", "upsilon",
];

/// A spacing overbar or overline between letters dropped: "tt¯H" (an
/// antiquark's bar, written after it) is "ttH", as cited.
fn unbarred(title: &str) -> String {
    let chars: Vec<char> = title.chars().collect();
    chars
        .iter()
        .enumerate()
        .filter(|&(i, c)| {
            !(matches!(c, '\u{00AF}' | '\u{203E}')
                && i > 0
                && chars[i - 1].is_alphanumeric()
                && chars.get(i + 1).is_some_and(|n| n.is_alphanumeric()))
        })
        .map(|(_, c)| *c)
        .collect()
}

/// Where an edition at the title's end begins: ", 14th Edition", " 2nd
/// ed.", "(Second edition)".
fn edition(title: &str) -> Option<usize> {
    let lower = title.to_lowercase();
    let trimmed = lower.trim_end_matches([' ', '.', ')']);
    let rest = trimmed
        .strip_suffix("edition")
        .or_else(|| trimmed.strip_suffix(" ed"))?
        .trim_end();
    let ordinal = rest.rsplit([' ', ',', '(']).next()?;
    let numbered = ordinal.len() > 2
        && ordinal[..ordinal.len() - 2]
            .chars()
            .all(|c| c.is_ascii_digit())
        && ["st", "nd", "rd", "th"]
            .iter()
            .any(|s| ordinal.ends_with(s));
    if !numbered && !ORDINALS.contains(&ordinal) {
        return None;
    }
    let at = rest.len() - ordinal.len();
    // Lowercasing kept byte offsets only if it changed no length; else
    // find the same place in the title as written.
    (lower.len() == title.len()).then_some(at)
}

/// Where a library's statement of who wrote or edited the work begins:
/// " / Elaine N. Marieb, Katja Hoehn", " / edited by J. Smith". Names
/// only after the slash: capitalised words, initials, joining words; and
/// shaped as names, with an initial, a comma between them or a "by"
/// ("Input / Output Systems" is a title).
fn responsibility(title: &str) -> Option<usize> {
    let at = title.rfind(" / ")?;
    let names = &title[at + 3..];
    let joining = [
        "and", "&", "et", "al", "al.", "by", "edited", "ed.", "eds.", "with", "von", "van", "de",
        "der", "den", "la", "le", "du", "di", "da",
    ];
    let tokens: Vec<&str> = names
        .split([' ', ','])
        .filter(|t| !t.is_empty() && *t != "[" && *t != "]")
        .collect();
    let named = tokens
        .iter()
        .filter(|t| t.starts_with(char::is_uppercase))
        .count();
    let shaped = names.contains(',')
        || tokens
            .iter()
            .any(|t| matches!(*t, "by" | "edited" | "ed." | "eds."))
        || tokens.iter().any(|t| {
            let t = t.trim_matches(['[', ']']);
            t.len() == 2 && t.ends_with('.') && t.starts_with(char::is_uppercase)
        });
    (named >= 1
        && shaped
        && tokens.len() <= 16
        && tokens.iter().all(|t| {
            let t = t.trim_matches(['[', ']', '.']);
            t.starts_with(char::is_uppercase) || joining.contains(&t) || t.is_empty()
        }))
    .then_some(at)
}

/// Where a bracketed group at a book's title's end begins, when it is a
/// code ("(MPB-35)": capitals and digits) or its series ("(Verso
/// Classics, 10)", "(Handbook of Oriental Studies/Handbuch Der
/// Orientalistik)"): two words or more. Not an article's: its title may
/// end in a bracketed name of its own ("… (Sepsis-3)", "(COVID-19)").
fn bracket(title: &str, book: bool) -> Option<usize> {
    let trimmed = title.trim_end();
    let (open, close) = match trimmed.chars().last()? {
        ')' => ('(', ')'),
        ']' => ('[', ']'),
        _ => return None,
    };
    let at = trimmed.rfind(open)?;
    let inner = &trimmed[at + 1..trimmed.len() - close.len_utf8()];
    if inner.contains(open) || inner.contains(close) || at == 0 {
        return None;
    }
    let code = !inner.contains(' ')
        && inner.chars().any(|c| c.is_ascii_digit())
        && inner.chars().any(|c| c.is_ascii_uppercase())
        && !inner.chars().any(char::is_lowercase);
    let series = words(inner).len() >= 2;
    (book && (code || series)).then_some(at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn math_markup_and_overbars_read_as_letters() {
        assert_eq!(
            record_title(
                "Combined measurements of Higgs boson couplings in proton–proton collisions at \
                 $$\\sqrt{s}=13\\,\\text {Te}\\text {V} $$",
                "",
                false
            ),
            "Combined measurements of Higgs boson couplings in proton–proton collisions at \
             √s=13 TeV"
        );
        assert_eq!(
            record_title("Observation of tt¯H Production", "", false),
            "Observation of ttH Production"
        );
        assert_eq!(
            record_title(
                "The Wall in Linda Hogan’s <em>People of the Whale</em>",
                "",
                false
            ),
            "The Wall in Linda Hogan’s People of the Whale"
        );
        assert_eq!(record_title("H<sub>2</sub>O < 3", "", false), "H2O < 3");
        // Markup the citation writes too stays.
        assert_eq!(
            record_title("at moderate $Q^2$", "Motyka L. … at moderate $Q^2$", false),
            "at moderate $Q^2$"
        );
        // A macron over a vowel is no overbar.
        assert_eq!(record_title("Tōkyō ¯ notes", "", false), "Tōkyō ¯ notes");
    }

    #[test]
    fn a_records_furniture_goes_unless_cited() {
        let goodman =
            "Goodman and Gilman's the Pharmacological Basis of Therapeutics, 14th Edition";
        assert_eq!(
            record_title(goodman, "Goodman & Gilman's ... | 14th (2023)", true),
            "Goodman and Gilman's the Pharmacological Basis of Therapeutics"
        );
        assert_eq!(
            record_title(goodman, "Goodman & Gilman's …, 14th edition", true),
            goodman
        );
        assert_eq!(
            record_title(
                "Complex population dynamics: A theoretical/empirical synthesis (MPB-35)",
                "Turchin, P. (2003). Complex Population Dynamics.",
                true
            ),
            "Complex population dynamics: A theoretical/empirical synthesis"
        );
        assert_eq!(
            record_title(
                "Human anatomy & physiology / Elaine N. Marieb, Katja Hoehn",
                "Marieb's Human Anatomy & Physiology",
                true
            ),
            "Human anatomy & physiology"
        );
        assert_eq!(
            record_title(
                "The Coming of the Book: The Impact of Printing 1450-1800 (Verso Classics, 10)",
                "The Coming of the Book. London: New Left Books, 1976.",
                true
            ),
            "The Coming of the Book: The Impact of Printing 1450-1800"
        );
        assert_eq!(
            record_title(
                "The Luwians (Handbook of Oriental Studies/Handbuch Der Orientalistik)",
                "Melchert, H. Craig, ed. The Luwians. Handbuch der Orientalistik I/68.",
                true
            ),
            "The Luwians"
        );
        // An article's own bracketed name stays; so does a series cited.
        let sepsis = "The Third International Consensus Definitions for Sepsis and Septic \
                      Shock (Sepsis-3)";
        assert_eq!(record_title(sepsis, "Singer M. JAMA 2016", false), sepsis);
        let covid = "Clinical features of patients with coronavirus (COVID-19)";
        assert_eq!(record_title(covid, "Huang C. Lancet 2020", false), covid);
        assert_eq!(
            record_title(
                "Hittite Myths (Second edition)",
                "Hittite Myths. 2nd ed.",
                true
            ),
            "Hittite Myths"
        );
        let series = "The Luwians (Handbook of Oriental Studies)";
        assert_eq!(
            record_title(series, "The Luwians. Handbook of Oriental Studies.", true),
            series
        );
        // A slash inside the title is no statement of who wrote it.
        let slash = "Input / Output Systems";
        assert_eq!(record_title(slash, "", true), slash);
    }
}
