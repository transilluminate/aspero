//! What a bare `aspero` prints: the overview, on stdout, and success.

use std::process::Command;

#[test]
fn a_bare_aspero_gives_the_overview() {
    let out = Command::new(env!("CARGO_BIN_EXE_aspero"))
        .output()
        .expect("aspero runs");
    assert!(out.status.success(), "{:?}", out.status);
    let text = String::from_utf8(out.stdout).unwrap();
    for part in ["Check", "verify <citation>", "Examples", "aspero --help"] {
        assert!(text.contains(part), "missing {part:?} in:\n{text}");
    }
    assert!(out.stderr.is_empty());
}

#[test]
fn short_help_leaves_the_cache_knobs_to_long_help() {
    let help = |flag: &str| {
        let out = Command::new(env!("CARGO_BIN_EXE_aspero"))
            .arg(flag)
            .output()
            .expect("aspero runs");
        String::from_utf8(out.stdout).unwrap()
    };
    let (short, long) = (help("-h"), help("--help"));
    assert!(!short.contains("--cache-max-mb") && long.contains("--cache-max-mb"));
    assert!(!short.contains("Exit codes") && long.contains("Exit codes"));
    assert!(short.contains("aspero --help"));
}
