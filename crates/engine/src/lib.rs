//! The analysis pipeline. Thin orchestration over the pure crates; all I/O happens here.

mod analysis;
mod changes;
mod evidence;
pub mod fixture;
mod history;
mod index;
mod limits;
mod report;
mod selection;
mod signals;
mod snapshot;
mod test_evidence;

pub use analysis::{AnalysisError, AnalyzeOptions, analyze};
pub use evidence::{CoverageFormat, CoverageInput, IngestOutcome, ingest_coverage, ingest_junit};
pub use index::{IndexOutcome, index_revision, indexed_graph};
pub use limits::Limits;
pub use report::*;
pub use ripplepath_git::GitError;
pub use snapshot::{FactCache, Snapshot, SnapshotFile, build_snapshot};

pub const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const MAX_EVIDENCE_BYTES: usize = ripplepath_evidence::MAX_INPUT_BYTES;
