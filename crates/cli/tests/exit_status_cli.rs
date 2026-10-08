#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Exit status 2 is reserved for a merge-policy FAIL (`analyze --fail-on-policy`). Usage errors
//! must not share it, or CI reads a misconfiguration as a verdict (docs/FINAL_REVIEW.md).

use std::process::Command;

fn status(args: &[&str]) -> i32 {
    Command::new(env!("CARGO_BIN_EXE_ripplepath")).args(args).output().unwrap().status.code().unwrap()
}

#[test]
fn usage_errors_exit_with_1_not_the_policy_status() {
    assert_eq!(status(&["analyze", "--base", "main", "--mode", "fastfeedback"]), 1);
    assert_eq!(status(&["analyze", "--base", "main", "--max-depth", "0"]), 1);
    assert_eq!(status(&["no-such-command"]), 1);
    assert_eq!(status(&["analyze"]), 1, "missing --base");
}

#[test]
fn help_and_version_succeed() {
    assert_eq!(status(&["--help"]), 0);
    assert_eq!(status(&["--version"]), 0);
    assert_eq!(status(&["analyze", "--help"]), 0);
}
