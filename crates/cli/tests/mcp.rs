//! `aspero mcp` as an agent launches it: the protocol alone on stdout, and
//! a clean exit when stdin closes. No tool is called, so nothing is fetched.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

#[test]
fn stdout_carries_the_protocol_and_eof_ends_it() {
    let cache = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_aspero"))
        .arg("mcp")
        .env("ASPERO_CACHE_DIR", cache.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let messages = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "0"},
        }}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    ];
    let mut stdin = child.stdin.take().unwrap();
    for message in messages {
        writeln!(stdin, "{message}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{out:?}");

    let stdout = String::from_utf8(out.stdout).unwrap();
    let replies: Vec<Value> = stdout
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {line}")))
        .collect();
    assert_eq!(replies.len(), 2, "{stdout}");
    assert!(replies.iter().all(|r| r["jsonrpc"] == "2.0"));
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "aspero");
    let tools = replies[1]["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 7);
    assert!(tools.iter().all(|t| t["outputSchema"].is_object()));
}

#[test]
fn a_budget_must_be_positive() {
    let out = Command::new(env!("CARGO_BIN_EXE_aspero"))
        .args(["mcp", "--budget", "0"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}
