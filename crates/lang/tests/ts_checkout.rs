#![allow(clippy::unwrap_used, clippy::expect_used)]

//! TypeScript frontend against the `typescript-checkout` fixture, read straight from disk.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ripplepath_core::{EdgeKind, Evidence, SymbolId, SymbolKind};
use ripplepath_lang::LanguageGraph;
use ripplepath_lang::ts::{self, facts::TsFile};

const BUDGET: Duration = Duration::from_secs(5);

fn fixture_root(version: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/typescript-checkout").join(version)
}

fn sources(root: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| ts::EXTENSIONS.iter().any(|x| e == *x)) {
                let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                out.push((rel, std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n")));
            }
        }
    }
    out.sort();
    out
}

fn extract_all(version: &str) -> Vec<TsFile> {
    sources(&fixture_root(version)).iter().map(|(p, t)| ts::extract(p, t, BUDGET).unwrap()).collect()
}

fn graph(version: &str) -> LanguageGraph {
    ts::resolve(&extract_all(version).iter().collect::<Vec<_>>())
}

fn graph_of(files: &[(&str, &str)]) -> LanguageGraph {
    let facts: Vec<TsFile> = files.iter().map(|(p, s)| ts::extract(p, s, BUDGET).unwrap()).collect();
    ts::resolve(&facts.iter().collect::<Vec<_>>())
}

fn edge(g: &LanguageGraph, from: &str, to: &str, kind: EdgeKind) -> Option<Evidence> {
    g.edges
        .iter()
        .find(|e| e.from == SymbolId::new(from) && e.to == SymbolId::new(to) && e.kind == kind)
        .map(|e| e.evidence)
}

const QUOTE_IMPL: &str = "ts:src/pricing/pricing-service.ts#PricingService.quote";
const QUOTE_DECL: &str = "ts:src/pricing/price-source.ts#PriceSource.quote";

#[test]
fn extracts_symbols_with_stable_ids() {
    let g = graph("v1");
    let ids: Vec<(&str, SymbolKind)> = g.symbols.iter().map(|s| (s.id.as_str(), s.kind)).collect();
    for expected in [
        ("ts:src/money.ts#money", SymbolKind::Function),
        ("ts:src/money.ts#Money", SymbolKind::Interface),
        ("ts:src/pricing/discount.ts#DiscountCode", SymbolKind::TypeAlias),
        ("ts:src/cart.ts#Cart", SymbolKind::Class),
        ("ts:src/cart.ts#Cart.total", SymbolKind::Method),
        ("ts:src/cart.ts#Cart.prices", SymbolKind::Field),
        ("ts:src/cart.ts#Cart.constructor", SymbolKind::Constructor),
        (QUOTE_DECL, SymbolKind::Method),
        ("ts:src/ui/CartBadge.tsx#CartBadge", SymbolKind::Function),
        ("ts:src/pricing/discount.test.ts#test:applyDiscount > takes 10% off with WELCOME10", SymbolKind::TestCase),
        ("ts:src/money.test.ts#test:formats with two decimals", SymbolKind::TestCase),
    ] {
        assert!(ids.contains(&expected), "missing {expected:?}; have {ids:#?}");
    }
    let test_file = g.symbols.iter().find(|s| s.id.as_str() == "file:src/cart.test.ts").unwrap();
    assert!(test_file.is_test);
}

#[test]
fn resolves_imports_through_barrels_and_star_exports() {
    let g = graph("v1");
    // `import { PricingService } from "./pricing"` → index.ts → named re-export.
    assert_eq!(
        edge(&g, "file:src/cart.test.ts", "ts:src/pricing/pricing-service.ts#PricingService", EdgeKind::Imports),
        Some(Evidence::ResolvedExact)
    );
    // `import type { PriceSource } from "./pricing"` → `export type { … } from`.
    assert_eq!(
        edge(&g, "file:src/cart.ts", "ts:src/pricing/price-source.ts#PriceSource", EdgeKind::Imports),
        Some(Evidence::ResolvedExact)
    );
    assert!(g.unresolved.is_empty(), "{:#?}", g.unresolved);
}

#[test]
fn resolves_calls_through_declared_types_and_parameter_properties() {
    let g = graph("v1");
    // `this.prices.quote(...)` where `prices` is a constructor parameter property typed PriceSource.
    assert_eq!(edge(&g, "ts:src/cart.ts#Cart.total", QUOTE_DECL, EdgeKind::Calls), Some(Evidence::ResolvedExact));
    assert_eq!(
        edge(&g, "ts:src/cart.ts#Cart.total", "ts:src/cart.ts#Cart.prices", EdgeKind::References),
        Some(Evidence::ResolvedExact),
        "the field used as receiver is a dependency too"
    );
    assert_eq!(
        edge(&g, QUOTE_IMPL, "ts:src/pricing/discount.ts#applyDiscount", EdgeKind::Calls),
        Some(Evidence::ResolvedExact)
    );
    // Typed parameter receiver: `cart.total()` with `cart: Cart`.
    assert_eq!(
        edge(&g, "ts:src/checkout/checkout.ts#checkoutSummary", "ts:src/cart.ts#Cart.total", EdgeKind::Calls),
        Some(Evidence::ResolvedExact)
    );
    // `new Cart(new PricingService(code))`: explicit constructors are the targets.
    assert_eq!(
        edge(&g, "ts:src/checkout/checkout.ts#newCart", "ts:src/cart.ts#Cart.constructor", EdgeKind::Instantiates),
        Some(Evidence::ResolvedExact)
    );
    assert_eq!(edge(&g, QUOTE_IMPL, QUOTE_DECL, EdgeKind::Overrides), Some(Evidence::ResolvedExact));
    assert_eq!(
        edge(
            &g,
            "ts:src/pricing/pricing-service.ts#PricingService",
            "ts:src/pricing/price-source.ts#PriceSource",
            EdgeKind::Implements
        ),
        Some(Evidence::ResolvedExact)
    );
}

#[test]
fn jsx_components_and_test_cases_carry_edges() {
    let g = graph("v1");
    assert_eq!(
        edge(&g, "ts:src/ui/CartBadge.tsx#Header", "ts:src/ui/CartBadge.tsx#CartBadge", EdgeKind::Calls),
        Some(Evidence::ResolvedExact)
    );
    // Suite-level `let cart: Cart` gives the test a typed receiver.
    assert_eq!(
        edge(&g, "ts:src/cart.test.ts#test:Cart > totals line items", "ts:src/cart.ts#Cart.total", EdgeKind::Calls),
        Some(Evidence::ResolvedExact)
    );
    // `beforeEach` runs for the whole file, so its edges belong to the file symbol.
    assert_eq!(
        edge(&g, "file:src/cart.test.ts", "ts:src/cart.ts#Cart.constructor", EdgeKind::Instantiates),
        Some(Evidence::ResolvedExact)
    );
}

#[test]
fn untyped_receivers_are_unresolved_not_guessed() {
    let g = graph("v2");
    let header = "ts:src/ui/CartBadge.tsx#Header";
    assert!(!g.edges.iter().any(|e| e.from.as_str() == header && e.to.as_str() == "ts:src/cart.ts#Cart.total"));
    let unresolved: Vec<_> = g.unresolved.iter().filter(|u| u.from.as_str() == header).collect();
    assert_eq!(unresolved.len(), 1, "{unresolved:#?}");
    assert_eq!(unresolved[0].detail, "call total");
}

#[test]
fn library_calls_on_untyped_values_are_not_noise() {
    let g = graph_of(&[("a.ts", "export function f(xs) { return xs.map((x) => x.trim()).join(','); }\n")]);
    assert!(g.unresolved.is_empty(), "{:#?}", g.unresolved);
}

#[test]
fn missing_relative_imports_are_reported() {
    let g = graph_of(&[("src/a.ts", "import { gone } from './missing';\nexport const x = gone();\n")]);
    let details: Vec<&str> = g.unresolved.iter().map(|u| u.detail.as_str()).collect();
    assert!(details.contains(&"import ./missing"), "{details:?}");
}

#[test]
fn default_and_namespace_imports_resolve() {
    let g = graph_of(&[
        ("src/lib.ts", "export default function helper() {}\nexport function other() {}\n"),
        (
            "src/use.ts",
            "import helper from './lib';\nimport * as lib from './lib';\nexport function run() { helper(); lib.other(); }\n",
        ),
    ]);
    assert_eq!(edge(&g, "ts:src/use.ts#run", "ts:src/lib.ts#helper", EdgeKind::Calls), Some(Evidence::ResolvedExact));
    assert_eq!(edge(&g, "ts:src/use.ts#run", "ts:src/lib.ts#other", EdgeKind::Calls), Some(Evidence::ResolvedExact));
}

#[test]
fn comment_only_edits_keep_fingerprints_and_methods_are_independent() {
    let a = graph_of(&[("c.ts", "export class C {\n  a() { return 1; }\n  b() { return 2; }\n}\n")]);
    let b = graph_of(&[("c.ts", "export class C {\n  // why\n  a() { return 1; }\n  b() { return 3; }\n}\n")]);
    let fp = |g: &LanguageGraph, id: &str| g.symbols.iter().find(|s| s.id.as_str() == id).unwrap().fingerprint;
    assert_eq!(fp(&a, "ts:c.ts#C.a"), fp(&b, "ts:c.ts#C.a"));
    assert_eq!(fp(&a, "ts:c.ts#C"), fp(&b, "ts:c.ts#C"));
    assert_ne!(fp(&a, "ts:c.ts#C.b"), fp(&b, "ts:c.ts#C.b"));
}

#[test]
fn adding_a_suite_does_not_change_the_file_but_suite_level_code_does() {
    let base = "describe('a', () => {
  it('x', () => {});
});
";
    let added_suite = format!(
        "{base}describe('b', () => {{
  it('y', () => {{}});
}});
"
    );
    let changed_hook = "describe('a', () => {
  beforeEach(() => setup());
  it('x', () => {});
});
";
    let header = |src: &str| ts::extract("t.test.ts", src, BUDGET).unwrap().header_fingerprint;
    assert_eq!(header(base), header(&added_suite));
    assert_ne!(header(base), header(changed_hook));
}

#[test]
fn hostile_nesting_does_not_overflow_the_stack() {
    let chain = format!("export const v = a{};\n", ".b".repeat(100_000));
    ts::extract("deep.ts", &chain, Duration::from_secs(60)).unwrap();
    let parens = format!("export const v = {}1{};\n", "(".repeat(20_000), ")".repeat(20_000));
    ts::extract("parens.ts", &parens, Duration::from_secs(60)).unwrap();
}

#[test]
fn resolution_is_deterministic_regardless_of_file_order() {
    let mut files = extract_all("v2");
    let forward = ts::resolve(&files.iter().collect::<Vec<_>>());
    files.reverse();
    let reversed = ts::resolve(&files.iter().collect::<Vec<_>>());
    assert_eq!(forward, reversed);
}
