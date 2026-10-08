#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Single-revision queries agree with the change analysis they are meant to predict.

use std::path::Path;

use ripplepath_core::SymbolId;
use ripplepath_engine::explore::load_revision;
use ripplepath_engine::fixture::build_fixture_repo;
use ripplepath_engine::{AnalyzeOptions, FactCache, Limits, analyze};
use ripplepath_graph::ImpactOptions;

#[test]
fn what_if_on_a_modified_symbol_matches_the_committed_change() {
    let dir = tempfile::tempdir().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/java-banking");
    build_fixture_repo(&[&root.join("v1"), &root.join("v2")], dir.path()).unwrap();

    // v1 → v2 modifies StandardFeePolicy#feeFor's body. Asking "what if" on v1 must reach every
    // symbol the real analysis attributes to that root, with the same explaining paths.
    let fee = SymbolId::new("java:com.acme.bank.domain.StandardFeePolicy#feeFor(Money)");
    let view = load_revision(dir.path(), "main~1", &mut FactCache::default(), &Limits::default()).unwrap();
    let what_if = view.what_if(&fee, ImpactOptions::default()).unwrap();
    let report = analyze(&AnalyzeOptions::new(dir.path(), "main~1", "main")).unwrap();

    let from_fee: Vec<_> = report.impacted_symbols.iter().filter(|s| s.root == fee).collect();
    assert!(!from_fee.is_empty());
    for expected in from_fee {
        let predicted = what_if.impacted.iter().find(|s| s.id == expected.id);
        let predicted = predicted.unwrap_or_else(|| panic!("what_if misses {}", expected.id));
        assert_eq!(predicted.depth, expected.depth, "{}", expected.id);
    }
    assert!(what_if.tests.iter().any(|t| t.id.as_str().contains("TransferServiceTest")));
    assert!(view.what_if(&SymbolId::new("java:nope"), ImpactOptions::default()).is_none());
}
