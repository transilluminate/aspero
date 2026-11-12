//! The binary's behaviour at the edges of a pipeline.

#![cfg(unix)]

use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};

/// `aspero scan … | head -1`: a closed stdout ends the process with SIGPIPE,
/// quietly, rather than a panic.
#[test]
fn closed_stdout_exits_quietly() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_aspero"))
        .args(["--format", "jsonl", "scan", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Close the reading end before anything is written.
    drop(child.stdout.take());
    let mut stdin = child.stdin.take().unwrap();
    // Ignore write errors: the child may die before reading it all.
    for i in 0..1000 {
        let _ = writeln!(stdin, "See doi:10.1038/nature{i:05}.");
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.signal(), Some(libc::SIGPIPE), "{out:?}");
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
