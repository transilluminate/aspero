//! A BibTeX entry pasted as a reference: its fields, and its accents read as
//! letters.

/// BibTeX's accents and special letters as the letters they mark, the
/// accent dropped (names and titles are compared folded):
/// `Micha{\"e}l` is "Michael", `Veli{\v{c}}kovi{\'c}` "Velickovic",
/// `Tr{\k{e}}bacz` "Trebacz", `\ss` "ss", `\&` "&". Braces stay.
pub(crate) fn delatex(text: &str) -> String {
    const ACCENTS: &str = "\"'`^~=.vkcuHrdbt";
    const LETTERS: &[(&str, &str)] = &[
        ("ss", "ss"),
        ("aa", "a"),
        ("AA", "A"),
        ("ae", "ae"),
        ("AE", "AE"),
        ("oe", "oe"),
        ("OE", "OE"),
        ("o", "o"),
        ("O", "O"),
        ("l", "l"),
        ("L", "L"),
        ("i", "i"),
        ("j", "j"),
    ];
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        // A letter's group, its braces dropped too: `{\"e}`, `{\v{c}}`.
        if chars[i] == '{' && chars.get(i + 1) == Some(&'\\') {
            let mut depth = 0usize;
            let close = (i..chars.len().min(i + 12)).find(|&j| {
                match chars[j] {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    _ => {}
                }
                depth == 0
            });
            if let Some(close) = close {
                let inner: String = chars[i + 1..close].iter().collect();
                let letter = delatex(&inner);
                if !letter.contains(['\\', '{', '}']) && letter.chars().count() <= 2 {
                    out.push_str(&letter);
                    i = close + 1;
                    continue;
                }
            }
        }
        if chars[i] != '\\' || i + 1 >= chars.len() {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let next = chars[i + 1];
        let word: String = chars[i + 1..]
            .iter()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect();
        if let Some((_, letter)) = LETTERS.iter().find(|(name, _)| *name == word) {
            out.push_str(letter);
            i += 1 + word.len();
        } else if ACCENTS.contains(next) && (!next.is_ascii_alphabetic() || word.len() == 1) {
            // The accent, then its letter: bare or in braces.
            i += 2;
            while chars.get(i) == Some(&' ') {
                i += 1;
            }
            if chars.get(i) == Some(&'{') {
                i += 1;
                while let Some(&c) = chars.get(i) {
                    i += 1;
                    if c == '}' {
                        break;
                    }
                    out.push(c);
                }
            } else if let Some(&c) = chars.get(i) {
                out.push(c);
                i += 1;
            }
        } else if !next.is_ascii_alphabetic() {
            // An escaped character: `\&`, `\%`.
            out.push(next);
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// A field of a BibTeX entry pasted as a reference (`@article{key, journal
/// = {Nature}, …}`), its braces dropped and its accents read
/// ([`delatex`]): `None` if the text isn't one or lacks the field.
pub(crate) fn bibtex_field(text: &str, name: &str) -> Option<String> {
    let text = text.trim_start();
    if !text.starts_with('@') {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find(name) {
        let at = from + found;
        from = at + name.len();
        // The whole field name ("title", not "booktitle"), then "=".
        let whole = !lower[..at]
            .chars()
            .next_back()
            .is_some_and(char::is_alphanumeric);
        let Some(rest) = lower[from..].trim_start().strip_prefix('=') else {
            continue;
        };
        if !whole {
            continue;
        }
        let start = text.len() - rest.trim_start().len();
        let close = match text[start..].chars().next()? {
            '{' => '}',
            '"' => '"',
            _ => {
                // A bare number: `year = 2020`.
                let bare: String = text[start..]
                    .chars()
                    .take_while(char::is_ascii_alphanumeric)
                    .collect();
                return (!bare.is_empty()).then_some(bare);
            }
        };
        let mut depth = 0usize;
        let mut value = String::new();
        for c in delatex(&text[start + 1..]).chars() {
            match c {
                c if c == close && depth == 0 => {
                    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
                    return (!value.is_empty()).then_some(value);
                }
                '{' => depth += 1,
                '}' => depth = depth.saturating_sub(1),
                c => value.push(c),
            }
        }
        return None;
    }
    None
}
