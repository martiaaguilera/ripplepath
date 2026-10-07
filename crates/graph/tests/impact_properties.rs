#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;

use proptest::prelude::*;
use ripplepath_core::{
    Edge, EdgeKind, Evidence, FingerprintBuilder, Language, Span, Symbol, SymbolId, SymbolKind, Visibility,
};
use ripplepath_graph::{CodeGraph, ImpactOptions, impact};

const KINDS: [EdgeKind; 4] = [EdgeKind::Calls, EdgeKind::References, EdgeKind::Overrides, EdgeKind::Contains];
const EVIDENCE: [Evidence; 3] = [Evidence::ResolvedExact, Evidence::StaticInferred, Evidence::NamingHeuristic];

fn symbol(i: usize) -> Symbol {
    Symbol {
        id: SymbolId::new(format!("s{i:02}")),
        kind: SymbolKind::Method,
        name: format!("s{i}"),
        language: Language::Java,
        module: "m".into(),
        file: "f".into(),
        span: Span { start_line: 1, end_line: 1 },
        parent: None,
        visibility: Visibility::Public,
        is_test: false,
        fingerprint: FingerprintBuilder::new().finish(),
    }
}

fn edges_strategy(nodes: usize) -> impl Strategy<Value = Vec<Edge>> {
    prop::collection::vec((0..nodes, 0..nodes, 0..KINDS.len(), 0..EVIDENCE.len()), 0..60).prop_map(|raw| {
        raw.into_iter()
            .filter(|(a, b, ..)| a != b)
            .map(|(a, b, k, e)| Edge {
                from: SymbolId::new(format!("s{a:02}")),
                to: SymbolId::new(format!("s{b:02}")),
                kind: KINDS[k],
                evidence: EVIDENCE[e],
                file: "f".into(),
                line: 1,
                rule: "prop".into(),
            })
            .collect()
    })
}

const NODES: usize = 16;

fn build(edges: Vec<Edge>) -> CodeGraph {
    CodeGraph::new((0..NODES).map(symbol).collect(), edges)
}

proptest! {
    #[test]
    fn traversal_has_no_duplicates_and_excludes_roots(edges in edges_strategy(NODES), root in 0..NODES) {
        let graph = build(edges);
        let roots = [SymbolId::new(format!("s{root:02}"))];
        let result = impact(&graph, &roots, ImpactOptions { max_depth: 32, max_impacted: 10_000 });
        let unique: BTreeSet<_> = result.impacted.iter().map(|i| &i.id).collect();
        prop_assert_eq!(unique.len(), result.impacted.len());
        prop_assert!(!unique.contains(&roots[0]));
    }

    #[test]
    fn result_is_independent_of_edge_order(edges in edges_strategy(NODES), root in 0..NODES) {
        let roots = [SymbolId::new(format!("s{root:02}"))];
        let forward = impact(&build(edges.clone()), &roots, ImpactOptions::default());
        let mut reversed_edges = edges;
        reversed_edges.reverse();
        let reversed = impact(&build(reversed_edges), &roots, ImpactOptions::default());
        prop_assert_eq!(forward, reversed);
    }

    #[test]
    fn every_path_is_made_of_existing_edges_and_has_matching_depth(edges in edges_strategy(NODES), root in 0..NODES) {
        let graph = build(edges);
        let roots = [SymbolId::new(format!("s{root:02}"))];
        let result = impact(&graph, &roots, ImpactOptions::default());
        let existing: BTreeSet<&Edge> = graph.edges().iter().collect();
        for item in &result.impacted {
            prop_assert_eq!(item.path.len() as u32, item.depth);
            prop_assert_eq!(&item.path.last().unwrap().symbol, &item.id);
            for hop in &item.path {
                prop_assert!(existing.contains(&hop.edge));
                prop_assert!(hop.edge.kind != EdgeKind::Contains);
            }
        }
    }

    #[test]
    fn removing_an_edge_removes_paths_that_require_it(edges in edges_strategy(NODES), root in 0..NODES, pick in any::<prop::sample::Index>()) {
        prop_assume!(!edges.is_empty());
        let graph = build(edges.clone());
        let removed = graph.edges()[pick.index(graph.edges().len())].clone();
        let remaining: Vec<Edge> = graph.edges().iter().filter(|e| **e != removed).cloned().collect();
        let roots = [SymbolId::new(format!("s{root:02}"))];
        let after = impact(&build(remaining), &roots, ImpactOptions::default());
        for item in &after.impacted {
            prop_assert!(item.path.iter().all(|hop| hop.edge != removed));
        }
    }
}
