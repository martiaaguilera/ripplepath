#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Fallback behaviour when a change touches code Ripplepath cannot see (docs/FINAL_REVIEW.md):
//! uncertainty must widen the selection, never shrink it to "nothing to run".

use std::path::Path;

use ripplepath_engine::fixture::{Snapshot, build_repo_from_snapshots, read_snapshot};
use ripplepath_engine::{AnalyzeOptions, SelectionDecision, SelectionMode, analyze};

fn base() -> Snapshot {
    read_snapshot(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/java-banking/v1")).unwrap()
}

fn with_file(mut snapshot: Snapshot, path: &str, text: &str) -> Snapshot {
    snapshot.files.retain(|(p, _)| p != path);
    snapshot.files.push((path.to_owned(), text.as_bytes().to_vec()));
    snapshot.files.sort();
    snapshot
}

fn decide(head: Snapshot, mode: SelectionMode) -> (SelectionDecision, Vec<String>, usize) {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    let v1 = with_file(base(), "src/main/kotlin/com/acme/bank/Rates.kt", "package com.acme.bank\nfun rate() = 1\n");
    build_repo_from_snapshots(&[v1, head], &repo).unwrap();
    let mut options = AnalyzeOptions::new(&repo, "main~1", "main");
    options.mode = Some(mode);
    let report = analyze(&options).unwrap();
    let codes = report.test_selection.fallback_reasons.iter().map(|r| r.code.clone()).collect();
    (report.test_selection.decision, codes, report.tests.len())
}

/// Only a source file of a language Ripplepath does not analyse changed: no symbol changed, so no
/// test is recommended. That must not read as "nothing to run" in conservative mode.
#[test]
fn a_change_only_in_unanalysed_source_is_not_an_empty_selection_in_conservative_mode() {
    let head = with_file(base(), "src/main/kotlin/com/acme/bank/Rates.kt", "package com.acme.bank\nfun rate() = 2\n");
    let (decision, codes, tests) = decide(head.clone(), SelectionMode::Conservative);
    assert_eq!(tests, 0);
    assert_eq!(decision, SelectionDecision::FullSuite, "{codes:?}");
    assert!(codes.iter().any(|c| c == "UNSUPPORTED_FILE_CHANGED"), "{codes:?}");

    // Balanced mode keeps its contract (only high-severity reasons force the full suite) but
    // still reports the reason.
    let (_, codes, _) = decide(head, SelectionMode::Balanced);
    assert!(codes.iter().any(|c| c == "UNSUPPORTED_FILE_CHANGED"), "{codes:?}");
}
