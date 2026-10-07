use std::collections::{BTreeMap, HashMap};

use ripplepath_core::{Edge, Symbol, SymbolId};

/// Immutable symbol graph with adjacency in both directions.
///
/// Adjacency lists hold indices into the sorted edge vector, so iterating neighbours yields edges
/// in (from, to, kind) order — callers get deterministic traversal without sorting again.
#[derive(Debug, Default)]
pub struct CodeGraph {
    symbols: BTreeMap<SymbolId, Symbol>,
    edges: Vec<Edge>,
    incoming: HashMap<SymbolId, Vec<usize>>,
    outgoing: HashMap<SymbolId, Vec<usize>>,
    dangling_edges: usize,
}

impl CodeGraph {
    /// Builds the graph. Edges whose endpoints are not symbols are dropped and counted rather than
    /// kept: an edge to nothing cannot be explained in a report.
    pub fn new(symbols: Vec<Symbol>, mut edges: Vec<Edge>) -> Self {
        let symbols: BTreeMap<SymbolId, Symbol> = symbols.into_iter().map(|s| (s.id.clone(), s)).collect();
        let before = edges.len();
        edges.retain(|e| symbols.contains_key(&e.from) && symbols.contains_key(&e.to));
        let dangling_edges = before - edges.len();
        edges.sort();
        edges.dedup_by(|a, b| a.from == b.from && a.to == b.to && a.kind == b.kind);

        let mut incoming: HashMap<SymbolId, Vec<usize>> = HashMap::new();
        let mut outgoing: HashMap<SymbolId, Vec<usize>> = HashMap::new();
        for (i, edge) in edges.iter().enumerate() {
            outgoing.entry(edge.from.clone()).or_default().push(i);
            incoming.entry(edge.to.clone()).or_default().push(i);
        }
        // Incoming lists are filled in (from, to, kind) order of the edge vector, which for a fixed
        // `to` is ordered by `from` — already what traversal wants.
        Self { symbols, edges, incoming, outgoing, dangling_edges }
    }

    pub fn symbol(&self, id: &SymbolId) -> Option<&Symbol> {
        self.symbols.get(id)
    }

    pub fn contains(&self, id: &SymbolId) -> bool {
        self.symbols.contains_key(id)
    }

    /// All symbols, sorted by id.
    pub fn symbols(&self) -> impl Iterator<Item = &Symbol> {
        self.symbols.values()
    }

    /// All edges, sorted by (from, to, kind).
    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    pub fn incoming(&self, id: &SymbolId) -> impl Iterator<Item = &Edge> {
        self.incoming.get(id).into_iter().flatten().map(|&i| &self.edges[i])
    }

    pub fn outgoing(&self, id: &SymbolId) -> impl Iterator<Item = &Edge> {
        self.outgoing.get(id).into_iter().flatten().map(|&i| &self.edges[i])
    }

    pub fn symbol_count(&self) -> usize {
        self.symbols.len()
    }

    pub fn dangling_edges(&self) -> usize {
        self.dangling_edges
    }
}
