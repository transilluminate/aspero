//! Building citation claims from arguments, flags and stdin lines.

use aspero_json::{ClaimInput, InputClaim};

use super::{Failure, read_stdin};

/// The claims to process: `text` plus flags as one claim, or one per stdin
/// line when `text` is `-` (blank lines and `#` comments skipped). With
/// `split`, the text or stdin is a reference list, split into references.
pub fn claims(
    text: Option<String>,
    fields: ClaimInput,
    split: bool,
) -> Result<Vec<InputClaim>, Failure> {
    if split {
        let list = match text.as_deref() {
            Some("-") => read_stdin()?,
            Some(text) => text.to_owned(),
            None => return Err(Failure::usage("--split needs text or `-`")),
        };
        return Ok(aspero_core::split_references(&list)
            .into_iter()
            .map(|r| {
                let mut claim = InputClaim::from_line(&r.text);
                if let Ok(c) = &mut claim.claim {
                    c.alternatives = r.alternatives;
                }
                claim
            })
            .collect());
    }
    if text.as_deref() == Some("-") {
        let input = read_stdin()?;
        return Ok(input
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(InputClaim::from_line)
            .collect());
    }
    let label = text.clone().unwrap_or_else(|| fields.describe());
    let input = ClaimInput { text, ..fields };
    Ok(vec![InputClaim {
        input: label,
        key: None,
        claim: input.into_claim(),
    }])
}
