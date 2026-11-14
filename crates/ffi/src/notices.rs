//! The library's third-party notices, as `just notices` wrote them to
//! `notices/` (checked by `notices-gen`'s tests).

use std::sync::LazyLock;

use serde_json::Value;

use crate::records::Dependency;

/// The full notices text.
pub const NOTICES: &str = include_str!("../notices/notices.txt");

const DEPENDENCIES: &str = include_str!("../notices/dependencies.json");

/// The crates and data, as listed (by name, ignoring case).
pub fn dependencies() -> &'static [Dependency] {
    static PARSED: LazyLock<Vec<Dependency>> = LazyLock::new(|| parse(DEPENDENCIES));
    &PARSED
}

fn parse(json: &str) -> Vec<Dependency> {
    let file: Value = serde_json::from_str(json).expect("notices/dependencies.json is JSON");
    let text = |d: &Value, key: &str| d[key].as_str().map(str::to_owned);
    file["dependencies"]
        .as_array()
        .expect("notices/dependencies.json lists dependencies")
        .iter()
        .map(|d| Dependency {
            name: text(d, "name").expect("a dependency's name"),
            version: text(d, "version").expect("a dependency's version"),
            license: text(d, "license").expect("a dependency's licence"),
            repository: text(d, "repository"),
        })
        .collect()
}
