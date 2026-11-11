//! Identifier mentions found in text.

use aspero_core::id::Mention;
use serde_json::{Value, json};

/// Characters of context shown either side of a mention in a long line.
const CONTEXT: usize = 60;

/// One mention: where it is, the identifier, and how it was written.
pub fn mention(file: &str, text: &str, m: &Mention) -> Value {
    let mut out = json!({
        "file": file,
        "line": m.line,
        "column": m.column,
        "id": m.id,
        "kind": m.id.kind().prefix(),
        "text": &text[m.span.clone()],
        "context": context(text, m),
    });
    // A DOI broken at a hyphen, read without it: to try if it isn't found.
    if let Some(alternative) = m.alternative(text) {
        out["alternative"] = json!(alternative);
    }
    out
}

/// The mention's line, trimmed, cut to a window around it when long.
fn context(text: &str, m: &Mention) -> String {
    let line_start = text[..m.span.start].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[m.span.end..]
        .find('\n')
        .map_or(text.len(), |i| m.span.end + i);
    let before: Vec<char> = text[line_start..m.span.start].chars().collect();
    let after: Vec<char> = text[m.span.end..line_end].chars().collect();
    let cut_before = before.len() > CONTEXT;
    let cut_after = after.len() > CONTEXT;
    let before: String = before[before.len().saturating_sub(CONTEXT)..]
        .iter()
        .collect();
    let after: String = after[..after.len().min(CONTEXT)].iter().collect();
    format!(
        "{}{}{}{}{}",
        if cut_before { "…" } else { "" },
        before.trim_start(),
        &text[m.span.clone()],
        after.trim_end(),
        if cut_after { "…" } else { "" },
    )
}

#[cfg(test)]
mod tests {
    use aspero_core::id::scan;

    use super::*;

    #[test]
    fn mentions_carry_where_and_how_they_were_written() {
        let text = "# Notes\n  PE trials: PEITHO (doi:10.1056/NEJMoa1302097).\r\n";
        let m = &scan(text)[0];
        assert_eq!(
            mention("vault/pe.md", text, m),
            json!({
                "file": "vault/pe.md",
                "line": 2,
                "column": 22,
                "id": "doi:10.1056/nejmoa1302097",
                "kind": "doi",
                "text": "doi:10.1056/NEJMoa1302097",
                "context": "PE trials: PEITHO (doi:10.1056/NEJMoa1302097).",
            })
        );
    }

    #[test]
    fn long_lines_are_cut_around_the_mention() {
        let text = format!("{} PMC6716493 {}", "a".repeat(100), "b".repeat(100));
        let m = &scan(&text)[0];
        let c = context(&text, m);
        assert!(c.starts_with('…') && c.ends_with('…'), "{c}");
        assert!(c.contains(" PMC6716493 "));
        assert_eq!(c.chars().count(), 2 * CONTEXT + "PMC6716493".len() + 2);
    }
}
