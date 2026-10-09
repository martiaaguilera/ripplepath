#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Adversarial resolver cases from the final review (docs/FINAL_REVIEW.md): situations where a
//! static resolver can be tempted to claim `RESOLVED_EXACT` for a choice the compiler makes with
//! information Ripplepath does not have.

use std::time::Duration;

use ripplepath_core::{Edge, EdgeKind, Evidence, SymbolId};
use ripplepath_lang::LanguageGraph;
use ripplepath_lang::java;

const BUDGET: Duration = Duration::from_secs(5);

fn graph(files: &[(&str, &str)]) -> LanguageGraph {
    let facts: Vec<_> = files.iter().map(|(path, source)| java::extract(path, source, BUDGET).unwrap()).collect();
    java::resolve(&facts.iter().collect::<Vec<_>>())
}

fn calls<'g>(g: &'g LanguageGraph, from: &str) -> Vec<&'g Edge> {
    g.edges.iter().filter(|e| e.from == SymbolId::new(from) && e.kind == EdgeKind::Calls).collect()
}

/// Java overload resolution considers every member method of the receiver type, inherited ones
/// included. A same-arity overload declared in a supertype makes the choice depend on argument
/// types, which Ripplepath does not compute.
#[test]
fn same_arity_overload_in_a_supertype_is_not_exact() {
    let g = graph(&[
        ("p/Base.java", "package p;\npublic class Base {\n  public void put(String s) {}\n}\n"),
        ("p/Child.java", "package p;\npublic class Child extends Base {\n  public void put(int i) {}\n}\n"),
        (
            "p/User.java",
            "package p;\nclass User {\n  void run(Child c) {\n    c.put(\"x\");\n  }\n  void self() {}\n}\n",
        ),
    ]);
    let edges = calls(&g, "java:p.User#run(Child)");
    assert!(!edges.is_empty(), "the call must produce an edge: {:?}", g.edges);
    for edge in &edges {
        assert_eq!(edge.evidence, Evidence::StaticInferred, "{edge:?}");
    }
    assert!(
        edges.iter().any(|e| e.to == SymbolId::new("java:p.Base#put(String)")),
        "the inherited overload is a candidate: {edges:?}"
    );
}

/// The same within one hierarchy level is already inferred; kept as a baseline.
#[test]
fn same_arity_overloads_in_one_type_are_inferred() {
    let g = graph(&[
        ("p/A.java", "package p;\npublic class A {\n  public void put(String s) {}\n  public void put(int i) {}\n}\n"),
        ("p/User.java", "package p;\nclass User {\n  void run(A a) {\n    a.put(1);\n  }\n}\n"),
    ]);
    let edges = calls(&g, "java:p.User#run(A)");
    assert_eq!(edges.len(), 2, "{edges:?}");
    assert!(edges.iter().all(|e| e.evidence == Evidence::StaticInferred));
}

/// An override in the subclass with the same signature is the one target: still exact.
#[test]
fn override_of_the_same_signature_stays_exact() {
    let g = graph(&[
        ("p/Base.java", "package p;\npublic class Base {\n  public void put(String s) {}\n}\n"),
        (
            "p/Child.java",
            "package p;\npublic class Child extends Base {\n  @Override public void put(String s) {}\n}\n",
        ),
        ("p/User.java", "package p;\nclass User {\n  void run(Child c) {\n    c.put(\"x\");\n  }\n}\n"),
    ]);
    let edges = calls(&g, "java:p.User#run(Child)");
    assert_eq!(edges.len(), 1, "{edges:?}");
    assert_eq!(edges[0].to, SymbolId::new("java:p.Child#put(String)"));
    assert_eq!(edges[0].evidence, Evidence::ResolvedExact);
}
