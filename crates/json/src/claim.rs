//! Claims as callers write them: one JSON object (or plain reference) per
//! line, echoed back on every result.

use aspero_core::{Claim, WorkId};
use serde::Deserialize;
use serde_json::{Value, json};

const EMPTY: &str = "claim has no text, title, authors or identifiers";

/// A claim as an agent would write it, one JSON object per stdin line.
/// Unknown keys are rejected so typos aren't silently ignored.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimInput {
    pub text: Option<String>,
    pub title: Option<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    pub year: Option<i16>,
    #[serde(alias = "journal")]
    pub container: Option<String>,
    #[serde(default, deserialize_with = "text_or_number")]
    pub volume: Option<String>,
    /// The first page, or a range.
    #[serde(default, alias = "pages", deserialize_with = "text_or_number")]
    pub page: Option<String>,
    /// The caller's own reference for this claim, echoed back untouched;
    /// allowed here, read from the raw line (see [`InputClaim::from_line`]).
    #[serde(rename = "key")]
    pub _key: Option<serde::de::IgnoredAny>,
    pub doi: Option<String>,
    pub arxiv: Option<String>,
    /// Any identifier form `lookup` accepts.
    pub id: Option<String>,
    #[serde(default)]
    pub ids: Vec<String>,
}

/// Volumes and pages are written as numbers or strings ("e1234").
fn text_or_number<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(match Option::<serde_json::Value>::deserialize(d)? {
        Some(serde_json::Value::String(s)) => Some(s),
        Some(serde_json::Value::Number(n)) => Some(n.to_string()),
        None | Some(serde_json::Value::Null) => None,
        Some(other) => {
            return Err(serde::de::Error::custom(format!(
                "expected text or a number, got {other}"
            )));
        }
    })
}

impl ClaimInput {
    /// The fields as one line, to label a claim given without text.
    pub fn describe(&self) -> String {
        let parts: Vec<String> = [
            (!self.authors.is_empty()).then(|| self.authors.join(", ")),
            self.title.clone(),
            self.container.clone(),
            self.year.map(|y| y.to_string()),
            self.volume.clone(),
            self.page.clone(),
            self.doi
                .clone()
                .or(self.arxiv.clone())
                .or(self.id.clone())
                .or(self.ids.first().cloned()),
        ]
        .into_iter()
        .flatten()
        .collect();
        parts.join(". ")
    }

    pub fn into_claim(self) -> Result<Claim, String> {
        let mut claim = match &self.text {
            Some(text) => Claim::from_input(text),
            None => Claim::default(),
        };
        let prefixed = [
            self.doi.map(|d| format!("doi:{d}")),
            self.arxiv.map(|a| format!("arxiv:{a}")),
            self.id,
        ];
        for raw in prefixed.into_iter().flatten().chain(self.ids) {
            let id: WorkId = raw.parse().map_err(|e| format!("{e}"))?;
            if !claim.ids.contains(&id) {
                claim.ids.push(id);
            }
        }
        claim.title = self.title;
        claim.authors = self.authors;
        claim.year = self.year;
        claim.container = self.container;
        claim.volume = self.volume;
        claim.page = self.page;
        if claim.is_empty() {
            return Err(EMPTY.into());
        }
        Ok(claim)
    }
}

/// A claim with the input it came from and the caller's `key`, or why that
/// input isn't one.
pub struct InputClaim {
    pub input: String,
    pub key: Option<Value>,
    pub claim: Result<Claim, String>,
}

impl InputClaim {
    /// One stdin line: a JSON object or a plain reference. A JSON line's
    /// `key` is kept even when the rest of it is invalid.
    pub fn from_line(line: &str) -> Self {
        let (key, claim) = match line.trim_start().starts_with('{') {
            false => (
                None,
                Some(Claim::from_input(line))
                    .filter(|c| !c.is_empty())
                    .ok_or_else(|| EMPTY.to_owned()),
            ),
            true => match serde_json::from_str::<Value>(line) {
                Err(e) => (None, Err(format!("invalid claim JSON: {e}"))),
                Ok(value) => (
                    value.get("key").cloned(),
                    ClaimInput::deserialize(value)
                        .map_err(|e| format!("invalid claim: {e}"))
                        .and_then(ClaimInput::into_claim),
                ),
            },
        };
        InputClaim {
            input: line.to_owned(),
            key,
            claim,
        }
    }

    /// The claims that could be read, with each one's index in `claims`.
    pub fn valid(claims: &[InputClaim]) -> (Vec<usize>, Vec<Claim>) {
        claims
            .iter()
            .enumerate()
            .filter_map(|(i, c)| Some((i, c.claim.as_ref().ok()?.clone())))
            .unzip()
    }

    /// What every result for this claim starts with: the input, the
    /// caller's key, and the claim as understood.
    pub fn echo(&self) -> Value {
        let mut echo = json!({ "input": self.input });
        if let Some(key) = &self.key {
            echo["key"] = key.clone();
        }
        if let Ok(claim) = &self.claim {
            echo["claim"] = json!(claim);
        }
        echo
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(line: &str) -> Result<Claim, String> {
        InputClaim::from_line(line).claim
    }

    #[test]
    fn json_lines_become_structured_claims() {
        let c = claim(
            r#"{"title": "Attention is all you need", "authors": ["Vaswani, A."], "year": 2017, "arxiv": "1706.03762"}"#,
        )
        .unwrap();
        assert_eq!(c.year, Some(2017));
        assert_eq!(c.ids[0].to_string(), "arxiv:1706.03762");
        assert!(
            claim(r#"{"titel": "typo"}"#)
                .unwrap_err()
                .contains("unknown field")
        );
        assert!(claim(r#"{"doi": "not a doi"}"#).is_err());
    }

    #[test]
    fn journal_volume_and_page_as_text_or_numbers() {
        let c = claim(
            r#"{"title": "X", "journal": "N Engl J Med", "volume": 376, "pages": "2545-55"}"#,
        )
        .unwrap();
        assert_eq!(c.container.as_deref(), Some("N Engl J Med"));
        assert_eq!(c.volume.as_deref(), Some("376"));
        assert_eq!(c.page.as_deref(), Some("2545-55"));
        assert!(claim(r#"{"title": "X", "volume": [1]}"#).is_err());
    }

    #[test]
    fn keys_are_echoed_even_for_invalid_claims() {
        let ok = InputClaim::from_line(r#"{"key": {"note": "pe.md", "line": 7}, "title": "X"}"#);
        assert_eq!(ok.echo()["key"], json!({"note": "pe.md", "line": 7}));
        assert_eq!(ok.echo()["claim"], json!({"title": "X"}));
        for blank in ["", "   ", r#"{"text": "  ", "key": 1}"#] {
            assert!(claim(blank).is_err(), "{blank:?}");
        }
        let bad = InputClaim::from_line(r#"{"key": "ref-12", "titel": "typo"}"#);
        assert!(bad.claim.is_err());
        assert_eq!(bad.echo()["key"], "ref-12");
        assert!(bad.echo().get("claim").is_none());
    }
}
