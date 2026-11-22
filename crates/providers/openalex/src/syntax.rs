//! Citation text as OpenAlex search words, never syntax.
//!
//! OpenAlex reads `search=` and the `.search` filters as boolean queries
//! (help.openalex.org, "Search entities", 2026-11): `AND`, `OR` and `NOT`
//! in upper case are operators, quotes make a phrase and parentheses
//! group. A `|` is read as an OR too: one with an empty side is refused
//! (HTTP 400, "Search value contains an empty OR operand", a table row's
//! `… | 10.1136/… | 27660368`, checked live 2026-11-22). A minus doesn't
//! exclude, and wildcards need the exact search, so neither is touched.
//!
//! A query tree's operators are its structure; its leaves go through
//! [`crate::tree`]'s own `text_value`, which drops every syntax character.

/// `text` (a title, author, journal or keywords read from a citation) with
/// OpenAlex's syntax made plain: `|` a space, a standalone `AND`, `OR` or
/// `NOT` lower-cased (a word, then: "NOT ALL SEPSIS IS THE SAME" finds the
/// paper, not 25,701 works without "all sepsis is the same"), and an
/// unmatched `"` or bracket dropped, as are empty pairs. Balanced quotes
/// and brackets stay: a quoted phrase is the citation's own, and words in
/// brackets are all searched for, like the rest. Unchanged text is
/// returned as it is.
pub(crate) fn plain(text: &str) -> String {
    let mut chars: Vec<Option<char>> = text
        .chars()
        .map(|c| Some(if c == '|' { ' ' } else { c }))
        .collect();
    drop_unmatched(&mut chars);
    drop_empty_pairs(&mut chars);
    let kept: String = chars.into_iter().flatten().collect();
    let words: Vec<String> = kept.split_whitespace().map(plain_word).collect();
    let out = words.join(" ");
    // Only text that changed is respaced: the rest is sent as it was.
    if out.split_whitespace().eq(text.split_whitespace()) {
        text.to_owned()
    } else {
        out
    }
}

/// A word lower-cased when it is an operator (beside any quotes or
/// brackets: `(NOT`, `"OR"`).
fn plain_word(word: &str) -> String {
    match word.trim_matches(['"', '(', ')']) {
        "AND" | "OR" | "NOT" => word.to_lowercase(),
        _ => word.to_owned(),
    }
}

/// Drops each `(` never closed and `)` never opened, and the last `"` of
/// an odd number.
fn drop_unmatched(chars: &mut [Option<char>]) {
    let mut open = Vec::new();
    for (i, c) in chars.iter_mut().enumerate() {
        match *c {
            Some('(') => open.push(i),
            Some(')') if open.pop().is_none() => *c = None,
            _ => {}
        }
    }
    for i in open {
        chars[i] = None;
    }
    let quotes: Vec<usize> = (0..chars.len())
        .filter(|&i| chars[i] == Some('"'))
        .collect();
    if quotes.len() % 2 == 1 {
        chars[*quotes.last().expect("an odd count")] = None;
    }
}

/// Drops `()` and `""` with nothing but spaces inside, until none is left:
/// an empty group or phrase may be refused, as an empty OR side is.
fn drop_empty_pairs(chars: &mut [Option<char>]) {
    loop {
        let mut changed = false;
        let mut quote_open: Option<usize> = None;
        let mut last_open: Option<usize> = None;
        for i in 0..chars.len() {
            let Some(c) = chars[i] else { continue };
            match c {
                '"' => match quote_open.take() {
                    Some(j) => {
                        if empty_between(chars, j, i) {
                            chars[j] = None;
                            chars[i] = None;
                            changed = true;
                        }
                    }
                    None => quote_open = Some(i),
                },
                '(' => last_open = Some(i),
                ')' => {
                    if let Some(j) = last_open.take()
                        && empty_between(chars, j, i)
                    {
                        chars[j] = None;
                        chars[i] = None;
                        changed = true;
                    }
                }
                c if !c.is_whitespace() => last_open = None,
                _ => {}
            }
        }
        if !changed {
            return;
        }
    }
}

fn empty_between(chars: &[Option<char>], from: usize, to: usize) -> bool {
    chars[from + 1..to]
        .iter()
        .flatten()
        .all(|c| c.is_whitespace())
}

#[cfg(test)]
mod tests {
    use super::plain;

    #[test]
    fn operators_become_words() {
        assert_eq!(
            plain("NOT ALL SEPSIS IS THE SAME"),
            "not ALL SEPSIS IS THE SAME"
        );
        assert_eq!(
            plain("RISK OR BENEFIT OF ASPIRIN"),
            "RISK or BENEFIT OF ASPIRIN"
        );
        assert_eq!(plain("DIET AND EXERCISE"), "DIET and EXERCISE");
        // Beside quotes and brackets too; inside other words, never.
        assert_eq!(
            plain(r#"Sepsis ("NOT the same") (OR more)"#),
            r#"Sepsis ("not the same") (or more)"#
        );
        assert_eq!(
            plain("NOTCH signalling ANDROGEN"),
            "NOTCH signalling ANDROGEN"
        );
    }

    #[test]
    fn pipes_become_spaces() {
        assert_eq!(
            plain("Exercise and knee pain | 10.1136/bjsports-2016-096130 | 27660368"),
            "Exercise and knee pain 10.1136/bjsports-2016-096130 27660368"
        );
        assert_eq!(plain("a||b|"), "a b");
    }

    #[test]
    fn unbalanced_quotes_and_brackets_are_dropped() {
        assert_eq!(
            plain(r#"The "fourth wave of obesity"#),
            "The fourth wave of obesity"
        );
        assert_eq!(plain(r#"The "X" effect, "Y"#), r#"The "X" effect, Y"#);
        assert_eq!(plain("Lipoprotein(a) (a review"), "Lipoprotein(a) a review");
        assert_eq!(plain("a) b (c"), "a b c");
        assert_eq!(plain("a ( ) b \"\" c (( ))"), "a b c");
    }

    #[test]
    fn plain_text_is_sent_as_it_was() {
        for text in [
            "Deep learning",
            r#"The "obesity paradox" (a review)"#,
            "Smith,  J.  (2020)",
            "",
        ] {
            assert_eq!(plain(text), text);
        }
    }
}
