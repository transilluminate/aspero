//! Building citation claims from arguments, flags and stdin lines.

use std::io::IsTerminal;

use aspero_json::{ClaimInput, InputClaim};

use super::{Failure, lines, read_stdin};

/// The claims to process: `text` plus flags as one claim, or one per stdin
/// line when `text` is `-`, or when neither text nor flags are given and
/// stdin is piped (blank lines and `#` comments skipped). With `split`,
/// the text or stdin is a reference list, split into references.
pub fn claims(
    text: Option<String>,
    fields: ClaimInput,
    split: bool,
) -> Result<Vec<InputClaim>, Failure> {
    // Nothing given: stdin, as `lookup` reads it, unless it's a terminal.
    let text = match text {
        None if fields.describe().is_empty() => {
            if std::io::stdin().is_terminal() {
                return Err(Failure::usage(if split {
                    "no reference list given (pass one, or pipe it)"
                } else {
                    "no citation given (pass one, or pipe one per line)"
                }));
            }
            Some("-".to_owned())
        }
        text => text,
    };
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
        return Ok(lines(&read_stdin()?).map(InputClaim::from_line).collect());
    }
    let label = text.clone().unwrap_or_else(|| fields.describe());
    let input = ClaimInput { text, ..fields };
    Ok(vec![InputClaim {
        input: label,
        key: None,
        claim: input.into_claim(),
    }])
}
