//! TypeScript / JavaScript frontend: tree-sitter extraction of per-file facts, then whole-snapshot
//! module and name resolution.

mod extract;
pub mod facts;
mod resolve;

pub use extract::{extract, is_test_path};
pub use resolve::resolve;

/// Bumped whenever extraction output for the same input can change (part of the fact-cache key).
pub const EXTRACTOR_VERSION: u32 = 2;

/// Extensions handled by this frontend.
pub const EXTENSIONS: &[&str] = &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];
