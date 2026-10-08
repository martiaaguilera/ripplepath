#![allow(clippy::unwrap_used, clippy::expect_used)]

//! `ripplepath demo` from the final review (docs/FINAL_REVIEW.md): it must not read fixtures from
//! the current directory, and its output must be byte-identical across runs.

use std::path::Path;
use std::process::{Command, Output};

fn demo(cwd: &Path, dir: &str, fixture: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ripplepath"))
        .current_dir(cwd)
        .args(["demo", "--dir", dir, "--fixture", fixture, "--format", "json"])
        .output()
        .unwrap()
}

/// A checkout that ships its own `fixtures/java-banking/v1` must not be able to substitute the
/// demo's files: before the fix the demo looked in the current directory first.
#[test]
fn demo_ignores_fixtures_in_the_current_directory() {
    let cwd = tempfile::tempdir().unwrap();
    for snapshot in ["v1", "v2"] {
        let planted = cwd.path().join("fixtures/java-banking").join(snapshot);
        std::fs::create_dir_all(&planted).unwrap();
        std::fs::write(planted.join("PLANTED.java"), "class Planted {}\n").unwrap();
    }
    let output = demo(cwd.path(), "demo-repo", "java-banking");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let repo = cwd.path().join("demo-repo");
    assert!(!repo.join("PLANTED.java").exists());
    assert!(repo.join("src/main/java/com/acme/bank/domain/Account.java").is_file());
}

#[test]
fn demo_output_is_byte_identical_across_runs() {
    for fixture in ["java-banking", "typescript-checkout"] {
        let cwd = tempfile::tempdir().unwrap();
        let first = demo(cwd.path(), "a", fixture);
        let second = demo(cwd.path(), "b", fixture);
        assert!(first.status.success(), "{}", String::from_utf8_lossy(&first.stderr));
        assert!(second.status.success(), "{}", String::from_utf8_lossy(&second.stderr));
        assert_eq!(first.stdout, second.stdout, "{fixture}");
        assert!(!first.stdout.is_empty());
    }
}
