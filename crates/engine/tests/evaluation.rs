#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Offline replay evaluation over `fixtures/eval-history`: twelve snapshots whose tests were really
//! run at every snapshot by scripts/collect-eval-history.sh (fixtures/eval-history/evidence). The
//! expected values below are what that committed evidence produces; they were read off a run of
//! the harness, not chosen.

use std::path::{Path, PathBuf};

use ripplepath_engine::evaluation::{
    EvaluationCase, EvaluationError, EvaluationOptions, EvaluationReport, SMALL_SAMPLE_LABEL, evaluate, load_cases,
};
use ripplepath_engine::fixture::build_fixture_repo;
use ripplepath_engine::{
    AnalyzeOptions, CoverageFormat, CoverageInput, Limits, SelectionDecision, SelectionMode, analyze,
    ingest_coverage_batch, ingest_junit,
};

const SNAPSHOTS: usize = 12;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/eval-history")
}

fn rev(version: usize) -> String {
    match SNAPSHOTS - version {
        0 => "main".to_owned(),
        n => format!("main~{n}"),
    }
}

struct Setup {
    dir: tempfile::TempDir,
    repo: PathBuf,
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    let snapshots: Vec<PathBuf> = (1..=SNAPSHOTS).map(|v| fixture().join(format!("v{v}"))).collect();
    let refs: Vec<&Path> = snapshots.iter().map(PathBuf::as_path).collect();
    build_fixture_repo(&refs, &repo).unwrap();
    Setup { dir, repo }
}

/// A database holding the committed evidence of `versions`, ingested oldest first at the revision
/// each was measured at: testwise JaCoCo per test class and the suite's JUnit run.
fn database(s: &Setup, name: &str, versions: impl IntoIterator<Item = usize>) -> PathBuf {
    let db = s.dir.path().join(name);
    add_evidence(s, &db, versions);
    db
}

fn add_evidence(s: &Setup, db: &Path, versions: impl IntoIterator<Item = usize>) {
    for version in versions {
        let dir = fixture().join("evidence").join(format!("v{version}"));
        let mut files: Vec<PathBuf> =
            std::fs::read_dir(dir.join("coverage")).unwrap().map(|e| e.unwrap().path()).collect();
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
        for outcome in ingest_coverage_batch(&s.repo, &rev(version), db, &inputs, &Limits::default()).unwrap() {
            assert_eq!(outcome.unmapped, 0, "v{version}: {:?}", outcome.unmapped_examples);
        }
        let junit = std::fs::read_to_string(dir.join("junit/run-1.xml")).unwrap();
        let outcome = ingest_junit(&s.repo, &rev(version), db, &junit, "run-1.xml", &Limits::default()).unwrap();
        assert_eq!(outcome.unmapped, 0, "v{version}: {:?}", outcome.unmapped_examples);
    }
}

fn cases(names: &[&str]) -> Vec<EvaluationCase> {
    let all = load_cases(&fixture().join("cases.json")).unwrap();
    if names.is_empty() {
        return all;
    }
    all.into_iter().filter(|c| names.contains(&c.name.as_str())).collect()
}

fn run(s: &Setup, db: &Path, cases: &[EvaluationCase], modes: &[SelectionMode]) -> EvaluationReport {
    let mut options = EvaluationOptions::new(&s.repo, db);
    options.modes = modes.to_vec();
    evaluate(&options, cases).unwrap()
}

const ALL_MODES: [SelectionMode; 3] =
    [SelectionMode::Conservative, SelectionMode::Balanced, SelectionMode::FastFeedback];

#[test]
fn replay_of_the_real_history_scores_what_was_measured() {
    let s = setup();
    let db = database(&s, "all.db", 1..=SNAPSHOTS);
    let report = run(&s, &db, &cases(&[]), &ALL_MODES);

    assert_eq!(report.evaluation_schema_version, 1);
    let sample = &report.sample;
    assert_eq!((sample.cases, sample.failing_cases, sample.failed_tests), (11, 5, 15));
    assert_eq!(sample.label.as_deref(), Some(SMALL_SAMPLE_LABEL));

    // No change in this history raised a fallback reason, so the three modes decide alike; fast
    // feedback never had more than 10 recommendations to cut.
    for aggregate in &report.modes {
        assert_eq!((aggregate.failed_tests, aggregate.caught_failures, aggregate.missed_failures), (15, 10, 5));
        assert_eq!(aggregate.failing_test_recall, Some(0.6667));
        assert_eq!((aggregate.failing_cases, aggregate.failing_cases_fully_caught), (5, 4));
        assert_eq!(aggregate.full_suite_fallbacks, 0);
        assert_eq!(aggregate.mean_selected_test_reduction, Some(0.6372));
        assert_eq!((aggregate.runtime_cases, aggregate.mean_runtime_reduction), (11, Some(0.6569)));
    }

    // (case, selected, total, failed, caught, position of the first failing test)
    let expected: [(&str, usize, usize, usize, usize, Option<usize>); 11] = [
        ("v1..v2", 7, 17, 0, 0, None),
        ("v2..v3", 12, 17, 3, 3, Some(3)),
        ("v3..v4", 15, 20, 0, 0, None),
        ("v4..v5", 8, 20, 2, 2, Some(1)),
        ("v5..v6", 11, 20, 0, 0, None),
        ("v6..v7", 0, 20, 5, 0, None),
        ("v7..v8", 0, 20, 0, 0, None),
        ("v8..v9", 7, 20, 4, 4, Some(1)),
        ("v9..v10", 7, 20, 0, 0, None),
        ("v10..v11", 4, 20, 1, 1, Some(2)),
        ("v11..v12", 6, 22, 0, 0, None),
    ];
    for (case, (name, selected, total, failed, caught, first)) in report.cases.iter().zip(expected) {
        assert_eq!(case.name, name);
        for result in &case.modes {
            let s = &result.score;
            assert_eq!(result.decision, SelectionDecision::Selected, "{name}");
            assert!(result.fallback_reasons.is_empty(), "{name}: {:?}", result.fallback_reasons);
            assert_eq!(
                (s.selected_tests, s.total_tests, s.failed_tests, s.caught_failures, s.first_failure_position),
                (selected, total, failed, caught, first),
                "{name} {:?}",
                result.mode
            );
        }
    }

    let case = |name: &str| report.cases.iter().find(|c| c.name == name).unwrap();
    // v4..v5 breaks BulkDiscount, reached from checkout only through DiscountPolicy (OVERRIDES).
    let bulk = &case("v4..v5").modes[1].score;
    assert_eq!(bulk.time_to_first_failure_ms, Some(39));
    assert_eq!(bulk.runtime_reduction, Some(0.8193));
    // v6..v7 breaks tax-rates.properties: no code changed, no reason fired, nothing was selected,
    // and every failure was missed. This is the miss the evaluation exists to expose.
    let resource = case("v6..v7");
    assert_eq!(
        resource.observed.failed,
        vec![
            "java:com.acme.shop.application.CheckoutServiceTest#bulkOrdersGetTheBulkDiscount()",
            "java:com.acme.shop.application.CheckoutServiceTest#freeShippingFromThreshold()",
            "java:com.acme.shop.application.CheckoutServiceTest#reservesStockForEveryItem()",
            "java:com.acme.shop.application.CheckoutServiceTest#totalsDiscountTaxAndShipping()",
            "java:com.acme.shop.pricing.TaxCalculatorTest#loadsRatesFromResource()",
        ]
    );
    assert_eq!(resource.modes[1].score.missed_failures, resource.observed.failed);
    assert_eq!(resource.modes[1].score.failing_test_recall, Some(0.0));

    // Every case sees exactly the JUnit runs of base and its ancestors: one per earlier snapshot.
    for (i, case) in report.cases.iter().enumerate() {
        assert_eq!(case.visible_evidence.test_runs, i + 1, "{}", case.name);
        assert_eq!(case.visible_evidence.coverage_reports_other_commits, 0, "{}", case.name);
    }
}

#[test]
fn evidence_recorded_at_head_or_later_cannot_affect_a_case() {
    let s = setup();
    // v4..v5: base is v4. The clean database never saw anything after v4; the other one also
    // holds the runs and coverage of v5 (head) and v6 (later).
    let up_to_base = database(&s, "prior.db", 1..=4);
    let everything = s.dir.path().join("through-v6.db");
    std::fs::copy(&up_to_base, &everything).unwrap();
    add_evidence(&s, &everything, 5..=6);
    let case = cases(&["v4..v5"]);

    let leaky_db = run(&s, &everything, &case, &ALL_MODES);
    let clean_db = run(&s, &up_to_base, &case, &ALL_MODES);
    assert_eq!(leaky_db.cases, clean_db.cases, "evidence from v5 and v6 changed the v4..v5 replay");

    // The test is not vacuous: without the ancestry restriction the same database feeds the
    // analysis head's and later runs and coverage, and the selection itself changes.
    let mut unrestricted = AnalyzeOptions::new(&s.repo, rev(4), rev(5));
    unrestricted.db = Some(everything.clone());
    unrestricted.mode = Some(SelectionMode::Conservative);
    let leaked = analyze(&unrestricted).unwrap();
    assert_eq!(leaked.evidence.test_runs, 6);
    assert_eq!(leaky_db.cases[0].visible_evidence.test_runs, 4);
    assert!(leaked.test_selection.fallback_reasons.iter().any(|r| r.code == "STALE_COVERAGE"));
    assert_eq!(leaked.test_selection.decision, SelectionDecision::FullSuite);
    assert_eq!(leaky_db.cases[0].modes[0].decision, SelectionDecision::Selected);

    // The harness counts the units the analysis says it selects.
    let mut restricted = unrestricted.clone();
    restricted.mode = Some(SelectionMode::Balanced);
    restricted.evidence_commits = Some(
        ["main~11", "main~10", "main~9", "main~8"]
            .iter()
            .map(|spec| ripplepath_git::Repo::open(&s.repo).unwrap().resolve(spec).unwrap().commit.unwrap())
            .collect(),
    );
    let analysed = analyze(&restricted).unwrap();
    assert_eq!(analysed.summary.tests_recommended, leaky_db.cases[0].modes[1].score.selected_tests);

    // Same inputs, same bytes.
    assert_eq!(serde_json::to_string(&leaky_db).unwrap(), serde_json::to_string(&clean_db).unwrap());
}

#[test]
fn a_case_whose_head_is_not_after_base_is_refused() {
    let s = setup();
    let db = s.dir.path().join("empty.db");
    let mut backwards = cases(&["v1..v2"]);
    let first = &mut backwards[0];
    std::mem::swap(&mut first.base, &mut first.head);
    let error = evaluate(&EvaluationOptions::new(&s.repo, &db), &backwards).unwrap_err();
    assert!(matches!(error, EvaluationError::HeadNotAfterBase { .. }), "{error}");

    let mut same = cases(&["v1..v2"]);
    same[0].head = same[0].base.clone();
    let error = evaluate(&EvaluationOptions::new(&s.repo, &db), &same).unwrap_err();
    assert!(matches!(error, EvaluationError::HeadNotAfterBase { .. }), "{error}");

    let mut unobserved = cases(&["v1..v2"]);
    unobserved[0].junit.clear();
    let error = evaluate(&EvaluationOptions::new(&s.repo, &db), &unobserved).unwrap_err();
    assert!(matches!(error, EvaluationError::NoObservedOutcome { .. }), "{error}");

    assert!(matches!(evaluate(&EvaluationOptions::new(&s.repo, &db), &[]), Err(EvaluationError::NoCases)));
}
