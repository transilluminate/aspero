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
            Exit::Unsupported => 2,
            Exit::Unavailable | Exit::Protocol | Exit::Internal => 3,
            Exit::Throttled => 4,
            Exit::Auth => 5,
            Exit::Usage => 6,
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
}
