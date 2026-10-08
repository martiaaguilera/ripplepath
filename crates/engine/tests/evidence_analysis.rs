#![allow(clippy::unwrap_used, clippy::expect_used)]

//! End-to-end test intelligence on the fixtures with REAL evidence: coverage and JUnit files
//! produced by scripts/collect-evidence-{java,ts}.sh (see fixtures/*/evidence/README.md) are
//! ingested at the revision they were measured at, then analysed.

use std::path::{Path, PathBuf};

use ripplepath_core::{EdgeKind, Evidence};
use ripplepath_engine::fixture::build_fixture_repo;
use ripplepath_engine::{
    AnalysisError, AnalysisReport, AnalyzeOptions, CoverageFormat, CoverageInput, CoverageStatus, EvidenceTier,
    IngestOutcome, Limits, Reliability, SelectionDecision, SelectionMode, Severity, TestRecommendation, analyze,
    ingest_coverage, ingest_junit,
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(name)
}

fn files_with_extension(dir: &Path, extension: &str, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files_with_extension(&path, extension, out);
        } else if path.to_string_lossy().ends_with(extension) {
            out.push(path);
        }
    }
    out.sort();
}

struct Setup {
    _dir: tempfile::TempDir,
    repo: PathBuf,
    db: PathBuf,
}

/// Builds the fixture repository (v1 = `main~1`, v2 = `main`) and an empty evidence database.
fn setup(name: &str) -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    let root = fixture(name);
    build_fixture_repo(&[&root.join("v1"), &root.join("v2")], &repo).unwrap();
    let db = dir.path().join("evidence.db");
    Setup { _dir: dir, repo, db }
}

const REVISIONS: [(&str, &str); 2] = [("v1", "main~1"), ("v2", "main")];

/// Ingests every committed coverage file of `version`; the test that produced each file is named
/// by its path (`<class FQN>.xml` for Java, `<test file>.lcov` for TypeScript).
fn ingest_coverage_of(s: &Setup, name: &str, version: &str, rev: &str) -> Vec<IngestOutcome> {
    let dir = fixture(name).join("evidence").join(version).join("coverage");
    let (format, extension) =
        if name == "java-banking" { (CoverageFormat::Jacoco, ".xml") } else { (CoverageFormat::Lcov, ".lcov") };
    let mut files = Vec::new();
    files_with_extension(&dir, extension, &mut files);
    assert!(!files.is_empty(), "no committed coverage in {}", dir.display());
    files
        .iter()
        .map(|file| {
            let relative = file.strip_prefix(&dir).unwrap().to_string_lossy().replace('\\', "/");
            let test = relative.strip_suffix(extension).unwrap().to_owned();
            let text = std::fs::read_to_string(file).unwrap();
            ingest_coverage(
                &s.repo,
                rev,
                &s.db,
                CoverageInput { format, text: &text, source: &relative, test: Some(&test) },
                &Limits::default(),
            )
            .unwrap()
        })
        .collect()
}

fn ingest_junit_of(s: &Setup, name: &str, version: &str, rev: &str) -> Vec<IngestOutcome> {
    let dir = fixture(name).join("evidence").join(version).join("junit");
    let mut files = Vec::new();
    files_with_extension(&dir, ".xml", &mut files);
    assert!(!files.is_empty(), "no committed JUnit results in {}", dir.display());
    files
        .iter()
        .map(|file| {
            let text = std::fs::read_to_string(file).unwrap();
            ingest_junit(&s.repo, rev, &s.db, &text, &file.to_string_lossy(), &Limits::default()).unwrap()
        })
        .collect()
}

fn ingest_everything(s: &Setup, name: &str) {
    for (version, rev) in REVISIONS {
        ingest_coverage_of(s, name, version, rev);
        ingest_junit_of(s, name, version, rev);
    }
}

fn analyze_with(s: &Setup, mode: SelectionMode) -> AnalysisReport {
    let mut options = AnalyzeOptions::new(&s.repo, "main~1", "main");
    options.db = Some(s.db.clone());
    options.mode = Some(mode);
    analyze(&options).unwrap()
}

fn coverage_of(report: &AnalysisReport, id: &str) -> Option<CoverageStatus> {
    report.changed_symbols.iter().find(|s| s.id.as_str() == id).unwrap_or_else(|| panic!("{id} not changed")).coverage
}

fn test<'a>(report: &'a AnalysisReport, id: &str) -> &'a TestRecommendation {
    report.tests.iter().find(|t| t.id.as_str() == id).unwrap_or_else(|| panic!("{id} not recommended"))
}

const TS_FLAKY: &str = "ts:src/clock.test.ts#test:applies a discount before the checkout deadline";
const TS_CART: &str = "ts:src/cart.test.ts#test:Cart > totals line items";
const TS_SPRING: &str = "ts:src/pricing/discount.test.ts#test:SPRING20 > takes 20% off";

#[test]
fn every_real_result_and_coverage_file_maps_onto_the_fixtures() {
    for (name, units_v1, units_v2) in [("java-banking", 3, 3), ("typescript-checkout", 5, 6)] {
        let s = setup(name);
        for (version, rev) in REVISIONS {
            let expected = if version == "v1" { units_v1 } else { units_v2 };
            for outcome in ingest_junit_of(&s, name, version, rev) {
                // Vitest writes classname = file path, name = "describe > title"; the JUnit Platform
                // writes classname = class FQN, name = "method()". Both must map without loss.
                assert_eq!((outcome.mapped, outcome.unmapped), (expected, 0), "{name} {version}: {outcome:?}");
            }
            for outcome in ingest_coverage_of(&s, name, version, rev) {
                assert_eq!(outcome.unmapped, 0, "{name} {version}: {:?}", outcome.unmapped_examples);
                assert!(outcome.mapped > 0 && outcome.covered_symbols > 0, "{name} {version}: {outcome:?}");
            }
        }
    }
}

#[test]
fn java_coverage_explains_recommendations_and_measures_changed_symbols() {
    let s = setup("java-banking");
    ingest_everything(&s, "java-banking");
    let report = analyze_with(&s, SelectionMode::Balanced);

    assert_eq!(report.evidence.coverage_reports, 3, "latest testwise report per test class");
    assert_eq!(report.evidence.coverage_reports_other_commits, 0);
    assert!(report.evidence.coverage_edges > 0);

    // TransferServiceTest's measured run executed TransferService#load, which changed.
    let service = test(&report, "java:com.acme.bank.application.TransferServiceTest");
    assert_eq!(service.tier, EvidenceTier::Strong);
    assert!(service.coverage_observed);
    let hop = service.path.last().unwrap();
    assert_eq!((hop.edge.kind, hop.edge.evidence), (EdgeKind::Tests, Evidence::CoverageObserved));
    assert_eq!(hop.edge.to.as_str(), "java:com.acme.bank.application.TransferService#load(String)");
    assert!(hop.edge.rule.starts_with("coverage.jacoco@"), "{}", hop.edge.rule);
    // The controller test also reaches the change statically (three exact CALLS); the measured,
    // class-level hop is listed next to it rather than hidden by it.
    let unit = test(&report, "java:com.acme.bank.api.TransferControllerTest#returnsOkOnSuccessfulTransfer()");
    assert_eq!((unit.tier, unit.coverage_observed), (EvidenceTier::Medium, false));
    assert!(test(&report, "java:com.acme.bank.api.TransferControllerTest").coverage_observed);

    let cov = |id: &str| coverage_of(&report, id);
    assert_eq!(cov("java:com.acme.bank.domain.Account#withdraw(Money)"), Some(CoverageStatus::Covered));
    assert_eq!(cov("java:com.acme.bank.domain.StandardFeePolicy#feeFor(Money)"), Some(CoverageStatus::Covered));
    assert_eq!(cov("java:com.acme.bank.application.TransferService#load(String)"), Some(CoverageStatus::Covered));
    // No fixture test calls these, and their files were instrumented.
    assert_eq!(cov("java:com.acme.bank.api.AccountController#balance(String)"), Some(CoverageStatus::NotCovered));
    assert_eq!(cov("java:com.acme.bank.domain.Account#freeze()"), Some(CoverageStatus::NotCovered));
    // An interface has no executable line: JaCoCo lists the file, but nothing in it could run.
    assert_eq!(
        cov("java:com.acme.bank.persistence.AccountRepository#findById(String,boolean)"),
        Some(CoverageStatus::NoData)
    );
    // Deleted code is described only by coverage measured at base; the v1 reports were superseded.
    assert_eq!(cov("java:com.acme.bank.api.AccountController#legacyBalance(String)"), Some(CoverageStatus::NoData));

    let reasons = &report.test_selection.fallback_reasons;
    let uncovered = reasons.iter().find(|r| r.code == "CHANGED_CODE_WITHOUT_COVERAGE").unwrap();
    assert_eq!(uncovered.severity, Severity::Medium);
    assert!(uncovered.detail.contains("balance(String)"), "{}", uncovered.detail);
}

#[test]
fn java_migration_forces_the_full_suite_unless_fast_feedback_is_asked_for() {
    let s = setup("java-banking");
    ingest_everything(&s, "java-banking");

    let balanced = analyze_with(&s, SelectionMode::Balanced).test_selection;
    assert_eq!(balanced.decision, SelectionDecision::FullSuite);
    let migration = balanced.fallback_reasons.iter().find(|r| r.code == "MIGRATION_CHANGED").unwrap();
    assert_eq!(migration.severity, Severity::High);
    assert!(migration.detail.contains("V2__account_frozen.sql"), "{}", migration.detail);
    assert_eq!((balanced.selected_units, balanced.total_units), (3, 3));
    // Every unit has recorded runs, so both estimates exist, and the full suite is what runs.
    assert!(balanced.full_runtime_ms.is_some());
    assert_eq!(balanced.selected_runtime_ms, balanced.full_runtime_ms);

    let conservative = analyze_with(&s, SelectionMode::Conservative).test_selection;
    assert_eq!(conservative.decision, SelectionDecision::FullSuite);

    let fast = analyze_with(&s, SelectionMode::FastFeedback).test_selection;
    assert_eq!(fast.decision, SelectionDecision::Selected);
    assert!(fast.fallback_reasons.iter().any(|r| r.code == "MIGRATION_CHANGED"));
    assert!(fast.notes.iter().any(|n| n.contains("run the full suite after these")), "{:?}", fast.notes);
    assert_eq!(
        fast.ordered.first().map(|id| id.as_str()),
        Some("java:com.acme.bank.application.TransferServiceTest#movesMoneyAndChargesFee()"),
        "the changed test has the strongest evidence"
    );
}

#[test]
fn real_same_commit_flips_make_a_test_flaky_and_rank_it_after_reliable_ones() {
    let s = setup("typescript-checkout");
    ingest_everything(&s, "typescript-checkout");
    let report = analyze_with(&s, SelectionMode::Balanced);

    // Recorded: 8 suite runs at v1 (4 with the clock test failing), 4 at v2 (all passing).
    assert_eq!(report.evidence.test_runs, 12);
    let flaky = test(&report, TS_FLAKY).history.clone().unwrap();
    assert_eq!(flaky.reliability, Reliability::Flaky);
    assert_eq!((flaky.runs, flaky.failures, flaky.flaky_commits), (12, 4, 1));
    let cart = test(&report, TS_CART).history.clone().unwrap();
    assert_eq!((cart.reliability, cart.runs, cart.failures), (Reliability::Stable, 12, 0));
    // A test added at v2 has only the 4 v2 runs: enough (MIN_RUNS = 3) to be called stable.
    let spring = test(&report, TS_SPRING).history.clone().unwrap();
    assert_eq!((spring.reliability, spring.runs), (Reliability::Stable, 4));

    // Same tier and a shorter path, yet the flaky unit runs after every reliable one.
    let position = |id: &str| report.test_selection.ordered.iter().position(|t| t.as_str() == id).unwrap();
    assert_eq!(test(&report, TS_FLAKY).tier, test(&report, TS_CART).tier);
    assert!(test(&report, TS_FLAKY).depth < test(&report, TS_CART).depth);
    assert!(position(TS_FLAKY) > position(TS_CART));
}

#[test]
fn ts_modes_select_by_uncertainty_and_estimate_runtime_from_history() {
    let s = setup("typescript-checkout");
    ingest_everything(&s, "typescript-checkout");

    let balanced = analyze_with(&s, SelectionMode::Balanced);
    let selection = &balanced.test_selection;
    // Only medium-severity reasons apply: an unresolved call and an untested change in CartBadge.
    assert!(selection.fallback_reasons.iter().all(|r| r.severity == Severity::Medium), "{selection:#?}");
    assert!(selection.fallback_reasons.iter().any(|r| r.code == "UNRESOLVED_REFERENCE"));
    assert!(selection.fallback_reasons.iter().any(|r| r.code == "CHANGED_CODE_WITHOUT_COVERAGE"));
    assert_eq!(coverage_of(&balanced, "ts:src/ui/CartBadge.tsx#Header"), Some(CoverageStatus::NotCovered));
    assert_eq!(coverage_of(&balanced, "ts:src/pricing/discount.ts#applyDiscount"), Some(CoverageStatus::Covered));
    assert_eq!(selection.decision, SelectionDecision::Selected);
    assert_eq!((selection.selected_units, selection.total_units), (5, 6), "money.test.ts is not affected");
    let (selected, full) = (selection.selected_runtime_ms.unwrap(), selection.full_runtime_ms.unwrap());
    assert!(selected <= full, "{selected} > {full}");
    let file = test(&balanced, "file:src/pricing/discount.test.ts");
    assert_eq!((file.tier, file.coverage_observed), (EvidenceTier::Strong, true));

    let conservative = analyze_with(&s, SelectionMode::Conservative).test_selection;
    assert_eq!(conservative.decision, SelectionDecision::FullSuite);
    assert_eq!(conservative.selected_units, 6);
    assert_eq!(conservative.selected_runtime_ms, Some(full));

    let fast = analyze_with(&s, SelectionMode::FastFeedback).test_selection;
    assert_eq!(fast.decision, SelectionDecision::Selected);
    assert!(fast.notes.iter().any(|n| n.contains("not complete validation")));
}

#[test]
fn runtime_is_not_estimated_when_a_selected_unit_has_no_history() {
    let s = setup("typescript-checkout");
    // Only v1 results: the SPRING20 test added at v2 has never run.
    ingest_junit_of(&s, "typescript-checkout", "v1", "main~1");
    let report = analyze_with(&s, SelectionMode::Balanced);
    assert_eq!(report.evidence.tests_with_history, 5);
    assert!(test(&report, TS_SPRING).history.is_none());
    let selection = &report.test_selection;
    assert_eq!(selection.selected_runtime_ms, None);
    assert_eq!(selection.full_runtime_ms, None);
    assert!(selection.notes.iter().any(|n| n.contains("not estimated")), "{:?}", selection.notes);
    // Without coverage nothing is COVERAGE_OBSERVED and no coverage status is claimed.
    assert!(report.tests.iter().all(|t| !t.coverage_observed));
    assert!(report.changed_symbols.iter().all(|c| c.coverage.is_none()));
}

#[test]
fn coverage_measured_at_base_describes_deleted_code() {
    let s = setup("typescript-checkout");
    ingest_coverage_of(&s, "typescript-checkout", "v1", "main~1");
    let report = analyze_with(&s, SelectionMode::Balanced);
    assert_eq!(report.evidence.coverage_reports_other_commits, 0, "base is not stale");
    // checkout.ts was instrumented at v1 and no test called it.
    assert_eq!(coverage_of(&report, "ts:src/checkout/checkout.ts#newCart"), Some(CoverageStatus::NotCovered));
    assert_eq!(coverage_of(&report, "ts:src/pricing/discount.ts#applyDiscount"), Some(CoverageStatus::Covered));
    // summary.ts did not exist when coverage was measured.
    assert_eq!(coverage_of(&report, "ts:src/checkout/summary.ts#checkoutSummary"), Some(CoverageStatus::NoData));
}

#[test]
fn unknown_test_selector_is_refused_and_foreign_paths_are_counted() {
    let s = setup("typescript-checkout");
    let lcov =
        std::fs::read_to_string(fixture("typescript-checkout").join("evidence/v2/coverage/src/cart.test.ts.lcov"))
            .unwrap();
    let input = CoverageInput { format: CoverageFormat::Lcov, text: &lcov, source: "cart.lcov", test: Some("nope") };
    let error = ingest_coverage(&s.repo, "main", &s.db, input, &Limits::default());
    assert!(matches!(error, Err(AnalysisError::UnknownTest(ref t)) if t == "nope"), "{error:?}");

    // Real JaCoCo output of the Java fixture names no file of this repository.
    let jacoco = std::fs::read_to_string(
        fixture("java-banking").join("evidence/v2/coverage/com.acme.bank.domain.MoneyTest.xml"),
    )
    .unwrap();
    let input = CoverageInput { format: CoverageFormat::Jacoco, text: &jacoco, source: "money.xml", test: None };
    let outcome = ingest_coverage(&s.repo, "main", &s.db, input, &Limits::default()).unwrap();
    assert_eq!((outcome.mapped, outcome.unmapped, outcome.covered_symbols), (0, 11, 0));
    assert_eq!(outcome.unmapped_examples.len(), 5, "examples are bounded");
}

#[test]
fn output_with_evidence_is_deterministic() {
    let s = setup("typescript-checkout");
    ingest_everything(&s, "typescript-checkout");
    let first = serde_json::to_string(&analyze_with(&s, SelectionMode::Balanced)).unwrap();
    let second = serde_json::to_string(&analyze_with(&s, SelectionMode::Balanced)).unwrap();
    assert_eq!(first, second);
}
