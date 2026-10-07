use std::time::Duration;

/// Resource bounds for analysing a possibly hostile repository. Every limit that is hit is
/// reported in the analysis as uncertainty; none is silently applied.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Larger source files are not parsed. Real hand-written sources are far below this; files
    /// above it are typically generated or minified, and parsing them buys little.
    pub max_file_bytes: u64,
    /// A snapshot with more tracked files than this is refused.
    pub max_files: usize,
    pub parse_budget: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self { max_file_bytes: 1024 * 1024, max_files: 200_000, parse_budget: Duration::from_secs(2) }
    }
}
