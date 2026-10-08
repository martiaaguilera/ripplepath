//! Java frontend: tree-sitter extraction of per-file facts, then whole-snapshot resolution.

mod extract;
pub mod facts;
mod resolve;

pub use extract::extract;
pub use resolve::resolve;

/// Bumped whenever extraction output for the same input can change. Part of the fact-cache key, so
/// a new extractor never reuses facts produced by an old one.
pub const EXTRACTOR_VERSION: u32 = 2;

/// Annotations that mark a JUnit 4/5 or TestNG test method. Matched on simple name, so
/// `@org.junit.Test` and `@Test` are equivalent.
pub const TEST_ANNOTATIONS: &[&str] = &["Test", "ParameterizedTest", "RepeatedTest", "TestFactory", "TestTemplate"];
