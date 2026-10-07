//! Typed dependency graph and impact propagation. Pure: no I/O, no global state.

mod graph;
mod impact;

pub use graph::CodeGraph;
pub use impact::{Hop, ImpactOptions, ImpactResult, ImpactedSymbol, impact};
