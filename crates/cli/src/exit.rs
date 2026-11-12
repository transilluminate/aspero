//! Exit codes: one per [`ErrorCategory`], documented in `--help`.

use std::process::ExitCode;

use aspero_core::ErrorCategory;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Exit {
    Ok = 0,
    Internal = 1,
    Usage = 2,
    NotFound = 3,
    Unavailable = 4,
    Throttled = 5,
    Auth = 6,
    Unsupported = 7,
    Protocol = 8,
    /// A citation doesn't check out: mismatched identifier, or only a
    /// partial match.
    Unverified = 9,
}

impl Exit {
    pub fn from_category(category: ErrorCategory) -> Exit {
        match category {
            ErrorCategory::Unavailable => Exit::Unavailable,
            ErrorCategory::Throttled => Exit::Throttled,
            ErrorCategory::Auth => Exit::Auth,
            ErrorCategory::NotFound => Exit::NotFound,
            ErrorCategory::Usage => Exit::Usage,
            ErrorCategory::Unsupported => Exit::Unsupported,
            ErrorCategory::Protocol => Exit::Protocol,
            _ => Exit::Internal,
        }
    }

    /// The more severe of two outcomes, for commands with several results.
    /// Problems the caller can act on (usage, credentials, budget) outrank
    /// "not found".
    pub fn worst(self, other: Exit) -> Exit {
        let rank = |e: Exit| match e {
            Exit::Ok => 0,
            Exit::NotFound => 1,
            Exit::Unverified => 2,
            Exit::Unsupported => 3,
            Exit::Unavailable | Exit::Protocol | Exit::Internal => 4,
            Exit::Throttled => 5,
            Exit::Auth => 6,
            Exit::Usage => 7,
        };
        if rank(other) > rank(self) {
            other
        } else {
            self
        }
    }

    pub fn code(self) -> ExitCode {
        ExitCode::from(self as u8)
    }

    /// The `category` of a JSON error. Agents match on these: keep them.
    pub fn as_str(self) -> &'static str {
        match self {
            Exit::Ok => "ok",
            Exit::Internal => "internal",
            Exit::Usage => "usage",
            Exit::NotFound => "notfound",
            Exit::Unavailable => "unavailable",
            Exit::Throttled => "throttled",
            Exit::Auth => "auth",
            Exit::Unsupported => "unsupported",
            Exit::Protocol => "protocol",
            Exit::Unverified => "unverified",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worst_prefers_actionable_problems() {
        assert_eq!(Exit::Ok.worst(Exit::NotFound), Exit::NotFound);
        assert_eq!(Exit::NotFound.worst(Exit::Throttled), Exit::Throttled);
        assert_eq!(Exit::Usage.worst(Exit::Unavailable), Exit::Usage);
    }

    /// The names came from `Debug`, lowercased; they stay as they were.
    #[test]
    fn names_are_as_they_were() {
        let all = [
            Exit::Ok,
            Exit::Internal,
            Exit::Usage,
            Exit::NotFound,
            Exit::Unavailable,
            Exit::Throttled,
            Exit::Auth,
            Exit::Unsupported,
            Exit::Protocol,
            Exit::Unverified,
        ];
        for exit in all {
            assert_eq!(exit.as_str(), format!("{exit:?}").to_lowercase());
        }
    }
}
