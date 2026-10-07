#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Java frontend against the `java-banking` fixture, read straight from disk.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ripplepath_core::{EdgeKind, Evidence, SymbolId, SymbolKind};
use ripplepath_lang::LanguageGraph;
use ripplepath_lang::java::{self, facts::JavaFile};

const BUDGET: Duration = Duration::from_secs(5);

fn fixture_root(version: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/java-banking").join(version)
}

fn java_files(root: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "java") {
                let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                // Fixtures are checked out with platform line endings; normalize so spans match.
                let text = std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
                out.push((rel, text));
            }
        }
    }
    out.sort();
    out
}

fn extract_all(version: &str) -> Vec<JavaFile> {
    java_files(&fixture_root(version)).iter().map(|(path, text)| java::extract(path, text, BUDGET).unwrap()).collect()
}

fn graph(version: &str) -> LanguageGraph {
    java::resolve(&extract_all(version).iter().collect::<Vec<_>>())
}

fn id(raw: &str) -> SymbolId {
    SymbolId::new(raw)
}

fn edge(graph: &LanguageGraph, from: &str, to: &str, kind: EdgeKind) -> Option<Evidence> {
    graph.edges.iter().find(|e| e.from == id(from) && e.to == id(to) && e.kind == kind).map(|e| e.evidence)
}

#[test]
fn extracts_stable_symbol_identities() {
    let graph = graph("v1");
    let ids: Vec<&str> = graph.symbols.iter().map(|s| s.id.as_str()).collect();
    for expected in [
        "java:com.acme.bank.domain.Money",
        "java:com.acme.bank.domain.Money#<init>(BigDecimal,String)",
        "java:com.acme.bank.domain.Money#of(String,String)",
        "java:com.acme.bank.domain.Money#amount",
        "java:com.acme.bank.domain.FeePolicy#feeFor(Money)",
        "java:com.acme.bank.application.TransferService#transfer(String,String,Money)",
        "java:com.acme.bank.application.TransferServiceTest#movesMoneyAndChargesFee()",
        "file:src/main/java/com/acme/bank/domain/Money.java",
    ] {
        assert!(ids.contains(&expected), "missing {expected}; have {ids:#?}");
    }
    let test = graph
        .symbols
        .iter()
        .find(|s| s.id == id("java:com.acme.bank.application.TransferServiceTest#movesMoneyAndChargesFee()"))
        .unwrap();
    assert!(test.is_test);
    assert_eq!(test.kind, SymbolKind::Method);
}

#[test]
fn resolves_calls_through_fields_locals_and_static_receivers() {
    let g = graph("v1");
    // field receiver: `feePolicy.feeFor(amount)`
    assert_eq!(
        edge(
            &g,
            "java:com.acme.bank.application.TransferService#transfer(String,String,Money)",
            "java:com.acme.bank.domain.FeePolicy#feeFor(Money)",
            EdgeKind::Calls
        ),
        Some(Evidence::ResolvedExact)
    );
    // local receiver: `from.withdraw(...)`
    assert_eq!(
        edge(
            &g,
            "java:com.acme.bank.application.TransferService#transfer(String,String,Money)",
            "java:com.acme.bank.domain.Account#withdraw(Money)",
            EdgeKind::Calls
        ),
        Some(Evidence::ResolvedExact)
    );
    // unqualified call to a private helper
    assert_eq!(
        edge(
            &g,
            "java:com.acme.bank.application.TransferService#transfer(String,String,Money)",
            "java:com.acme.bank.application.TransferService#load(String)",
            EdgeKind::Calls
        ),
        Some(Evidence::ResolvedExact)
    );
    // static receiver resolved through an import: `Money.of(...)`
    assert_eq!(
        edge(
            &g,
            "java:com.acme.bank.api.TransferController#transfer(String,String,String,String)",
            "java:com.acme.bank.domain.Money#of(String,String)",
            EdgeKind::Calls
        ),
        Some(Evidence::ResolvedExact)
    );
    // chained call on a parameter of an in-repo type: `amount.plus(fee)` inside an argument
    assert_eq!(
        edge(
            &g,
            "java:com.acme.bank.application.TransferService#transfer(String,String,Money)",
            "java:com.acme.bank.domain.Money#plus(Money)",
            EdgeKind::Calls
        ),
        Some(Evidence::ResolvedExact)
    );
    // chained through a declared return type: `repo.findById("b").get().balance()` stops at
    // Optional (external) — no edge to Account#balance may be invented.
    assert_eq!(
        edge(
            &g,
            "java:com.acme.bank.application.TransferServiceTest#movesMoneyAndChargesFee()",
            "java:com.acme.bank.domain.Account#balance()",
            EdgeKind::Calls
        ),
        None
    );
}

#[test]
fn records_overrides_instantiation_and_supertypes() {
    let g = graph("v1");
    assert_eq!(
        edge(
            &g,
            "java:com.acme.bank.domain.StandardFeePolicy#feeFor(Money)",
            "java:com.acme.bank.domain.FeePolicy#feeFor(Money)",
            EdgeKind::Overrides
        ),
        Some(Evidence::ResolvedExact)
    );
    assert_eq!(
        edge(
            &g,
            "java:com.acme.bank.domain.StandardFeePolicy",
            "java:com.acme.bank.domain.FeePolicy",
            EdgeKind::Implements
        ),
        Some(Evidence::ResolvedExact)
    );
    assert!(
        !g.edges.iter().any(
            |e| e.from == id("java:com.acme.bank.domain.InsufficientFundsException") && e.kind == EdgeKind::Extends
        ),
        "RuntimeException is external; no supertype edge may be fabricated"
    );
    // `new StandardFeePolicy()` has no declared constructor: the type itself is the target.
    assert_eq!(
        edge(
            &g,
            "java:com.acme.bank.application.TransferServiceTest#movesMoneyAndChargesFee()",
            "java:com.acme.bank.domain.StandardFeePolicy",
            EdgeKind::Instantiates
        ),
        Some(Evidence::ResolvedExact)
    );
    assert_eq!(
        edge(
            &g,
            "java:com.acme.bank.domain.Money#of(String,String)",
            "java:com.acme.bank.domain.Money#<init>(BigDecimal,String)",
            EdgeKind::Instantiates
        ),
        Some(Evidence::ResolvedExact)
    );
}

#[test]
fn fixture_has_no_unresolved_references() {
    let g = graph("v1");
    assert!(g.unresolved.is_empty(), "{:#?}", g.unresolved);
}

#[test]
fn comment_only_edit_keeps_fingerprint_but_body_edit_changes_it() {
    let base = graph("v1");
    let head = graph("v2");
    let fp = |g: &LanguageGraph, raw: &str| g.symbols.iter().find(|s| s.id == id(raw)).map(|s| s.fingerprint);

    let is_negative = "java:com.acme.bank.domain.Money#isNegative()";
    assert_eq!(fp(&base, is_negative), fp(&head, is_negative));
    assert_eq!(fp(&base, "java:com.acme.bank.domain.Money"), fp(&head, "java:com.acme.bank.domain.Money"));

    let fee_for = "java:com.acme.bank.domain.StandardFeePolicy#feeFor(Money)";
    assert_ne!(fp(&base, fee_for), fp(&head, fee_for));
    // Editing a method must not mark its class as modified.
    let class = "java:com.acme.bank.domain.StandardFeePolicy";
    assert_eq!(fp(&base, class), fp(&head, class));
}

#[test]
fn untyped_lambda_receivers_are_reported_as_unresolved_not_guessed() {
    let account = "package p;\npublic class Account { public void close() {} }\n";
    let user = "package p;\nimport java.util.List;\nclass Closer {\n  void run(List<Account> all) {\n    all.forEach(a -> a.close());\n  }\n}\n";
    let files = [
        java::extract("p/Account.java", account, BUDGET).unwrap(),
        java::extract("p/Closer.java", user, BUDGET).unwrap(),
    ];
    let g = java::resolve(&files.iter().collect::<Vec<_>>());
    assert!(!g.edges.iter().any(|e| e.kind == EdgeKind::Calls), "{:#?}", g.edges);
    assert_eq!(g.unresolved.len(), 1, "{:#?}", g.unresolved);
    assert_eq!(g.unresolved[0].detail, "call close/0");
    assert_eq!(g.unresolved[0].line, 5);
}

#[test]
fn overloads_with_same_arity_are_inferred_not_exact() {
    let src =
        "package p;\nclass A {\n  void f(String s) {}\n  void f(Integer i) {}\n  void g(Object o) { f(null); }\n}\n";
    let a = java::extract("p/A.java", src, BUDGET).unwrap();
    let g = java::resolve(&[&a]);
    let from = "java:p.A#g(Object)";
    assert_eq!(edge(&g, from, "java:p.A#f(String)", EdgeKind::Calls), Some(Evidence::StaticInferred));
    assert_eq!(edge(&g, from, "java:p.A#f(Integer)", EdgeKind::Calls), Some(Evidence::StaticInferred));
}

#[test]
fn hostile_nesting_does_not_overflow_the_stack() {
    // A 100k-segment qualified type nests 100k levels deep in the syntax tree.
    let qualified = format!("{}B", "a.".repeat(100_000));
    let src = format!("class X {{ {qualified} f; void m({qualified} p) {{ {qualified} local = ({qualified}) p; }} }}");
    let file = java::extract("X.java", &src, Duration::from_secs(60)).unwrap();
    assert_eq!(file.types.len(), 1);

    let parens = format!("class Y {{ int f = {}1{}; }}", "(".repeat(20_000), ")".repeat(20_000));
    let file = java::extract("Y.java", &parens, Duration::from_secs(60)).unwrap();
    assert_eq!(file.types[0].fields.len(), 1);
}

#[test]
fn syntax_errors_are_recorded() {
    let src = "package p;\nclass Broken {\n  void f( {\n}\n";
    let file = java::extract("p/Broken.java", src, BUDGET).unwrap();
    assert!(!file.syntax_error_lines.is_empty());
}

#[test]
fn resolution_is_deterministic_regardless_of_file_order() {
    let mut files = extract_all("v2");
    let forward = java::resolve(&files.iter().collect::<Vec<_>>());
    files.reverse();
    let reversed = java::resolve(&files.iter().collect::<Vec<_>>());
    assert_eq!(forward, reversed);
}
