//! Typed dependency graph and impact propagation. Pure: no I/O, no global state.

mod graph;
mod impact;
mod path;

pub use graph::CodeGraph;
pub use impact::{Hop, ImpactOptions, ImpactResult, ImpactedSymbol, impact};
pub use path::{DependencyPath, PathOptions, PathSearch, dependency_path};
