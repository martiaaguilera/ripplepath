//! The analysis pipeline. Thin orchestration over the pure crates; all I/O happens here.

mod analysis;
mod changes;
pub mod fixture;
mod limits;
mod report;
mod snapshot;

pub use analysis::{AnalysisError, AnalyzeOptions, analyze};
pub use limits::Limits;
pub use report::*;
pub use snapshot::{FactCache, Snapshot, SnapshotFile, build_snapshot};

pub const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");
