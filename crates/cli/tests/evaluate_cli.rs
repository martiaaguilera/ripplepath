#![allow(clippy::unwrap_used, clippy::expect_used)]

//! CLI smoke test for `ripplepath evaluate` over the first snapshots of `fixtures/eval-history` with
//! their committed real evidence. The metrics themselves are pinned by the engine's evaluation test.

use std::path::{Path, PathBuf};
use std::process::Command;

use ripplepath_engine::fixture::build_fixture_repo;
use ripplepath_engine::{CoverageFormat, CoverageInput, Limits, ingest_coverage_batch, ingest_junit};

fn ripplepath(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_ripplepath")).args(args).output().unwrap();
    assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/eval-history")
}

#[test]
fn evaluate_reports_json_and_text() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    let snapshots: Vec<PathBuf> = (1..=3).map(|v| fixture().join(format!("v{v}"))).collect();
    let refs: Vec<&Path> = snapshots.iter().map(PathBuf::as_path).collect();
    build_fixture_repo(&refs, &repo).unwrap();
    let db = dir.path().join("evidence.db");

    // Evidence of v1 and v2 (the bases), ingested oldest first.
    for (version, rev) in [(1, "main~2"), (2, "main~1")] {
        let evidence = fixture().join(format!("evidence/v{version}"));
        let mut files: Vec<PathBuf> =
            std::fs::read_dir(evidence.join("coverage")).unwrap().map(|e| e.unwrap().path()).collect();
        files.sort();
        let loaded: Vec<(String, String)> = files
            .iter()
            .map(|f| (f.file_stem().unwrap().to_string_lossy().into_owned(), std::fs::read_to_string(f).unwrap()))
            .collect();
        let inputs: Vec<CoverageInput<'_>> = loaded
            .iter()
            .map(|(class, text)| CoverageInput {
                format: CoverageFormat::Jacoco,
                text,
                source: class,
                test: Some(class),
            })
            .collect();
        ingest_coverage_batch(&repo, rev, &db, &inputs, &Limits::default()).unwrap();
        let junit = std::fs::read_to_string(evidence.join("junit/run-1.xml")).unwrap();
        ingest_junit(&repo, rev, &db, &junit, "run-1.xml", &Limits::default()).unwrap();
    }

    let junit =
        |v: usize| fixture().join(format!("evidence/v{v}/junit/run-1.xml")).to_string_lossy().replace('\\', "/");
    let cases = dir.path().join("cases.json");
    std::fs::write(
        &cases,
        format!(
            r#"{{"cases": [
                {{"name": "v1..v2", "base": "main~2", "head": "main~1", "junit": ["{}"]}},
                {{"name": "v2..v3", "base": "main~1", "head": "main", "junit": ["{}"]}}
            ]}}"#,
            junit(2),
            junit(3)
        ),
    )
    .unwrap();
    let args = |format: &'static str| {
        vec![
            "evaluate".to_owned(),
            "--repo".to_owned(),
            repo.to_string_lossy().into_owned(),
            "--cases".to_owned(),
            cases.to_string_lossy().into_owned(),
            "--db".to_owned(),
            db.to_string_lossy().into_owned(),
            "--format".to_owned(),
            format.to_owned(),
        ]
    };
    let run = |format: &'static str| {
        let owned = args(format);
        ripplepath(&owned.iter().map(String::as_str).collect::<Vec<_>>())
    };

    let json = run("json");
    assert_eq!(json, run("json"), "same inputs must give the same bytes");
    let report: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(report["evaluation_schema_version"], 1);
    assert_eq!(report["sample"]["cases"], 2);
    assert_eq!(report["sample"]["failing_cases"], 1);
    assert_eq!(report["sample"]["label"], "small sample — not statistically meaningful");
    let modes: Vec<&str> = report["modes"].as_array().unwrap().iter().map(|m| m["mode"].as_str().unwrap()).collect();
    assert_eq!(modes, ["CONSERVATIVE", "BALANCED", "FAST_FEEDBACK"]);

    let text = run("text");
    assert!(text.starts_with("Offline evaluation: 2 case(s), 1 with failing tests, 3 failing test(s)"), "{text}");
    assert!(text.contains("small sample — not statistically meaningful"), "{text}");
    assert!(text.contains("v2..v3"), "{text}");
}
