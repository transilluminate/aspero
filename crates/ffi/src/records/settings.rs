//! What a settings or about pane shows beside budgets: the OpenAlex
//! key's check, the cache's size, and what the library is built from.

use aspero_engine as engine;

use super::Budget;

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum KeyStatus {
    /// Accepted (even if its budget is spent: see `budget`).
    Valid,
    Rejected,
    /// OpenAlex couldn't be reached, so the key is untested.
    Unreachable,
}

impl From<engine::KeyStatus> for KeyStatus {
    fn from(s: engine::KeyStatus) -> Self {
        match s {
            engine::KeyStatus::Valid => KeyStatus::Valid,
            engine::KeyStatus::Rejected => KeyStatus::Rejected,
            _ => KeyStatus::Unreachable,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct KeyCheck {
    pub status: KeyStatus,
    /// Why, in English, unless valid.
    pub message: Option<String>,
    /// What the key has left.
    pub budget: Option<Budget>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct CacheStats {
    pub records: u64,
    pub identifiers: u64,
    /// Remembered "not found" answers.
    pub misses: u64,
    pub searches: u64,
    /// The cache's size on disk (its database pages; the write-ahead log
    /// aside), as `Config.cacheMaxMb` limits it.
    #[uniffi(default = 0)]
    pub bytes: u64,
    /// Works with cached citation links, counted once per direction (the
    /// works one cites, the works citing it).
    #[uniffi(default = 0)]
    pub edges: u64,
}

/// Something the library is built from: a Rust crate (aspero's own
/// included), or bundled data (`CSL styles (8)`, `CSL locales (2)`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Dependency {
    pub name: String,
    /// A crate's version; for data, the commit it's from.
    pub version: String,
    /// An SPDX licence expression: `MIT OR Apache-2.0`, `CC-BY-SA-3.0`.
    pub license: String,
    pub repository: Option<String>,
}
