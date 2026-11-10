//! Building citation claims from arguments, flags and stdin lines.

use aspero_core::{Claim, WorkId};
use serde::Deserialize;

use super::{Failure, read_stdin};

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
    pub doi: Option<String>,
    pub arxiv: Option<String>,
    /// Any identifier form `lookup` accepts.
    pub id: Option<String>,
    #[serde(default)]
    pub ids: Vec<String>,
}

impl ClaimInput {
    pub fn into_claim(self) -> Result<Claim, String> {
        let mut claim = match &self.text {
            Some(text) => text_claim(text),
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
        if claim.is_empty() {
            return Err("claim has no text, title, authors or identifiers".into());
        }
        Ok(claim)
    }
}

/// A free-text claim. If the whole text is an identifier, the claim is just
/// that identifier (an existence check); otherwise the text is the reference,
/// with any DOIs or arXiv IDs in it taken as asserted.
pub fn text_claim(text: &str) -> Claim {
    let text = text.trim();
    match text.parse::<WorkId>() {
        Ok(id) => {
            let mut claim = Claim::default();
            claim.ids = vec![id];
            claim
        }
        Err(_) => Claim::from_text(text),
    }
}

/// One stdin line: a JSON object or a plain reference.
pub fn line_claim(line: &str) -> Result<Claim, String> {
    if line.trim_start().starts_with('{') {
        let input: ClaimInput =
            serde_json::from_str(line).map_err(|e| format!("invalid claim JSON: {e}"))?;
        input.into_claim()
    } else {
        Ok(text_claim(line))
    }
}

/// A claim with the input it came from, or why that input isn't one.
pub type InputClaim = (String, Result<Claim, String>);

/// The claims to process: `text` plus flags as one claim, or one per stdin
/// line when `text` is `-` (blank lines and `#` comments skipped). Each comes
/// with the input it was built from, for reporting.
pub fn claims(text: Option<String>, fields: ClaimInput) -> Result<Vec<InputClaim>, Failure> {
    if text.as_deref() == Some("-") {
        let input = read_stdin()?;
        return Ok(input
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| (l.to_owned(), line_claim(l)))
            .collect());
    }
    let label = text.clone().unwrap_or_else(|| describe_fields(&fields));
    let input = ClaimInput { text, ..fields };
    Ok(vec![(label, input.into_claim())])
}

fn describe_fields(f: &ClaimInput) -> String {
    let parts: Vec<String> = [
        (!f.authors.is_empty()).then(|| f.authors.join(", ")),
        f.title.clone(),
        f.year.map(|y| y.to_string()),
        f.doi.clone().or(f.arxiv.clone()).or(f.id.clone()),
    ]
    .into_iter()
    .flatten()
    .collect();
    parts.join(". ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_identifier_is_an_existence_check() {
        let claim = text_claim("10.1038/nature14539");
        assert_eq!(claim.ids.len(), 1);
        assert!(claim.text.is_none());
    }

    #[test]
    fn references_keep_text_and_embedded_ids() {
        let claim = text_claim("LeCun et al. Deep learning. Nature 2015. doi:10.1038/nature14539.");
        assert!(claim.text.is_some());
        assert_eq!(claim.ids[0].to_string(), "doi:10.1038/nature14539");
    }

    #[test]
    fn json_lines_become_structured_claims() {
        let claim = line_claim(
            r#"{"title": "Attention is all you need", "authors": ["Vaswani, A."], "year": 2017, "arxiv": "1706.03762"}"#,
        )
        .unwrap();
        assert_eq!(claim.year, Some(2017));
        assert_eq!(claim.ids[0].to_string(), "arxiv:1706.03762");
        assert!(
            line_claim(r#"{"titel": "typo"}"#)
                .unwrap_err()
                .contains("unknown field")
        );
        assert!(line_claim(r#"{"doi": "not a doi"}"#).is_err());
    }
}
