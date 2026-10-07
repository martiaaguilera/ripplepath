//! Accumulates a frontend's symbols and edges with one deduplication rule for every language.

use std::collections::BTreeMap;

use ripplepath_core::{Edge, EdgeKind, Evidence, Symbol, SymbolId};

use crate::{LanguageGraph, UnresolvedRef};

#[derive(Default)]
pub(crate) struct Output {
    pub(crate) symbols: Vec<Symbol>,
    edges: BTreeMap<(SymbolId, SymbolId, EdgeKind), Edge>,
    unresolved: Vec<UnresolvedRef>,
}

impl Output {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn edge(
        &mut self,
        from: &SymbolId,
        to: &SymbolId,
        kind: EdgeKind,
        evidence: Evidence,
        file: &str,
        line: u32,
        rule: &str,
    ) {
        if from == to {
            return; // recursion is not a dependency on something else
        }
        let candidate = Edge {
            from: from.clone(),
            to: to.clone(),
            kind,
            evidence,
            file: file.to_owned(),
            line,
            rule: rule.to_owned(),
        };
        // One edge per (from, to, kind). Keep the strongest evidence, then the earliest location,
        // so the stored explanation is the most defensible one and independent of visit order.
        let key = (from.clone(), to.clone(), kind);
        match self.edges.get(&key) {
            Some(existing)
                if (std::cmp::Reverse(existing.evidence.strength()), existing.line, &existing.rule)
                    <= (std::cmp::Reverse(candidate.evidence.strength()), candidate.line, &candidate.rule) => {}
            _ => {
                self.edges.insert(key, candidate);
            }
        }
    }

    pub(crate) fn unresolved(&mut self, from: &SymbolId, file: &str, line: u32, detail: String) {
        self.unresolved.push(UnresolvedRef { from: from.clone(), file: file.to_owned(), line, detail });
    }

    pub(crate) fn finish(mut self) -> LanguageGraph {
        self.symbols.sort_by(|a, b| a.id.cmp(&b.id));
        self.unresolved.sort();
        self.unresolved.dedup();
        LanguageGraph { symbols: self.symbols, edges: self.edges.into_values().collect(), unresolved: self.unresolved }
    }
}
