#![allow(clippy::unwrap_used, clippy::expect_used)]

//! End-to-end: configuration from the base revision, architecture delta, owners, API surface, risk
//! decomposition and merge policy.

use std::path::{Path, PathBuf};

use ripplepath_core::EdgeKind;
use ripplepath_engine::api_surface::ApiChangeKind;
use ripplepath_engine::architecture::DeltaStatus;
use ripplepath_engine::config::Gate;
use ripplepath_engine::fixture::build_fixture_repo;
use ripplepath_engine::policy::{GateStatus, PolicyResult};
use ripplepath_engine::risk::{RiskLevel, SignalId};
use ripplepath_engine::{
    AnalysisReport, AnalyzeOptions, ConfigChange, ConfigSource, SelectionMode, analyze, check_architecture,
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

/// Two snapshots written from (path, content) lists.
fn repo_from(v1: &[(&str, &str)], v2: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let mut roots = Vec::new();
    for (name, files) in [("v1", v1), ("v2", v2)] {
        let root = dir.path().join(name);
        std::fs::create_dir_all(&root).unwrap();
        for (path, content) in files {
            let target = root.join(path);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(target, content).unwrap();
        }
        roots.push(root);
    }
    let repo = dir.path().join("repo");
    build_fixture_repo(&[&roots[0], &roots[1]], &repo).unwrap();
    (dir, repo)
}

fn gate(report: &AnalysisReport, gate: Gate) -> GateStatus {
    report.policy.gates.iter().find(|g| g.gate == gate).unwrap().status
}

const LAYERS: &str = "version: 1\narchitecture:\n  layers:\n    - name: api\n      match: [\"api/**\"]\n    - name: domain\n      match: [\"domain/**\"]\n  rules:\n    - from: domain\n      deny: [api]\n";
const API: &str = "package api;\npublic class Api {\n  public static String name() { return \"a\"; }\n}\n";
const DOMAIN_CLEAN: &str = "package domain;\npublic class Order {\n  public String id() { return \"o\"; }\n}\n";
const DOMAIN_VIOLATING: &str =
    "package domain;\nimport api.Api;\npublic class Order {\n  public String id() { return Api.name(); }\n}\n";

#[test]
fn banking_change_introduces_a_domain_to_api_violation() {
    let (_dir, report) = analyze_banking();
    assert_eq!(report.config.source, ConfigSource::BaseRevision);
    assert_eq!(report.config.head_change, ConfigChange::Unchanged);

    let arch = &report.architecture;
    assert!(arch.configured);
    assert_eq!(arch.summary.new_violations, 2);
    assert_eq!(arch.summary.pre_existing_violations, 0);
    let found: Vec<(DeltaStatus, &str, &str, EdgeKind, u32)> = arch
        .violations
        .iter()
        .map(|v| (v.status, v.edge.from.as_str(), v.edge.to.as_str(), v.edge.kind, v.edge.line))
        .collect();
    assert_eq!(
        found,
        vec![
            (
                DeltaStatus::New,
                "file:src/main/java/com/acme/bank/domain/Account.java",
                "java:com.acme.bank.api.ApiErrors",
                EdgeKind::Imports,
                3
            ),
            (
                DeltaStatus::New,
                "java:com.acme.bank.domain.Account#withdraw(Money)",
                "java:com.acme.bank.api.ApiErrors#accountFrozen(String)",
                EdgeKind::Calls,
                25
            ),
        ]
    );
    assert!(arch.violations.iter().all(|v| v.from_layer == "domain" && v.to_layer == "api" && v.rule == 0));
    // domain → api closes a loop through api → application → domain (and persistence).
    assert_eq!(arch.summary.new_cycles, 1);
    assert_eq!(arch.cycles[0].layers, vec!["api", "application", "domain", "persistence"]);

    assert_eq!(report.policy.result, PolicyResult::Fail);
    assert_eq!(gate(&report, Gate::NewArchitectureViolation), GateStatus::Fail);
    assert_eq!(gate(&report, Gate::NewCycle), GateStatus::Fail);
    assert_eq!(gate(&report, Gate::BreakingApiChange), GateStatus::Warn);
    assert_eq!(gate(&report, Gate::ConfigInvalid), GateStatus::Pass);
}

#[test]
fn banking_public_api_changes() {
    let (_dir, report) = analyze_banking();
    let api: Vec<(ApiChangeKind, &str)> = report.api_surface.changes.iter().map(|c| (c.kind, c.id.as_str())).collect();
    assert_eq!(
        api,
        vec![
            (ApiChangeKind::Removed, "java:com.acme.bank.api.AccountController#legacyBalance(String)"),
            (
                ApiChangeKind::SignatureChanged,
                "java:com.acme.bank.persistence.AccountRepository#findById(String,boolean)"
            ),
            (
                ApiChangeKind::SignatureChanged,
                "java:com.acme.bank.persistence.InMemoryAccountRepository#findById(String,boolean)"
            ),
            (ApiChangeKind::Added, "java:com.acme.bank.domain.Account#freeze()"),
        ],
        "ApiErrors is package-private, the new field is private"
    );
    assert_eq!(report.api_surface.breaking, 3);
}

#[test]
fn banking_risk_decomposition_is_exact() {
    let (_dir, report) = analyze_banking();
    let risk = &report.risk;
    let scored: Vec<(SignalId, u64, u32)> =
        risk.signals.iter().filter(|s| s.points > 0).map(|s| (s.id, s.value, s.points)).collect();
    assert_eq!(
        scored,
        vec![
            (SignalId::PublicApiChanged, 3, 24),
            (SignalId::MigrationChanged, 1, 15),
            (SignalId::BlastRadius, 3, 5),
            (SignalId::ChangedSymbolCentrality, 3, 5),
            // Account.java, StandardFeePolicy.java and the migration; Money.java only gained a comment.
            (SignalId::CriticalPathChanged, 3, 20),
            (SignalId::NewArchitectureViolation, 2, 20),
            (SignalId::NewLayerCycle, 1, 10),
        ]
    );
    assert_eq!(risk.score, 99);
    assert_eq!(risk.level, RiskLevel::Critical);
    assert_eq!(risk.model_version, 1);
    assert!(risk.interpretation.contains("NOT a probability"));
    let unevaluated: Vec<SignalId> = risk.signals.iter().filter(|s| !s.evaluated).map(|s| s.id).collect();
    assert_eq!(unevaluated, vec![SignalId::ChangedCodeWithoutCoverage, SignalId::FlakyImpactedTests]);
}

#[test]
fn configuration_comes_from_base_so_a_change_cannot_weaken_its_own_rules() {
    let weakened = "version: 1\narchitecture:\n  layers:\n    - name: api\n      match: [\"api/**\"]\n    - name: domain\n      match: [\"domain/**\"]\npolicy:\n  fail_on: []\n  warn_on: []\n";
    let (_dir, repo) = repo_from(
        &[("ripplepath.yml", LAYERS), ("api/Api.java", API), ("domain/Order.java", DOMAIN_CLEAN)],
        &[("ripplepath.yml", weakened), ("api/Api.java", API), ("domain/Order.java", DOMAIN_VIOLATING)],
    );
    let report = analyze(&AnalyzeOptions::new(&repo, "main~1", "main")).unwrap();
    assert_eq!(report.config.head_change, ConfigChange::Modified);
    assert_eq!(report.architecture.summary.new_violations, 2, "base rules still apply");
    assert_eq!(gate(&report, Gate::NewArchitectureViolation), GateStatus::Warn, "base default policy warns");
    assert_eq!(gate(&report, Gate::ConfigChanged), GateStatus::Warn);
    let signal = report.risk.signals.iter().find(|s| s.id == SignalId::ConfigChanged).unwrap();
    assert_eq!(signal.points, 5);
}

#[test]
fn invalid_configuration_falls_back_to_defaults_and_fails_the_policy() {
    let typo = "version: 1\npolcy:\n  fail_on: [new_cycle]\n";
    let (_dir, repo) = repo_from(
        &[("ripplepath.yml", typo), ("domain/Order.java", DOMAIN_CLEAN)],
        &[("ripplepath.yml", typo), ("domain/Order.java", DOMAIN_CLEAN.replace("\"o\"", "\"p\"").as_str())],
    );
    let report = analyze(&AnalyzeOptions::new(&repo, "main~1", "main")).unwrap();
    assert_eq!(report.config.source, ConfigSource::InvalidUsingDefaults);
    assert!(report.config.errors[0].contains("polcy"), "{:?}", report.config.errors);
    assert!(report.config.head_errors.is_empty(), "an unchanged file is reported once");
    assert_eq!(gate(&report, Gate::ConfigInvalid), GateStatus::Fail);
    assert_eq!(report.policy.result, PolicyResult::Fail);
}

#[test]
fn test_mode_comes_from_configuration_unless_overridden() {
    let config = "version: 1\ntests:\n  mode: fast_feedback\n";
    let (_dir, repo) = repo_from(
        &[("ripplepath.yml", config), ("domain/Order.java", DOMAIN_CLEAN)],
        &[("ripplepath.yml", config), ("domain/Order.java", DOMAIN_CLEAN.replace("\"o\"", "\"p\"").as_str())],
    );
    let report = analyze(&AnalyzeOptions::new(&repo, "main~1", "main")).unwrap();
    assert_eq!(report.test_selection.mode, SelectionMode::FastFeedback);
    let mut options = AnalyzeOptions::new(&repo, "main~1", "main");
    options.mode = Some(SelectionMode::Conservative);
    assert_eq!(analyze(&options).unwrap().test_selection.mode, SelectionMode::Conservative);
}

#[test]
fn codeowners_from_head_route_changed_and_impacted_files() {
    let owners = "* @everyone\n/domain/ @domain-team\n";
    let caller =
        "package api;\nimport domain.Order;\npublic class Api {\n  public String name(Order o) { return o.id(); }\n}\n";
    let (_dir, repo) = repo_from(
        &[(".github/CODEOWNERS", owners), ("api/Api.java", caller), ("domain/Order.java", DOMAIN_CLEAN)],
        &[
            (".github/CODEOWNERS", owners),
            ("api/Api.java", caller),
            ("domain/Order.java", DOMAIN_CLEAN.replace("\"o\"", "\"p\"").as_str()),
        ],
    );
    let report = analyze(&AnalyzeOptions::new(&repo, "main~1", "main")).unwrap();
    let o = &report.owners;
    assert_eq!(o.source.as_deref(), Some(".github/CODEOWNERS"));
    assert!(!o.changed_in_head);
    let rows: Vec<(&str, &str)> =
        o.files.iter().map(|f| (f.path.as_str(), f.owners.first().map_or("", String::as_str))).collect();
    assert_eq!(rows, vec![("domain/Order.java", "@domain-team"), ("api/Api.java", "@everyone")]);
    assert!(o.note.contains("not authorization"));
}

#[test]
fn deleting_a_test_that_reaches_critical_code_is_gated() {
    let config = "version: 1\ncritical: [\"domain/**\"]\npolicy:\n  fail_on: [removed_test_on_critical_path]\n";
    let test = "package t;\nimport domain.Order;\nimport org.junit.jupiter.api.Test;\nclass OrderTest {\n  @Test void id() { new Order().id(); }\n  @Test void other() { }\n}\n";
    let test_after = "package t;\nimport domain.Order;\nimport org.junit.jupiter.api.Test;\nclass OrderTest {\n  @Test void other() { }\n}\n";
    let (_dir, repo) = repo_from(
        &[("ripplepath.yml", config), ("domain/Order.java", DOMAIN_CLEAN), ("test/t/OrderTest.java", test)],
        &[("ripplepath.yml", config), ("domain/Order.java", DOMAIN_CLEAN), ("test/t/OrderTest.java", test_after)],
    );
    let report = analyze(&AnalyzeOptions::new(&repo, "main~1", "main")).unwrap();
    assert_eq!(gate(&report, Gate::RemovedTestOnCriticalPath), GateStatus::Fail);
    let deleted = report.risk.signals.iter().find(|s| s.id == SignalId::DeletedTests).unwrap();
    assert_eq!(deleted.evidence, vec!["java:t.OrderTest#id()".to_owned()]);
    assert_eq!(deleted.points, 5);
}

#[test]
fn architecture_check_reports_the_state_of_one_revision() {
    let dir = tempfile::tempdir().unwrap();
    let root = fixture("java-banking");
    build_fixture_repo(&[&root.join("v1"), &root.join("v2")], dir.path()).unwrap();
    let limits = ripplepath_engine::Limits::default();
    let before = check_architecture(dir.path(), "main~1", &limits).unwrap();
    assert!(before.config_found);
    assert!(before.violations.is_empty() && before.cycles.is_empty());
    let after = check_architecture(dir.path(), "main", &limits).unwrap();
    assert_eq!(after.violations.len(), 2);
    assert_eq!(after.cycles.len(), 1);
    assert_eq!(
        after.layers.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>(),
        ["api", "application", "domain", "persistence"]
    );
}
