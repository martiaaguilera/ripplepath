//! The java-banking demo report, built once per test binary.
#![allow(clippy::expect_used)]

use std::path::Path;
use std::sync::OnceLock;

use ripplepath_engine::{AnalysisReport, AnalyzeOptions, analyze, fixture};

pub fn java_banking_report() -> AnalysisReport {
    static REPORT: OnceLock<AnalysisReport> = OnceLock::new();
    REPORT
        .get_or_init(|| {
            let dir = tempfile::tempdir().expect("temp dir");
            let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/java-banking");
            let repo = dir.path().join("repo");
            fixture::build_fixture_repo(&[&fixtures.join("v1"), &fixtures.join("v2")], &repo).expect("fixture");
            analyze(&AnalyzeOptions::new(&repo, "main~1", "main")).expect("analysis")
        })
        .clone()
}
