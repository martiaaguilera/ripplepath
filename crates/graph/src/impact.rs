use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use ripplepath_core::{Edge, EdgeKind, Evidence, SymbolId};
use serde::{Deserialize, Serialize};

use crate::CodeGraph;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImpactOptions {
    pub max_depth: u32,
    /// Traversal stops once this many symbols are impacted and the result is marked truncated.
    pub max_impacted: usize,
}

impl Default for ImpactOptions {
    fn default() -> Self {
        Self { max_depth: 6, max_impacted: 5_000 }
    }
}

/// One step of an explaining path, from the changed symbol outward.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Hop {
    /// The symbol this hop arrives at.
    pub symbol: SymbolId,
    /// The edge as stored in the graph (its own `from → to` direction).
    pub edge: Edge,
    /// True when the hop follows an `OVERRIDES` edge forward: the change is in an implementation,
    /// and callers bound to the overridden declaration may dispatch to it at runtime.
    pub via_dispatch: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactedSymbol {
    pub id: SymbolId,
    pub depth: u32,
    /// The changed symbol the explaining path starts from.
    pub root: SymbolId,
    pub path: Vec<Hop>,
    /// Weakest evidence along the path: a path is only as certain as its least certain edge.
    pub weakest_evidence: Evidence,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactResult {
    /// Sorted by (depth, id). Never contains a root.
    pub impacted: Vec<ImpactedSymbol>,
    pub truncated: bool,
}

/// Ranking among candidate parents of the same node at the same depth; the smallest key wins:
/// strongest weakest-evidence first, then parent id, then the edge itself as a total tiebreak.
type RankKey = (Reverse<u8>, SymbolId, Edge);

#[derive(Clone)]
struct Visit {
    depth: u32,
    root: SymbolId,
    parent: Option<(SymbolId, Edge, bool)>,
    weakest: Evidence,
}

/// Breadth-first reverse reachability from `roots`.
///
/// Level-synchronous BFS gives every impacted symbol its minimum depth. Among equally short
/// explanations the one with the strongest weakest-edge wins, then the smallest parent id — so the
/// reported path is both short and as defensible as possible, and independent of hash order.
/// Each symbol is visited once, which also guarantees termination on cycles.
pub fn impact(graph: &CodeGraph, roots: &[SymbolId], options: ImpactOptions) -> ImpactResult {
    let mut visited: BTreeMap<SymbolId, Visit> = BTreeMap::new();
    let root_set: BTreeSet<&SymbolId> = roots.iter().filter(|r| graph.contains(r)).collect();
    for root in &root_set {
        visited.insert(
            (*root).clone(),
            Visit { depth: 0, root: (*root).clone(), parent: None, weakest: Evidence::ResolvedExact },
        );
    }

    let mut frontier: Vec<SymbolId> = root_set.into_iter().cloned().collect();
    let mut truncated = false;
    let mut impacted_count = 0usize;

    for depth in 1..=options.max_depth {
        let mut candidates: BTreeMap<SymbolId, (RankKey, Visit)> = BTreeMap::new();
        for node in &frontier {
            let Some(current) = visited.get(node).cloned() else {
                continue;
            };
            let reverse =
                graph.incoming(node).filter(|e| e.kind.propagates_impact()).map(|e| (e.from.clone(), e, false));
            let dispatch =
                graph.outgoing(node).filter(|e| e.kind == EdgeKind::Overrides).map(|e| (e.to.clone(), e, true));
            for (next, edge, via_dispatch) in reverse.chain(dispatch) {
                if visited.contains_key(&next) {
                    continue;
                }
                let weakest =
                    if edge.evidence.strength() < current.weakest.strength() { edge.evidence } else { current.weakest };
                let key = (Reverse(weakest.strength()), node.clone(), edge.clone());
                let visit = Visit {
                    depth,
                    root: current.root.clone(),
                    parent: Some((node.clone(), edge.clone(), via_dispatch)),
                    weakest,
                };
                match candidates.get(&next) {
                    Some((existing, _)) if *existing <= key => {}
                    _ => {
                        candidates.insert(next, (key, visit));
                    }
                }
            }
        }
        if candidates.is_empty() {
            break;
        }
        frontier = Vec::with_capacity(candidates.len());
        for (id, (_, visit)) in candidates {
            if impacted_count >= options.max_impacted {
                truncated = true;
                break;
            }
            impacted_count += 1;
            frontier.push(id.clone());
            visited.insert(id, visit);
        }
        if truncated {
            break;
        }
    }

    let mut impacted: Vec<ImpactedSymbol> = visited
        .iter()
        .filter(|(_, visit)| visit.depth > 0)
        .map(|(id, visit)| ImpactedSymbol {
            id: id.clone(),
            depth: visit.depth,
            root: visit.root.clone(),
            path: path_to(&visited, id),
            weakest_evidence: visit.weakest,
        })
        .collect();
    impacted.sort_by(|a, b| (a.depth, &a.id).cmp(&(b.depth, &b.id)));
    ImpactResult { impacted, truncated }
}

fn path_to(visited: &BTreeMap<SymbolId, Visit>, target: &SymbolId) -> Vec<Hop> {
    let mut hops = Vec::new();
    let mut current = target.clone();
    while let Some(Visit { parent: Some((parent, edge, via_dispatch)), .. }) = visited.get(&current) {
        hops.push(Hop { symbol: current.clone(), edge: edge.clone(), via_dispatch: *via_dispatch });
        current = parent.clone();
    }
    hops.reverse();
    hops
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
            line: 1,
            rule: "test".to_owned(),
        }
    }

    fn graph(ids: &[&str], edges: Vec<Edge>) -> CodeGraph {
        CodeGraph::new(ids.iter().map(|i| sym(i)).collect(), edges)
    }

    fn ids(result: &ImpactResult) -> Vec<(&str, u32)> {
        result.impacted.iter().map(|i| (i.id.as_str(), i.depth)).collect()
    }

    #[test]
    fn follows_reverse_dependencies_with_depth() {
        // c -> b -> a  (c depends on b depends on a)
        let g = graph(
            &["a", "b", "c"],
            vec![
                edge("b", "a", EdgeKind::Calls, Evidence::ResolvedExact),
                edge("c", "b", EdgeKind::Calls, Evidence::ResolvedExact),
            ],
        );
        let result = impact(&g, &[SymbolId::new("a")], ImpactOptions::default());
        assert_eq!(ids(&result), vec![("b", 1), ("c", 2)]);
        let c = &result.impacted[1];
        assert_eq!(c.path.iter().map(|h| h.symbol.as_str()).collect::<Vec<_>>(), vec!["b", "c"]);
        assert_eq!(c.root.as_str(), "a");
    }

    #[test]
    fn terminates_on_cycles_without_duplicates() {
        let g = graph(
            &["a", "b", "c"],
            vec![
                edge("b", "a", EdgeKind::Calls, Evidence::ResolvedExact),
                edge("c", "b", EdgeKind::Calls, Evidence::ResolvedExact),
                edge("a", "c", EdgeKind::Calls, Evidence::ResolvedExact),
            ],
        );
        let result = impact(&g, &[SymbolId::new("a")], ImpactOptions::default());
        assert_eq!(ids(&result), vec![("b", 1), ("c", 2)]);
    }

    #[test]
    fn containment_does_not_propagate() {
        let g = graph(
            &["class", "m1", "m2", "caller"],
            vec![
                edge("class", "m1", EdgeKind::Contains, Evidence::ResolvedExact),
                edge("class", "m2", EdgeKind::Contains, Evidence::ResolvedExact),
                edge("caller", "m2", EdgeKind::Calls, Evidence::ResolvedExact),
            ],
        );
        let result = impact(&g, &[SymbolId::new("m1")], ImpactOptions::default());
        assert!(result.impacted.is_empty());
    }

    #[test]
    fn overrides_dispatch_reaches_callers_of_the_declaration() {
        // impl OVERRIDES decl; caller CALLS decl. Changing impl impacts caller via dispatch.
        let g = graph(
            &["impl", "decl", "caller"],
            vec![
                edge("impl", "decl", EdgeKind::Overrides, Evidence::ResolvedExact),
                edge("caller", "decl", EdgeKind::Calls, Evidence::ResolvedExact),
            ],
        );
        let result = impact(&g, &[SymbolId::new("impl")], ImpactOptions::default());
        assert_eq!(ids(&result), vec![("decl", 1), ("caller", 2)]);
        assert!(result.impacted[0].path[0].via_dispatch);
        assert!(!result.impacted[1].path[1].via_dispatch);
    }

    #[test]
    fn depth_limit_is_respected() {
        let g = graph(
            &["a", "b", "c", "d"],
            vec![
                edge("b", "a", EdgeKind::Calls, Evidence::ResolvedExact),
                edge("c", "b", EdgeKind::Calls, Evidence::ResolvedExact),
                edge("d", "c", EdgeKind::Calls, Evidence::ResolvedExact),
            ],
        );
        let result = impact(&g, &[SymbolId::new("a")], ImpactOptions { max_depth: 2, max_impacted: 100 });
        assert_eq!(ids(&result), vec![("b", 1), ("c", 2)]);
    }

    #[test]
    fn prefers_stronger_evidence_among_equal_length_paths() {
        // top reaches a through x (inferred) or y (exact); both at depth 2.
        let g = graph(
            &["a", "x", "y", "top"],
            vec![
                edge("x", "a", EdgeKind::Calls, Evidence::StaticInferred),
                edge("y", "a", EdgeKind::Calls, Evidence::ResolvedExact),
                edge("top", "x", EdgeKind::Calls, Evidence::ResolvedExact),
                edge("top", "y", EdgeKind::Calls, Evidence::ResolvedExact),
            ],
        );
        let result = impact(&g, &[SymbolId::new("a")], ImpactOptions::default());
        let top = result.impacted.iter().find(|i| i.id.as_str() == "top").unwrap();
        assert_eq!(top.path[0].symbol.as_str(), "y");
        assert_eq!(top.weakest_evidence, Evidence::ResolvedExact);
    }

    #[test]
    fn truncation_is_reported() {
        let g = graph(
            &["a", "b", "c", "d"],
            vec![
                edge("b", "a", EdgeKind::Calls, Evidence::ResolvedExact),
                edge("c", "a", EdgeKind::Calls, Evidence::ResolvedExact),
                edge("d", "a", EdgeKind::Calls, Evidence::ResolvedExact),
            ],
        );
        let result = impact(&g, &[SymbolId::new("a")], ImpactOptions { max_depth: 6, max_impacted: 2 });
        assert!(result.truncated);
        assert_eq!(result.impacted.len(), 2);
    }
}
