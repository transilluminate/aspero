//! Acronyms in a cited title: written out as the title's words they stand
//! for ("SAEs" for "Sparse Autoencoders").

use super::text::{STOPWORDS, words};

/// A reference with the acronyms it writes for a run of `title`'s words
/// written out as those words ("Theory of Mind in LLMs" for "… in Large
/// Language Models", "RLHF" for "Reinforcement Learning from Human
/// Feedback", "ToM"): models abbreviate titles so. An acronym is two
/// capitals or more, a plural's "s" aside, spelling the initials of
/// consecutive words of the title, its joining words' optional, the last
/// a compound's two parts' ("SAEs" for "Sparse Autoencoders"); one the
/// title holds as a word stays. `None` when there is none.
pub fn expand_acronyms(text: &str, title: &str) -> Option<String> {
    /// Prefixes a compound word is made with, its second part a word.
    const COMPOUND_PREFIXES: &[&str] = &[
        "auto", "self", "multi", "hyper", "meta", "super", "micro", "nano", "inter", "cross",
        "over", "under", "back", "semi", "counter",
    ];
    // The run of title words from `at` whose initials spell `letters`.
    fn spelt(letters: &[char], words: &[String], at: usize) -> Option<usize> {
        let Some(&c) = letters.first() else {
            return Some(at);
        };
        let word = words.get(at)?;
        let joining = STOPWORDS.contains(&word.as_str());
        if word.starts_with(c)
            && let Some(end) = spelt(&letters[1..], words, at + 1)
        {
            return Some(end);
        }
        // A compound word may give the initials of both its parts, last
        // in the run ("Sparse Autoencoders" in "SAEs": auto-encoders), its
        // first part a prefix compounds are made with ("Attention" is no
        // compound for "GAT").
        if let [c, inner] = letters
            && COMPOUND_PREFIXES.iter().any(|p| {
                word.strip_prefix(p).is_some_and(|part| {
                    p.starts_with(*c) && part.len() >= 4 && part.starts_with(*inner)
                })
            })
        {
            return Some(at + 1);
        }
        // A joining word inside the run may give no letter ("Learning
        // from Human" in "RLHF").
        if joining && at > 0 {
            return spelt(letters, words, at + 1);
        }
        None
    }
    let title_words: Vec<&str> = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let lower: Vec<String> = title_words.iter().map(|w| w.to_lowercase()).collect();
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    let mut rest = text;
    let mut previous = "";
    while !rest.is_empty() {
        let start = rest.find(char::is_alphanumeric).unwrap_or(rest.len());
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let end = rest
            .find(|c: char| !c.is_alphanumeric())
            .unwrap_or(rest.len());
        let token = &rest[..end];
        rest = &rest[end..];
        let capitals = token.chars().filter(char::is_ascii_uppercase).count();
        let stem = token
            .strip_suffix('s')
            .filter(|_| token.len() > 2)
            .unwrap_or(token);
        let letters: Vec<char> = stem.to_lowercase().chars().collect();
        // A name's initials ("Kishore CP,") are no acronym.
        let initials = token.chars().all(|c| c.is_ascii_uppercase())
            && previous.starts_with(char::is_uppercase)
            && previous.chars().any(char::is_lowercase)
            && (rest.is_empty() || rest.starts_with([',', '.', ';']));
        let expansion = (capitals >= 2
            && !initials
            && (2..=8).contains(&letters.len())
            && token.chars().all(|c| c.is_ascii_alphabetic())
            && !lower
                .iter()
                .any(|w| w.eq_ignore_ascii_case(token) || *w == stem.to_lowercase()))
        .then(|| {
            (0..lower.len())
                .filter(|&i| !STOPWORDS.contains(&lower[i].as_str()))
                .find_map(|i| {
                    let end = spelt(&letters, &lower, i)?;
                    // A run ends on a word of its own, not a joining word.
                    (end - i >= 2 && !STOPWORDS.contains(&lower[end - 1].as_str()))
                        .then(|| title_words[i..end].join(" "))
                })
        })
        .flatten();
        // Nor one its words follow, written out ("MAE | Masked
        // Autoencoders Are …"): it defines them, and isn't more of them.
        let expansion = expansion.filter(|run| !defines(token, rest, run));
        match expansion {
            Some(words) => {
                out.push_str(&words);
                changed = true;
            }
            None => out.push_str(token),
        }
        previous = token;
    }
    changed.then_some(out)
}

/// Whether `acronym`, `rest` the text after it, is followed by the words
/// it stands for, `run` ("LATS: Language Agent Tree Search"): a definition.
fn defines(acronym: &str, rest: &str, run: &str) -> bool {
    let run = words(run);
    !acronym.is_empty() && !run.is_empty() && words(rest).starts_with(&run)
}
