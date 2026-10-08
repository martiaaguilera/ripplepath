//! Read-only Model Context Protocol server over stdio.
//!
//! An adapter, not a second product: every answer is a projection of the deterministic engine's
//! output (graph, impact paths, test evidence, architecture findings) with its evidence attached.
//! The repository is fixed when the server starts; tool calls choose revisions and symbols, never
//! paths, and nothing from the analysed repository is executed. See docs/MCP.md and
//! docs/adr/0007-read-only-mcp.md.

mod protocol;
mod tools;

use std::io::{BufRead, Write};
use std::path::PathBuf;

pub use protocol::{LEGACY_VERSIONS, MAX_MESSAGE_BYTES, MODERN_VERSIONS};
pub use tools::{MAX_RESULT_BYTES, TOOL_NAMES};

#[derive(Clone, Debug)]
pub struct McpConfig {
    pub repo: PathBuf,
    /// Fact cache and test evidence; without it answers use static evidence only.
    pub db: Option<PathBuf>,
    /// Revisions used when a tool call omits them.
    pub default_base: String,
    pub default_head: String,
}

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("cannot open the repository: {0}")]
    Repository(String),
    #[error("transport I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Serves MCP over a newline-delimited JSON-RPC stream until `input` ends.
///
/// Requests are handled one at a time: analyses are CPU-bound and a coding agent waits for each
/// answer anyway, so sequential processing is the simplest way to bound concurrency to one.
pub fn serve(config: McpConfig, input: impl BufRead, output: impl Write) -> Result<(), McpError> {
    let tools = tools::Tools::new(config)?;
    protocol::run(tools, input, output)
}
