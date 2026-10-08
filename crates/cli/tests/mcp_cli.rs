#![allow(clippy::unwrap_used, clippy::expect_used)]

//! `ripplepath mcp` as a real subprocess: stdout carries only protocol messages, and the server
//! exits when its input closes.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use ripplepath_engine::fixture::build_fixture_repo;

#[test]
fn mcp_subcommand_speaks_json_rpc_over_stdio() {
    let dir = tempfile::tempdir().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/java-banking");
    build_fixture_repo(&[&root.join("v1"), &root.join("v2")], dir.path()).unwrap();
    let cache = tempfile::tempdir().unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_ripplepath"))
        .args(["mcp", "--repo"])
        .arg(dir.path())
        .args(["--base", "main~1", "--head", "main"])
        // Keeps the default database lookup away from the user's real cache directory.
        .env("RIPPLEPATH_CACHE_DIR", cache.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let input = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"what_if","arguments":{"symbol":"java:com.acme.bank.domain.Money#minus(Money)"}}}"#,
        "\n",
    );
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));

    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<serde_json::Value> = stdout.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["result"]["serverInfo"]["name"], "ripplepath");
    let result = &lines[1]["result"];
    assert_eq!(result["isError"], false);
    let impacted = result["structuredContent"]["impacted_symbols"].as_array().unwrap();
    assert!(impacted.iter().any(|i| i["id"] == "java:com.acme.bank.domain.Account#withdraw(Money)"));
}

#[test]
fn mcp_subcommand_refuses_a_path_that_is_not_a_repository() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ripplepath"))
        .args(["mcp", "--repo"])
        .arg(dir.path())
        .env("RIPPLEPATH_CACHE_DIR", dir.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "nothing but protocol messages on stdout");
}
