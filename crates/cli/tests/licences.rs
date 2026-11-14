//! `aspero licences`: the crates and data in this binary, and the notices.

use std::process::Command;

fn aspero(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_aspero"))
        .args(args)
        .output()
        .expect("aspero runs");
    assert!(out.status.success(), "{:?}", out.status);
    assert!(out.stderr.is_empty());
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn licences_list_as_a_table() {
    let table = aspero(&["licences", "--format", "table"]);
    for part in [
        "Licence",
        "clap",
        "MIT OR Apache-2.0",
        "CSL styles (8)",
        "CC-BY-SA-3.0",
    ] {
        assert!(table.contains(part), "missing {part:?}");
    }
    let line = table.lines().find(|l| l.contains(" aspero ")).unwrap();
    assert!(line.contains(env!("CARGO_PKG_VERSION")), "{line}");
}

#[test]
fn licences_list_as_json() {
    let json: serde_json::Value =
        serde_json::from_str(&aspero(&["licenses", "--format", "json"])).unwrap();
    let deps = json["dependencies"].as_array().unwrap();
    assert!(deps.len() > 100);
    let names: Vec<&str> = deps.iter().map(|d| d["name"].as_str().unwrap()).collect();
    assert!(names.is_sorted_by_key(|n| n.to_lowercase()));
    let clap = deps.iter().find(|d| d["name"] == "clap").unwrap();
    assert_eq!(clap["license"], "MIT OR Apache-2.0");
    assert!(clap["repository"].as_str().unwrap().contains("clap"));
    // Not in the binary: the FFI's bindings, and build and dev dependencies.
    for name in ["uniffi", "cc", "tempfile"] {
        assert!(!names.contains(&name), "{name}");
    }
}

#[test]
fn full_prints_the_notices() {
    let text = aspero(&["licences", "--full", "--format", "text"]);
    assert!(text.starts_with("Third-party software and data in aspero"));
    assert!(text.contains("The aspero command-line tool"));
    assert!(text.contains("Used by:\n  "));
    assert!(text.contains("Permission is hereby granted, free of charge"));
    assert!(text.contains("Citation Style Language styles and locales"));
    let json: serde_json::Value =
        serde_json::from_str(&aspero(&["licences", "--full", "--format", "json"])).unwrap();
    assert_eq!(json["notices"].as_str(), Some(text.as_str()));
}
