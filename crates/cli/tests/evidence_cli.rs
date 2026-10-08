#![allow(clippy::unwrap_used, clippy::expect_used)]

//! CLI smoke test: `ingest junit|coverage` then `analyze --db --mode` on the TypeScript fixture with
//! its committed real evidence.

use std::path::Path;
use std::process::Command;

use ripplepath_engine::fixture::build_fixture_repo;

fn ripplepath(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_ripplepath")).args(args).output().unwrap();
    assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn ingest_then_analyze_with_a_mode() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/typescript-checkout");
    let repo = dir.path().join("repo");
    build_fixture_repo(&[&fixture.join("v1"), &fixture.join("v2")], &repo).unwrap();
    let db = dir.path().join("evidence.db");
    let (repo, db) = (repo.to_str().unwrap(), db.to_str().unwrap());

    let junit = fixture.join("evidence/v2/junit/run-1.xml");
    let out = ripplepath(&["ingest", "junit", junit.to_str().unwrap(), "--repo", repo, "--rev", "main", "--db", db]);
    assert!(out.contains("1 report(s), 6 mapped, 0 unmapped"), "{out}");

    let lcov = fixture.join("evidence/v2/coverage/src/cart.test.ts.lcov");
    let out = ripplepath(&[
        "ingest",
        "coverage",
        lcov.to_str().unwrap(),
        "--format",
        "lcov",
        "--test",
        "src/cart.test.ts",
        "--repo",
        repo,
        "--rev",
        "main",
        "--db",
        db,
    ]);
    assert!(out.contains("0 unmapped"), "{out}");

    let json = ripplepath(&[
        "analyze",
        "--repo",
        repo,
        "--base",
        "main~1",
        "--head",
        "main",
        "--db",
        db,
        "--mode",
        "conservative",
        "--format",
        "json",
    ]);
    let report: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(report["test_selection"]["mode"], "CONSERVATIVE");
    // Medium-severity reasons (an unresolved call) are enough for conservative mode to widen.
    assert_eq!(report["test_selection"]["decision"], "FULL_SUITE");
    assert_eq!(report["evidence"]["test_runs"], 1);
    assert_eq!(report["evidence"]["coverage_reports"], 1);
}
