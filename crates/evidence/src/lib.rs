//! Parsers for test evidence produced by other tools. Pure: text in, records out.
//!
//! These files come from CI artifacts and may be large or crafted. Every parser is bounded (input
//! size, element count, nesting depth) and XML input never expands custom entities or fetches
//! external resources.

mod jacoco;
mod junit;
mod lcov;
mod xml;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use jacoco::parse_jacoco;
pub use junit::{Outcome, TestCaseResult, parse_junit};
pub use lcov::parse_lcov;

/// Inputs above this size are refused. Real per-test reports are kilobytes to a few megabytes.
pub const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_ELEMENTS: usize = 5_000_000;
const MAX_DEPTH: usize = 128;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EvidenceError {
    #[error("input is {size} bytes, above the {limit} byte limit")]
    TooLarge { size: usize, limit: usize },
    #[error("malformed {format} input: {message}")]
    Malformed { format: &'static str, message: String },
    #[error("{format} input declares XML entities; refused to avoid entity expansion attacks")]
    EntityDeclaration { format: &'static str },
    #[error("{format} input exceeds structural limits ({what})")]
    LimitExceeded { format: &'static str, what: &'static str },
}

/// Line coverage for one source file as the tool named it (a path that may be absolute, relative
/// to some other root, or package-relative — mapping to repository paths happens later).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileCoverage {
    pub path: String,
    /// line → covered
    pub lines: BTreeMap<u32, bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageReport {
    /// Test that produced this coverage, when the format records it (LCOV `TN:`). `None` means
    /// aggregate coverage of a whole run.
    pub test: Option<String>,
    /// Sorted by path.
    pub files: Vec<FileCoverage>,
}

fn check_size(input: &str) -> Result<(), EvidenceError> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(EvidenceError::TooLarge { size: input.len(), limit: MAX_INPUT_BYTES });
    }
    Ok(())
}
