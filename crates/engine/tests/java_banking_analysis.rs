#![allow(clippy::unwrap_used, clippy::expect_used)]

//! End-to-end: fixture snapshots → real Git repository → analysis report.

use std::path::{Path, PathBuf};

use ripplepath_core::{EdgeKind, Evidence};
use ripplepath_engine::fixture::build_fixture_repo;
use ripplepath_engine::{
    AnalysisReport, AnalyzeOptions, ChangeKind, GraphSide, Severity, TestReason, UncertaintyKind, analyze,
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(name)
}

fn analyze_banking() -> (tempfile::TempDir, AnalysisReport) {
    let dir = tempfile::tempdir().unwrap();
    let root = fixture("java-banking");
    build_fixture_repo(&[&root.join("v1"), &root.join("v2")], dir.path()).unwrap();
    let report = analyze(&AnalyzeOptions::new(dir.path(), "main~1", "main")).unwrap();
    (dir, report)
}

fn change_of<'a>(report: &'a AnalysisReport, id: &str) -> Option<&'a ripplepath_engine::ChangedSymbol> {
    report.changed_symbols.iter().find(|s| s.id.as_str() == id)
}

#[test]
fn classifies_symbol_level_changes() {
    let (_dir, report) = analyze_banking();
    let kind = |id: &str| change_of(&report, id).map(|s| s.change);

    assert_eq!(kind("java:com.acme.bank.domain.StandardFeePolicy#feeFor(Money)"), Some(ChangeKind::Modified));
    assert_eq!(kind("java:com.acme.bank.domain.Account#withdraw(Money)"), Some(ChangeKind::Modified));
    assert_eq!(kind("java:com.acme.bank.domain.Account#freeze()"), Some(ChangeKind::Added));
    assert_eq!(kind("java:com.acme.bank.domain.Account#frozen"), Some(ChangeKind::Added));
    assert_eq!(kind("java:com.acme.bank.api.ApiErrors"), Some(ChangeKind::Added));
    assert_eq!(kind("java:com.acme.bank.api.AccountController#legacyBalance(String)"), Some(ChangeKind::Deleted));

    let signature =
        change_of(&report, "java:com.acme.bank.persistence.AccountRepository#findById(String,boolean)").unwrap();
    assert_eq!(signature.change, ChangeKind::SignatureChanged);
    assert_eq!(
        signature.previous_id.as_ref().map(|i| i.as_str()),
        Some("java:com.acme.bank.persistence.AccountRepository#findById(String)")
    );

    // Comment-only edit and untouched members are not changes; editing a method does not mark the class.
    assert_eq!(kind("java:com.acme.bank.domain.Money#isNegative()"), None);
    assert_eq!(kind("java:com.acme.bank.domain.Money"), None);
    assert_eq!(kind("java:com.acme.bank.domain.StandardFeePolicy"), None);
    assert_eq!(kind("java:com.acme.bank.domain.Account#deposit(Money)"), None);
}

#[test]
fn explains_impact_through_dispatch_and_calls() {
    let (_dir, report) = analyze_banking();
    let impacted = |id: &str| report.impacted_symbols.iter().find(|s| s.id.as_str() == id);

    // StandardFeePolicy.feeFor changed → FeePolicy.feeFor (dispatch) → TransferService.transfer
    // → TransferController.transfer.
    let decl = impacted("java:com.acme.bank.domain.FeePolicy#feeFor(Money)").unwrap();
    assert_eq!(decl.depth, 1);
    assert!(decl.path[0].via_dispatch);

    let controller =
        impacted("java:com.acme.bank.api.TransferController#transfer(String,String,String,String)").unwrap();
    assert_eq!(controller.graph, GraphSide::Head);
    assert_eq!(controller.weakest_evidence, Evidence::ResolvedExact);
    let hops: Vec<(&str, EdgeKind)> = controller.path.iter().map(|h| (h.symbol.as_str(), h.edge.kind)).collect();
    assert_eq!(
        hops.last(),
        Some(&("java:com.acme.bank.api.TransferController#transfer(String,String,String,String)", EdgeKind::Calls))
    );
    assert!(
        controller.path.iter().all(|h| !h.edge.file.is_empty() && h.edge.line > 0),
        "every hop carries source evidence"
    );

    // Changed symbols are never also listed as impacted.
    for item in &report.impacted_symbols {
        assert!(change_of(&report, item.id.as_str()).is_none(), "{} is both changed and impacted", item.id);
    }
}

#[test]
fn deleted_symbols_are_traced_on_the_base_graph() {
    let dir = tempfile::tempdir().unwrap();
    let base = "package p;\npublic class Lib {\n  public static int old() { return 1; }\n}\n";
    let user = "package p;\nclass User {\n  int run() { return Lib.old(); }\n}\n";
    let v1 = dir.path().join("v1");
    let v2 = dir.path().join("v2");
    for (root, lib) in [(&v1, base), (&v2, "package p;\npublic class Lib {\n}\n")] {
        std::fs::create_dir_all(root.join("p")).unwrap();
        std::fs::write(root.join("p/Lib.java"), lib).unwrap();
        std::fs::write(root.join("p/User.java"), user).unwrap();
    }
    let repo = dir.path().join("repo");
    build_fixture_repo(&[&v1, &v2], &repo).unwrap();
    let report = analyze(&AnalyzeOptions::new(&repo, "main~1", "main")).unwrap();

    assert_eq!(change_of(&report, "java:p.Lib#old()").map(|s| s.change), Some(ChangeKind::Deleted));
    let user_run = report.impacted_symbols.iter().find(|s| s.id.as_str() == "java:p.User#run()").unwrap();
    assert_eq!(user_run.graph, GraphSide::Base, "the caller only depends on the deleted method in base");
}

#[test]
fn recommends_tests_with_paths() {
    let (_dir, report) = analyze_banking();
    let test = |id: &str| report.tests.iter().find(|t| t.id.as_str() == id);

    let service_test = test("java:com.acme.bank.application.TransferServiceTest#movesMoneyAndChargesFee()").unwrap();
    // The test body itself changed (findById call updated), which outranks any static path.
    assert_eq!(service_test.reason, TestReason::ChangedTest);

    let controller_test =
        test("java:com.acme.bank.api.TransferControllerTest#returnsOkOnSuccessfulTransfer()").unwrap();
    assert_eq!(controller_test.reason, TestReason::StaticPath);
    assert!(controller_test.depth >= 2);
    assert!(!controller_test.path.is_empty());

    // MoneyTest only touches Money, which did not change.
    assert!(test("java:com.acme.bank.domain.MoneyTest#subtractingMoreThanBalanceIsNegative()").is_none());
    assert_eq!(report.summary.tests_total, 3);
}

#[test]
fn unsupported_changed_files_are_visible_as_uncertainty() {
    let (_dir, report) = analyze_banking();
    let migration = report
        .uncertainty
        .iter()
        .find(|u| u.file.as_deref() == Some("src/main/resources/db/migration/V2__account_frozen.sql"))
        .unwrap();
    assert_eq!(migration.kind, UncertaintyKind::UnsupportedLanguage);
    assert_eq!(migration.severity, Severity::Low);
    assert!(!report.uncertainty.iter().any(|u| u.kind == UncertaintyKind::UnresolvedReference));
}

#[test]
fn hunks_map_to_innermost_symbols() {
    let (_dir, report) = analyze_banking();
    let file =
        report.files.iter().find(|f| f.path == "src/main/java/com/acme/bank/domain/StandardFeePolicy.java").unwrap();
    assert!(!file.hunks.is_empty());
    for hunk in &file.hunks {
        assert_eq!(
            hunk.head_symbols.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
            vec!["java:com.acme.bank.domain.StandardFeePolicy#feeFor(Money)"]
        );
    }
}

#[test]
fn output_is_byte_identical_across_runs() {
    let (_a, first) = analyze_banking();
    let (_b, second) = analyze_banking();
    assert_eq!(serde_json::to_string(&first).unwrap(), serde_json::to_string(&second).unwrap());
    assert_eq!(first.head.commit, second.head.commit, "fixture commits are reproducible");
}

#[test]
fn graph_slice_contains_changed_and_impacted_nodes_and_their_edges() {
    let (_dir, report) = analyze_banking();
    let ids: Vec<&str> = report.graph.nodes.iter().map(|n| n.id.as_str()).collect();
    assert!(ids.contains(&"java:com.acme.bank.domain.StandardFeePolicy#feeFor(Money)"));
    assert!(ids.contains(&"java:com.acme.bank.domain.FeePolicy#feeFor(Money)"));
    for edge in &report.graph.edges {
        assert!(ids.contains(&edge.from.as_str()) && ids.contains(&edge.to.as_str()));
    }
    assert!(!report.graph.clamped);
}

#[test]
fn changed_lifecycle_method_recommends_its_whole_test_class() {
    let dir = tempfile::tempdir().unwrap();
    let test_v1 = "package p;\nimport org.junit.jupiter.api.*;\nclass LibTest {\n  @BeforeEach void setUp() { }\n  @Test void a() { }\n  @Test void b() { }\n}\n";
    let test_v2 = test_v1.replace("void setUp() { }", "void setUp() { System.gc(); }");
    let (v1, v2) = (dir.path().join("v1"), dir.path().join("v2"));
    for (root, text) in [(&v1, test_v1.to_owned()), (&v2, test_v2)] {
        std::fs::create_dir_all(root.join("p")).unwrap();
        std::fs::write(root.join("p/LibTest.java"), text).unwrap();
    }
    let repo = dir.path().join("repo");
    build_fixture_repo(&[&v1, &v2], &repo).unwrap();
    let report = analyze(&AnalyzeOptions::new(&repo, "main~1", "main")).unwrap();

    let ids: Vec<&str> = report.tests.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, vec!["java:p.LibTest"], "setUp runs before every test, so the class is recommended");
    assert_eq!(report.tests[0].reason, TestReason::ChangedTest);
    assert_eq!(report.tests[0].root.as_str(), "java:p.LibTest#setUp()");
}
