//! Language-agnostic model shared by every Ripplepath component.
//!
//! Everything here is plain data. Ordering derives (`Ord`) are part of the contract: output is
//! produced by sorting these types, which is what makes reports byte-for-byte reproducible.

mod edge;
mod fingerprint;
mod symbol;

pub use edge::{Edge, EdgeKind, Evidence};
pub use fingerprint::{Fingerprint, FingerprintBuilder};
pub use symbol::{Language, Span, Symbol, SymbolId, SymbolKind, Visibility};

/// Bumped whenever the shape or meaning of serialized analysis output changes.
pub const ANALYSIS_SCHEMA_VERSION: u32 = 1;
