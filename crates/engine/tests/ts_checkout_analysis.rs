#![allow(clippy::unwrap_used, clippy::expect_used)]

//! End-to-end on the TypeScript fixture: real Git repository, base → head analysis.

use std::path::Path;

use ripplepath_engine::fixture::build_fixture_repo;
use ripplepath_engine::{AnalysisReport, AnalyzeOptions, ChangeKind, FileStatus, TestReason, UncertaintyKind, analyze};

fn analyze_checkout() -> (tempfile::TempDir, AnalysisReport) {
    let dir = tempfile::tempdir().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/typescript-checkout");
    build_fixture_repo(&[&root.join("v1"), &root.join("v2")], dir.path()).unwrap();
    let report = analyze(&AnalyzeOptions::new(dir.path(), "main~1", "main")).unwrap();
    (dir, report)
}

#[test]
fn file_rename_is_detected_and_symbols_are_paired_as_probable_moves() {
    let (_dir, report) = analyze_checkout();
    let renamed = report.files.iter().find(|f| f.path == "src/checkout/summary.ts").unwrap();
    assert_eq!(renamed.status, FileStatus::Renamed);
    assert_eq!(renamed.old_path.as_deref(), Some("src/checkout/checkout.ts"));

    let added =
        report.changed_symbols.iter().find(|s| s.id.as_str() == "ts:src/checkout/summary.ts#checkoutSummary").unwrap();
    assert_eq!(added.change, ChangeKind::Added);
    assert_eq!(added.probable_move.as_ref().map(|m| m.as_str()), Some("ts:src/checkout/checkout.ts#checkoutSummary"));

    let deleted =
        report.changed_symbols.iter().find(|s| s.id.as_str() == "ts:src/checkout/checkout.ts#newCart").unwrap();
    assert_eq!(deleted.change, ChangeKind::Deleted);
    assert_eq!(deleted.probable_move, None);
}

#[test]
fn change_propagates_through_interface_dispatch_to_ui_and_tests() {
    let (_dir, report) = analyze_checkout();
    let depth = |id: &str| report.impacted_symbols.iter().find(|s| s.id.as_str() == id).map(|s| s.depth);

    assert_eq!(depth("ts:src/pricing/pricing-service.ts#PricingService.quote"), Some(1));
    assert_eq!(depth("ts:src/pricing/price-source.ts#PriceSource.quote"), Some(2), "dispatch to the interface");
    assert_eq!(depth("ts:src/cart.ts#Cart.total"), Some(3));
    assert_eq!(
        depth("ts:src/checkout/summary.ts#checkoutSummary"),
        None,
        "added in head, so it is changed, not impacted"
    );
    assert!(depth("ts:src/ui/CartBadge.tsx#CartBadge").is_some());

    let quote = report
        .impacted_symbols
        .iter()
        .find(|s| s.id.as_str() == "ts:src/pricing/price-source.ts#PriceSource.quote")
        .unwrap();
    assert!(quote.path.last().unwrap().via_dispatch);
}

#[test]
fn recommends_affected_tests_and_skips_unrelated_ones() {
    let (_dir, report) = analyze_checkout();
    let test = |id: &str| report.tests.iter().find(|t| t.id.as_str() == id);

    let spring = test("ts:src/pricing/discount.test.ts#test:SPRING20 > takes 20% off").unwrap();
    assert_eq!(spring.reason, TestReason::ChangedTest);

    let welcome = test("ts:src/pricing/discount.test.ts#test:applyDiscount > takes 10% off with WELCOME10").unwrap();
    assert_eq!(welcome.reason, TestReason::StaticPath);
    assert_eq!(welcome.depth, 1);

    let cart = test("ts:src/cart.test.ts#test:Cart > totals line items").unwrap();
    assert_eq!(cart.reason, TestReason::StaticPath);
    assert!(cart.path.iter().any(|hop| hop.via_dispatch), "explained through interface dispatch");

    assert!(test("ts:src/money.test.ts#test:formats with two decimals").is_none(), "money did not change");
    assert_eq!(report.summary.tests_total, 5);
    assert_eq!(report.summary.tests_recommended, 4);
    // A new `describe` block is not a change to the other tests in the file.
    assert!(test("file:src/pricing/discount.test.ts").is_none());
    assert!(
        !report.impacted_symbols.iter().any(|s| s.kind == ripplepath_core::SymbolKind::File && !s.is_test),
        "imports alone do not make files impacted; only test files with suite-level code can be"
    );
}

#[test]
fn untyped_call_is_reported_as_uncertainty() {
    let (_dir, report) = analyze_checkout();
    let item = report
        .uncertainty
        .iter()
        .find(|u| u.kind == UncertaintyKind::UnresolvedReference && u.file.as_deref() == Some("src/ui/CartBadge.tsx"))
        .unwrap();
    assert!(item.detail.contains("call total"), "{}", item.detail);
}

#[test]
fn output_is_deterministic() {
    let (_a, first) = analyze_checkout();
    let (_b, second) = analyze_checkout();
    assert_eq!(serde_json::to_string(&first).unwrap(), serde_json::to_string(&second).unwrap());
}
