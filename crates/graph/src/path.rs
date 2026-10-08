use std::cmp::Reverse;
use std::collections::BTreeMap;

use ripplepath_core::{Edge, EdgeKind, Evidence, SymbolId};
use serde::{Deserialize, Serialize};

use crate::{CodeGraph, Hop};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathOptions {
    pub max_depth: u32,
    /// Search stops once this many symbols have been reached; a miss is then inconclusive.
    pub max_visited: usize,
}

impl Default for PathOptions {
    fn default() -> Self {
        Self { max_depth: 8, max_visited: 50_000 }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyPath {
    /// Hops from `from` to `to`; each edge is in its stored `from → to` (depends-on) direction.
    pub hops: Vec<Hop>,
    pub weakest_evidence: Evidence,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathSearch {
    pub path: Option<DependencyPath>,
    /// True when the search hit `max_visited` before finishing: absence of a path is then unproven.
    pub truncated: bool,
    /// Symbols reached; for diagnostics.
    pub visited: usize,
}

type RankKey = (Reverse<u8>, SymbolId, Edge);

/// Shortest chain of dependency edges `from → … → to`, following edges in their stored direction.
///
/// CONTAINS is skipped: a class containing a method is structure, not a dependency, and following it
/// would "connect" any two members of one class. Among equally short paths the one with the
/// strongest weakest edge wins, then the smallest parent id, then the edge — the same ranking the
/// impact traversal uses, so the answer is deterministic and as defensible as the graph allows.
pub fn dependency_path(graph: &CodeGraph, from: &SymbolId, to: &SymbolId, options: PathOptions) -> PathSearch {
    if !graph.contains(from) || !graph.contains(to) {
        return PathSearch::default();
    }
    // node → (parent, edge, weakest evidence along the path so far)
    let mut visited: BTreeMap<SymbolId, Option<(SymbolId, Edge, Evidence)>> = BTreeMap::new();
    visited.insert(from.clone(), None);
    let weakest_to = |visited: &BTreeMap<SymbolId, Option<(SymbolId, Edge, Evidence)>>, id: &SymbolId| {
        visited.get(id).and_then(|v| v.as_ref().map(|(_, _, w)| *w)).unwrap_or(Evidence::ResolvedExact)
    };
    let mut frontier = vec![from.clone()];
    let mut truncated = false;
    let mut found = from == to;

    for _ in 1..=options.max_depth {
        if found || frontier.is_empty() {
            break;
        }
        let mut candidates: BTreeMap<SymbolId, (RankKey, (SymbolId, Edge, Evidence))> = BTreeMap::new();
        for node in &frontier {
            let current = weakest_to(&visited, node);
            for edge in graph.outgoing(node).filter(|e| e.kind != EdgeKind::Contains) {
                if visited.contains_key(&edge.to) {
                    continue;
                }
                let weakest = if edge.evidence.strength() < current.strength() { edge.evidence } else { current };
                let key = (Reverse(weakest.strength()), node.clone(), edge.clone());
                match candidates.get(&edge.to) {
                    Some((existing, _)) if *existing <= key => {}
                    _ => {
                        candidates.insert(edge.to.clone(), (key, (node.clone(), edge.clone(), weakest)));
                    }
                }
            }
        }
        frontier = Vec::with_capacity(candidates.len());
        for (id, (_, parent)) in candidates {
            if visited.len() >= options.max_visited {
                truncated = true;
                break;
            }
            found |= &id == to;
            frontier.push(id.clone());
            visited.insert(id, Some(parent));
        }
        if truncated {
            break;
        }
    }

    let path = found.then(|| {
        let mut hops = Vec::new();
        let mut current = to.clone();
        while let Some(Some((parent, edge, _))) = visited.get(&current) {
            hops.push(Hop { symbol: current.clone(), edge: edge.clone(), via_dispatch: false });
            current = parent.clone();
        }
        hops.reverse();
        DependencyPath { hops, weakest_evidence: weakest_to(&visited, to) }
    });
    // A found path is still the best one even if the cap cut its level short: every candidate of a
    // level is ranked against all frontier nodes before any is inserted, and earlier levels were
    // complete. Truncation only weakens a miss.
    PathSearch { truncated: truncated && path.is_none(), visited: visited.len(), path }
}

#[cfg(test)]
mod tests {
    use ripplepath_core::{FingerprintBuilder, Language, Span, Symbol, SymbolKind, Visibility};

    use super::*;

    fn sym(id: &str) -> Symbol {
        Symbol {
            id: SymbolId::new(id),
            kind: SymbolKind::Method,
            name: id.to_owned(),
            language: Language::Java,
            module: "m".to_owned(),
            file: "f.java".to_owned(),
            span: Span { start_line: 1, end_line: 1 },
            parent: None,
            visibility: Visibility::Public,
            is_test: false,
            fingerprint: FingerprintBuilder::new().finish(),
        }
    }

    fn edge(from: &str, to: &str, kind: EdgeKind, evidence: Evidence) -> Edge {
        Edge {
            from: SymbolId::new(from),
            to: SymbolId::new(to),
            kind,
            evidence,
            file: "f.java".to_owned(),
            line: 3,
            rule: "test".to_owned(),
        }
    }

    fn graph(ids: &[&str], edges: Vec<Edge>) -> CodeGraph {
        CodeGraph::new(ids.iter().map(|i| sym(i)).collect(), edges)
    }

    fn hop_ids(search: &PathSearch) -> Vec<&str> {
        search.path.as_ref().map(|p| p.hops.iter().map(|h| h.symbol.as_str()).collect()).unwrap_or_default()
    }

    #[test]
    fn finds_shortest_path_preferring_strong_evidence() {
        let g = graph(
            &["a", "x", "y", "b", "far"],
            vec![
                edge("a", "x", EdgeKind::Calls, Evidence::StaticInferred),
                edge("a", "y", EdgeKind::Calls, Evidence::ResolvedExact),
                edge("x", "b", EdgeKind::Calls, Evidence::ResolvedExact),
                edge("y", "b", EdgeKind::References, Evidence::ResolvedExact),
                edge("a", "far", EdgeKind::Calls, Evidence::ResolvedExact),
            ],
        );
        let search = dependency_path(&g, &SymbolId::new("a"), &SymbolId::new("b"), PathOptions::default());
        assert_eq!(hop_ids(&search), vec!["y", "b"]);
        assert_eq!(search.path.unwrap().weakest_evidence, Evidence::ResolvedExact);
    }

    #[test]
    fn ignores_containment_and_respects_direction_and_depth() {
        let g = graph(
            &["class", "m1", "m2", "c"],
            vec![
                edge("class", "m1", EdgeKind::Contains, Evidence::ResolvedExact),
                edge("m2", "c", EdgeKind::Calls, Evidence::ResolvedExact),
            ],
        );
        let none = dependency_path(&g, &SymbolId::new("class"), &SymbolId::new("m1"), PathOptions::default());
        assert!(none.path.is_none() && !none.truncated);
        let reverse = dependency_path(&g, &SymbolId::new("c"), &SymbolId::new("m2"), PathOptions::default());
        assert!(reverse.path.is_none());
        let shallow = dependency_path(
            &g,
            &SymbolId::new("m2"),
            &SymbolId::new("c"),
            PathOptions { max_depth: 1, max_visited: 10 },
        );
        assert_eq!(hop_ids(&shallow), vec!["c"]);
    }

    #[test]
    fn same_symbol_is_an_empty_path_and_unknown_is_none() {
        let g = graph(&["a"], vec![]);
        let same = dependency_path(&g, &SymbolId::new("a"), &SymbolId::new("a"), PathOptions::default());
        assert_eq!(same.path.map(|p| p.hops.len()), Some(0));
        let unknown = dependency_path(&g, &SymbolId::new("a"), &SymbolId::new("zz"), PathOptions::default());
        assert!(unknown.path.is_none());
    }

    #[test]
    fn reports_truncation_when_the_cap_stops_the_search() {
        let g = graph(
            &["a", "b", "c", "d"],
            vec![
                edge("a", "b", EdgeKind::Calls, Evidence::ResolvedExact),
                edge("b", "c", EdgeKind::Calls, Evidence::ResolvedExact),
                edge("c", "d", EdgeKind::Calls, Evidence::ResolvedExact),
            ],
        );
        let search =
            dependency_path(&g, &SymbolId::new("a"), &SymbolId::new("d"), PathOptions { max_depth: 8, max_visited: 2 });
        assert!(search.path.is_none() && search.truncated);
    }
}
