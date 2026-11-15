//! What the binary reads from the environment. `aspero mcp` sets up the
//! engine and, with no tool called, fetches nothing, then ends when stdin
//! closes.

use std::process::{Command, Output, Stdio};

/// `aspero mcp [args]` with `vars` set and stdin closed at once.
fn mcp(args: &[&str], vars: &[(&str, &str)]) -> Output {
    let cache = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_aspero"));
    command
        .args(args)
        .arg("mcp")
        .env("ASPERO_CACHE_DIR", cache.path())
        .env_remove("ASPERO_OPENALEX_CAP")
        .env_remove("ASPERO_OPENALEX_CAP_USD")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in vars {
        command.env(name, value);
    }
    command.output().unwrap()
}

#[test]
fn the_old_cap_variable_is_still_read() {
    let ok = mcp(&[], &[("ASPERO_OPENALEX_CAP", "0.5")]);
    assert!(ok.status.success(), "{ok:?}");

    // Read, not ignored: a value that isn't an amount is refused.
    let bad = mcp(&[], &[("ASPERO_OPENALEX_CAP", "half")]);
    assert_eq!(bad.status.code(), Some(2), "{bad:?}");
    let stderr = String::from_utf8_lossy(&bad.stderr);
    assert!(stderr.contains("ASPERO_OPENALEX_CAP: "), "{stderr}");

    // The new name wins over it, as the flag does.
    let new = mcp(
        &[],
        &[
            ("ASPERO_OPENALEX_CAP", "half"),
            ("ASPERO_OPENALEX_CAP_USD", "0.5"),
        ],
    );
    assert!(new.status.success(), "{new:?}");
    let flag = mcp(
        &["--openalex-cap-usd", "0.5"],
        &[("ASPERO_OPENALEX_CAP", "half")],
    );
    assert!(flag.status.success(), "{flag:?}");
}
