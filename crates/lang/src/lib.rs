//! Language frontends.
//!
//! Each frontend has two phases with different caching properties:
//! - `extract(path, source)` → per-file facts. Pure function of the file; cacheable by blob hash.
//! - `resolve(&[facts])` → symbols and evidence-carrying edges. Depends on the whole snapshot.

pub mod java;
mod syntax;

use ripplepath_core::{Edge, Symbol, SymbolId};
use serde::{Deserialize, Serialize};

pub use syntax::ParseError;

/// A reference the frontend could not bind to a declaration. Surfaced as uncertainty: the graph may
/// be missing an edge here.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UnresolvedRef {
    pub from: SymbolId,
    pub file: String,
    pub line: u32,
    pub detail: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LanguageGraph {
    /// Sorted by id.
    pub symbols: Vec<Symbol>,
    /// Sorted by (from, to, kind); at most one edge per triple.
    pub edges: Vec<Edge>,
    pub unresolved: Vec<UnresolvedRef>,
}
