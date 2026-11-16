//! `verify --report`: a link to a new GitHub issue, the wrong-verdict form
//! (`.github/ISSUE_TEMPLATE/wrong-verdict.yml`) filled in from a result.
//! Nothing is sent anywhere: the link is printed, for the user to open or
//! not.

use std::fmt::Write as _;

use serde_json::Value;

/// Where new issues are opened, and the form's file there.
const NEW_ISSUE: &str = "https://github.com/transilluminate/aspero/issues/new";
const TEMPLATE: &str = "wrong-verdict.yml";

/// The longest link made. Browsers and GitHub take about 8,000
/// characters; the output is shortened to fit.
pub const MAX_URL: usize = 7000;
/// Most of the citation's characters in the issue title.
const TITLE_CHARS: usize = 80;
/// Most of the link the citation may take, encoded: the rest is the
/// output's.
const CITATION_MAX: usize = 2000;
/// What ends a value shortened to fit.
const TRUNCATED: &str = "\n… (truncated)";

/// The new-issue link for one `verify` result (its JSON, which echoes the
/// `input` and gives the `verdict`): the title, then each of the form's
/// fields by id (`citation`, `verdict`, `version`, `surface`, and last
/// `output`, the JSON, as much as fits under [`MAX_URL`]).
pub fn issue_url(item: &Value) -> String {
    let citation = item["input"].as_str().unwrap_or_default();
    let verdict = item["verdict"].as_str().unwrap_or_default();
    let title = format!(
        "Wrong verdict: {verdict} for \"{}\"",
        shorten(citation, TITLE_CHARS)
    );
    let mut url = format!("{NEW_ISSUE}?template={TEMPLATE}");
    for (key, value) in [
        ("title", encode(&title)),
        ("citation", encode_within(citation, CITATION_MAX)),
        ("verdict", encode(verdict)),
        (
            "version",
            encode(concat!("aspero ", env!("CARGO_PKG_VERSION"))),
        ),
        ("surface", encode("CLI")),
    ] {
        let _ = write!(url, "&{key}={value}");
    }
    url.push_str("&output=");
    let room = MAX_URL.saturating_sub(url.len());
    // Pretty when it fits, compact when that does, else compact and cut.
    let pretty = serde_json::to_string_pretty(item).expect("JSON values serialise");
    let output = if encoded_len(&pretty) <= room {
        encode(&pretty)
    } else {
        encode_within(&item.to_string(), room)
    };
    url.push_str(&output);
    url
}

/// `s` percent-encoded as a query value: every byte but the unreserved
/// (`A–Z a–z 0–9 - . _ ~`) as `%XX` of its UTF-8.
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if unreserved(b) {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

fn unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')
}

fn encoded_len(s: &str) -> usize {
    s.bytes().map(|b| if unreserved(b) { 1 } else { 3 }).sum()
}

/// `s` encoded, at most `max` long: whole characters, then [`TRUNCATED`],
/// when all of it doesn't fit.
fn encode_within(s: &str, max: usize) -> String {
    if encoded_len(s) <= max {
        return encode(s);
    }
    let room = max.saturating_sub(encoded_len(TRUNCATED));
    let mut used = 0;
    let mut end = 0;
    for (i, c) in s.char_indices() {
        let mut buf = [0; 4];
        let width = encoded_len(c.encode_utf8(&mut buf));
        if used + width > room {
            break;
        }
        used += width;
        end = i + c.len_utf8();
    }
    encode(&format!("{}{TRUNCATED}", &s[..end]))
}

/// `s` on one line, at most `max` characters, with "…" when cut.
fn shorten(s: &str, max: usize) -> String {
    let line = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= max {
        return line;
    }
    let mut cut: String = line.chars().take(max - 1).collect();
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The query's values, decoded, by key.
    fn params(url: &str) -> Vec<(String, String)> {
        let (base, query) = url.split_once('?').unwrap();
        assert_eq!(base, NEW_ISSUE);
        query
            .split('&')
            .map(|pair| {
                let (k, v) = pair.split_once('=').unwrap();
                (k.to_owned(), decode(v))
            })
            .collect()
    }

    fn param(url: &str, key: &str) -> String {
        params(url)
            .into_iter()
            .find(|(k, _)| k == key)
            .unwrap_or_else(|| panic!("no {key} in {url}"))
            .1
    }

    fn decode(v: &str) -> String {
        let bytes = v.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            assert!(unreserved(bytes[i]) || bytes[i] == b'%', "{v}");
            if bytes[i] == b'%' {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap();
                out.push(u8::from_str_radix(hex, 16).unwrap());
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        String::from_utf8(out).expect("whole characters")
    }

    fn item(input: &str, verdict: &str) -> Value {
        json!({"input": input, "verdict": verdict, "reason_text": "found"})
    }

    #[test]
    fn values_are_percent_encoded() {
        assert_eq!(encode("a-z_0.9~"), "a-z_0.9~");
        assert_eq!(
            encode("Über a b&c=d/e?f#g+h\n\"i\""),
            "%C3%9Cber%20a%20b%26c%3Dd%2Fe%3Ff%23g%2Bh%0A%22i%22"
        );
        assert_eq!(encoded_len("Über a"), encode("Über a").len());
    }

    #[test]
    fn a_result_fills_in_the_form() {
        let citation = "LeCun Y. Deep learning. Nature. 2015;521:436 & more";
        let result = item(citation, "plausible");
        let url = issue_url(&result);
        let keys: Vec<String> = params(&url).into_iter().map(|(k, _)| k).collect();
        assert_eq!(
            keys,
            [
                "template", "title", "citation", "verdict", "version", "surface", "output"
            ]
        );
        assert_eq!(param(&url, "template"), "wrong-verdict.yml");
        assert_eq!(
            param(&url, "title"),
            format!("Wrong verdict: plausible for \"{citation}\"")
        );
        assert_eq!(param(&url, "citation"), citation);
        assert_eq!(param(&url, "verdict"), "plausible");
        assert_eq!(
            param(&url, "version"),
            format!("aspero {}", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(param(&url, "surface"), "CLI");
        let output: Value = serde_json::from_str(&param(&url, "output")).unwrap();
        assert_eq!(output, result);
    }

    #[test]
    fn long_output_and_citations_are_cut_to_fit() {
        // Output: whole when it fits, compact, then cut.
        let mut result = item("Smith J. A study. 2020", "verified");
        result["best_match"] = json!({"abstract": "é ".repeat(5000)});
        let url = issue_url(&result);
        assert!(url.len() <= MAX_URL, "{}", url.len());
        let output = param(&url, "output");
        assert!(!output.starts_with("{\n"), "compact: {output:.40}");
        assert!(output.ends_with(TRUNCATED));
        assert!(url.len() > MAX_URL - 20, "uses the room: {}", url.len());

        // A pasted essay as the citation: cut too, the title to a line.
        let essay = "word ".repeat(3000);
        let url = issue_url(&item(&essay, "not_found"));
        assert!(url.len() <= MAX_URL);
        assert!(encode(&param(&url, "citation")).len() <= CITATION_MAX);
        assert!(param(&url, "citation").ends_with(TRUNCATED));
        let title = param(&url, "title");
        assert!(
            title.ends_with("…\"") && title.chars().count() < 120,
            "{title}"
        );
        assert!(!param(&url, "output").is_empty());
    }

    /// The form's fields: each `- type:` block's `id:` and dropdown
    /// `options`, by a line scan (no YAML parser for one test).
    fn form_fields() -> Vec<(String, Vec<String>)> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.github/ISSUE_TEMPLATE/wrong-verdict.yml"
        );
        let yaml = std::fs::read_to_string(path).unwrap();
        let mut fields: Vec<(String, Vec<String>)> = Vec::new();
        let mut in_options = false;
        for line in yaml.lines() {
            let t = line.trim();
            if let Some(id) = t.strip_prefix("id:") {
                fields.push((id.trim().to_owned(), Vec::new()));
                in_options = false;
            } else if t == "options:" {
                in_options = true;
            } else if in_options && let Some(option) = t.strip_prefix("- ") {
                if let Some((_, options)) = fields.last_mut() {
                    options.push(option.trim().to_owned());
                }
            } else {
                in_options = false;
            }
        }
        fields
    }

    #[test]
    fn the_fields_are_the_forms() {
        let fields = form_fields();
        let ids: Vec<&str> = fields.iter().map(|(id, _)| id.as_str()).collect();
        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "ids repeat: {ids:?}");

        let url = issue_url(&item("Smith J. A study. 2020", "mismatch"));
        for (key, value) in params(&url) {
            if key == "template" || key == "title" {
                continue;
            }
            let (_, options) = fields
                .iter()
                .find(|(id, _)| *id == key)
                .unwrap_or_else(|| panic!("the form has no field {key:?}: {ids:?}"));
            // A dropdown takes only one of its options.
            assert!(
                options.is_empty() || options.contains(&value),
                "{key}={value} isn't one of {options:?}"
            );
        }
        let verdicts = &fields.iter().find(|(id, _)| id == "verdict").unwrap().1;
        for verdict in ["verified", "plausible", "mismatch", "not_found", "invalid"] {
            assert!(verdicts.iter().any(|v| v == verdict), "{verdict}");
        }
    }
}
