use std::collections::HashMap;
use std::ops::Range;

use ripplepath_core::{Edge, Symbol, SymbolId};

/// Immutable symbol graph with adjacency in both directions.
///
/// Adjacency lists hold indices into the sorted edge vector, so iterating neighbours yields edges
/// in (from, to, kind) order — callers get deterministic traversal without sorting again.
///
/// Layout (chosen by measurement, docs/ENGINEERING_LOG.md "performance"): symbols live in a vector
/// sorted by id, found through one id → position hash index; adjacency is compressed (CSR) over
/// those positions. Building it costs one hash lookup per edge endpoint, where per-endpoint
/// ordered-map lookups and per-edge key clones used to dominate. The hash map is only ever looked
/// up, never iterated, so its order cannot reach any output.
#[derive(Debug, Default)]
pub struct CodeGraph {
    /// Sorted by id, ids unique.
    symbols: Vec<Symbol>,
    position: HashMap<SymbolId, usize>,
    /// Sorted by (from, to, kind), so the edges leaving one symbol are contiguous.
    edges: Vec<Edge>,
    /// Edges leaving the symbol at position `p` are `edges[out_start[p]..out_start[p + 1]]`.
    out_start: Vec<usize>,
    /// Edges entering the symbol at position `p` are `in_edges[in_start[p]..in_start[p + 1]]`
    /// (indices into `edges`).
    in_start: Vec<usize>,
    in_edges: Vec<usize>,
    dangling_edges: usize,
}

impl CodeGraph {
    /// Builds the graph. Edges whose endpoints are not symbols are dropped and counted rather than
    /// kept: an edge to nothing cannot be explained in a report.
    pub fn new(mut symbols: Vec<Symbol>, mut edges: Vec<Edge>) -> Self {
        // Stable sort, then the last definition of a duplicated id wins — the semantics of
        // collecting into a map, which the graph had before.
        symbols.sort_by(|a, b| a.id.cmp(&b.id));
        let mut unique: Vec<Symbol> = Vec::with_capacity(symbols.len());
        for symbol in symbols {
            match unique.last_mut() {
                Some(last) if last.id == symbol.id => *last = symbol,
                _ => unique.push(symbol),
            }
        }
        let position: HashMap<SymbolId, usize> = unique.iter().enumerate().map(|(i, s)| (s.id.clone(), i)).collect();

        edges.sort();
        let mut kept: Vec<Edge> = Vec::with_capacity(edges.len());
        let mut ends: Vec<(usize, usize)> = Vec::with_capacity(edges.len());
        let mut dangling_edges = 0;
        for edge in edges {
            let (Some(&from), Some(&to)) = (position.get(&edge.from), position.get(&edge.to)) else {
                dangling_edges += 1;
                continue;
            };
            // Duplicates are adjacent after sorting; the first (smallest evidence/site) is kept.
            if kept.last().is_some_and(|l| l.from == edge.from && l.to == edge.to && l.kind == edge.kind) {
                continue;
            }
            kept.push(edge);
            ends.push((from, to));
        }

        let n = unique.len();
        let mut out_start = vec![0usize; n + 1];
        let mut in_start = vec![0usize; n + 1];
        for &(from, to) in &ends {
            out_start[from + 1] += 1;
            in_start[to + 1] += 1;
        }
        for p in 0..n {
            out_start[p + 1] += out_start[p];
            in_start[p + 1] += in_start[p];
        }
        // Filling in edge order keeps each incoming list ordered by `from` — what traversal wants.
        let mut cursor = in_start.clone();
        let mut in_edges = vec![0usize; kept.len()];
        for (i, &(_, to)) in ends.iter().enumerate() {
            in_edges[cursor[to]] = i;
            cursor[to] += 1;
        }
        Self { symbols: unique, position, edges: kept, out_start, in_start, in_edges, dangling_edges }
    }

    pub fn symbol(&self, id: &SymbolId) -> Option<&Symbol> {
        self.position.get(id).map(|&p| &self.symbols[p])
    }

    pub fn contains(&self, id: &SymbolId) -> bool {
        self.position.contains_key(id)
    }

    /// All symbols, sorted by id.
    pub fn symbols(&self) -> impl Iterator<Item = &Symbol> {
        self.symbols.iter()
    }

    /// All edges, sorted by (from, to, kind).
    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    pub fn incoming(&self, id: &SymbolId) -> impl Iterator<Item = &Edge> {
        let range = self.range(&self.in_start, id);
        self.in_edges[range].iter().map(|&i| &self.edges[i])
    }

    pub fn outgoing(&self, id: &SymbolId) -> impl Iterator<Item = &Edge> {
        self.edges[self.range(&self.out_start, id)].iter()
    }

    pub fn symbol_count(&self) -> usize {
        self.symbols.len()
    }

    pub fn dangling_edges(&self) -> usize {
        self.dangling_edges
    }

    /// Offsets arrays have one entry per symbol plus one, built together with `position`, so a
    /// known position always has both bounds.
    fn range(&self, starts: &[usize], id: &SymbolId) -> Range<usize> {
        self.position.get(id).map_or(0..0, |&p| starts[p]..starts[p + 1])
    }
}

#[cfg(test)]
mod tests {
    use ripplepath_core::{EdgeKind, Evidence, FingerprintBuilder, Language, Span, SymbolKind, Visibility};

    use super::*;

    fn symbol(id: &str, name: &str) -> Symbol {
        Symbol {
            id: SymbolId::new(id),
            kind: SymbolKind::Method,
            name: name.to_owned(),
            language: Language::Java,
            module: "m".into(),
            file: "A.java".into(),
            span: Span { start_line: 1, end_line: 2 },
            parent: None,
            visibility: Visibility::Public,
            is_test: false,
            fingerprint: FingerprintBuilder::new().finish(),
        }
    }

    fn edge(from: &str, to: &str, line: u32) -> Edge {
        Edge {
            from: SymbolId::new(from),
            to: SymbolId::new(to),
            kind: EdgeKind::Calls,
            evidence: Evidence::ResolvedExact,
            file: "A.java".into(),
            line,
            rule: "r".into(),
        }
    }

    #[test]
    fn adjacency_is_sorted_deduplicated_and_drops_dangling_edges() {
        let graph = CodeGraph::new(
            vec![symbol("c", "c"), symbol("a", "a"), symbol("b", "first"), symbol("b", "last")],
            vec![edge("c", "a", 1), edge("b", "a", 2), edge("b", "a", 1), edge("a", "x", 1), edge("a", "c", 1)],
        );
        fn ids<'a>(edges: impl Iterator<Item = &'a Edge>) -> Vec<(&'a str, &'a str, u32)> {
            edges.map(|e| (e.from.as_str(), e.to.as_str(), e.line)).collect()
        }
        assert_eq!(graph.symbols().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!(graph.symbol(&SymbolId::new("b")).map(|s| s.name.as_str()), Some("last"));
        assert_eq!(graph.dangling_edges(), 1);
        assert_eq!(graph.edges().len(), 3);
        assert_eq!(ids(graph.incoming(&SymbolId::new("a"))), [("b", "a", 1), ("c", "a", 1)]);
        assert_eq!(ids(graph.outgoing(&SymbolId::new("a"))), [("a", "c", 1)]);
        assert_eq!(graph.outgoing(&SymbolId::new("missing")).count(), 0);
        assert_eq!(graph.incoming(&SymbolId::new("b")).count(), 0);
        assert!(CodeGraph::default().incoming(&SymbolId::new("a")).next().is_none());
    }
}
