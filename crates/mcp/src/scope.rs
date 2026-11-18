//! Scopes: what a key may do over HTTP. Over stdio there are no keys: the
//! agent that launched the server may call every tool.
//!
//! A scope is a string. [`SOURCES`] is aspero's own, for the engine tools;
//! every other is a host tool's, declared by the host ("history.read",
//! "notes.write"), and means only what the host makes it mean. A key holds
//! a set of them, and may list and call a tool only if it holds exactly
//! that tool's scope: no scope implies another.

/// aspero's own scope: the engine tools, which search, look up and verify
/// works in open sources, and spend on them.
pub const SOURCES: &str = "sources";

/// The longest a scope may be, in bytes (it's ASCII).
pub const MAX_LEN: usize = 64;

/// Whether a host tool may declare `scope`: non-empty, at most
/// [`MAX_LEN`] ASCII letters, digits, `.`, `_` and `-`, and not
/// [`SOURCES`] in any case. If not, what's wrong, to follow "its scope
/// \"…\" ".
pub fn check_host(scope: &str) -> Result<(), String> {
    if scope.is_empty() {
        return Err("is empty".into());
    }
    if scope.eq_ignore_ascii_case(SOURCES) {
        return Err(format!(
            "is aspero's own (`{SOURCES}`): a host tool's must be another"
        ));
    }
    if scope.len() > MAX_LEN {
        return Err(format!("is longer than {MAX_LEN} characters"));
    }
    if !scope
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err("may hold only letters, digits, `.`, `_` and `-`".into());
    }
    Ok(())
}

/// Whether a key may hold `scope`: [`SOURCES`], or one a host tool may
/// declare ([`check_host`]), whether or not a tool declares it now. If
/// not, what's wrong.
pub fn check_key(scope: &str) -> Result<(), String> {
    if scope == SOURCES {
        return Ok(());
    }
    check_host(scope)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_scopes_are_short_plain_names_other_than_sources() {
        for ok in [
            "history.read",
            "history.remove",
            "notes.write",
            "a",
            "A_b-9",
        ] {
            assert_eq!(check_host(ok), Ok(()), "{ok}");
            assert_eq!(check_key(ok), Ok(()), "{ok}");
        }
        assert_eq!(check_host(&"a".repeat(MAX_LEN)), Ok(()));
        for bad in [
            "",
            "sources",
            "Sources",
            "history read",
            " history.read",
            "history/read",
            "historÿ.read",
            "history:read",
        ] {
            assert!(check_host(bad).is_err(), "{bad:?}");
        }
        assert!(check_host(&"a".repeat(MAX_LEN + 1)).is_err());
        // A key holds aspero's own as it's spelt, and nothing like it.
        assert_eq!(check_key("sources"), Ok(()));
        assert!(check_key("SOURCES").is_err());
        assert!(check_key("").is_err());
    }
}
