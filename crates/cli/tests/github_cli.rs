#![allow(clippy::unwrap_used, clippy::expect_used)]

//! `analyze --output-dir --fail-on-policy` on the java-banking demo: every CI output is written,
//! stdout matches the file of the same format, and a failing policy exits with status 2.

use std::path::Path;
use std::process::Command;

use ripplepath_engine::fixture::build_fixture_repo;

#[test]
fn writes_all_ci_outputs_and_exits_2_on_policy_failure() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/java-banking");
    let repo = dir.path().join("repo");
    build_fixture_repo(&[&fixture.join("v1"), &fixture.join("v2")], &repo).unwrap();
    let out_dir = dir.path().join("out");
    let run = |extra: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_ripplepath"))
            .args(["analyze", "--repo", repo.to_str().unwrap(), "--base", "main~1", "--head", "main"])
            .args(extra)
            .output()
            .unwrap()
    };

    let output = run(&["--format", "sarif", "--output-dir", out_dir.to_str().unwrap(), "--fail-on-policy"]);
    assert_eq!(output.status.code(), Some(2), "{}", String::from_utf8_lossy(&output.stderr));
    for name in ["analysis.json", "summary.md", "ripplepath.sarif", "annotations.txt"] {
        assert!(out_dir.join(name).is_file(), "{name} missing");
    }
    let sarif_file = std::fs::read(out_dir.join("ripplepath.sarif")).unwrap();
    assert_eq!(output.stdout, sarif_file, "stdout and --output-dir agree");

    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out_dir.join("analysis.json")).unwrap()).unwrap();
    assert_eq!(report["policy"]["result"], "FAIL");
    let summary = std::fs::read_to_string(out_dir.join("summary.md")).unwrap();
    assert!(summary.contains("**Policy: FAIL**"), "{summary}");
    let annotations = std::fs::read_to_string(out_dir.join("annotations.txt")).unwrap();
    assert!(
        annotations
            .lines()
            .all(|l| l.starts_with("::error ") || l.starts_with("::warning ") || l.starts_with("::notice ")),
        "{annotations}"
    );

    // Without --fail-on-policy the same failing policy is reported but does not fail the command.
    let output = run(&["--format", "markdown"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8(output.stdout).unwrap(), summary);
    let output = run(&["--format", "github-annotations"]);
    assert_eq!(String::from_utf8(output.stdout).unwrap(), annotations);
}
