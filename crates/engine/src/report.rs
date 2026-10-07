//! `analysis.json`, schema version [`ripplepath_core::ANALYSIS_SCHEMA_VERSION`].
//!
//! Every collection is emitted in a defined order (documented per field) so that the same inputs
//! produce byte-identical output. Field names are part of the public contract consumed by the CLI,
//! the web UI, CI and coding agents; changing their meaning requires a schema version bump.

use ripplepath_core::{Edge, Evidence, Language, Span, SymbolId, SymbolKind, Visibility};
use ripplepath_graph::Hop;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisReport {
    pub schema_version: u32,
    pub tool_version: String,
    pub base: RevisionInfo,
    pub head: RevisionInfo,
    pub summary: Summary,
    /// Sorted by path.
    pub files: Vec<FileChange>,
    /// Sorted by (file, span start, id).
    pub changed_symbols: Vec<ChangedSymbol>,
    /// Sorted by (depth, id).
    pub impacted_symbols: Vec<ImpactedSymbolReport>,
    /// Sorted by (reason, depth, weakest evidence strongest-first, id).
    pub tests: Vec<TestRecommendation>,
    /// Sorted by (severity, kind, file, line, detail).
    pub uncertainty: Vec<Uncertainty>,
    /// Bounded subgraph for visualisation: changed + impacted symbols and the edges among them.
    pub graph: GraphSlice,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevisionInfo {
    pub spec: String,
    pub commit: Option<String>,
    pub tree: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub files_changed: usize,
    pub symbols_changed: usize,
    pub symbols_impacted: usize,
    pub modules_impacted: usize,
    /// Test units selected by the recommendations (units inside a recommended container count).
    pub tests_recommended: usize,
    pub tests_total: usize,
    pub uncertainty_items: usize,
    /// True when impact traversal hit `max_impacted`; the impacted set is then a lower bound.
    pub impact_truncated: bool,
    pub max_depth: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FileStatus {
    Added,
    Deleted,
    Modified,
    Renamed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    pub old_path: Option<String>,
    pub status: FileStatus,
    /// Present for renames: share of lines common to both sides, in percent.
    pub similarity: Option<u8>,
    pub language: Option<Language>,
    /// Empty for binary, oversized or unparsed files.
    pub hunks: Vec<HunkReport>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HunkReport {
    pub old_start: u32,
    pub old_len: u32,
    pub new_start: u32,
    pub new_len: u32,
    /// Innermost symbols containing the removed lines (base revision), sorted.
    pub base_symbols: Vec<SymbolId>,
    /// Innermost symbols containing the added lines (head revision), sorted.
    pub head_symbols: Vec<SymbolId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChangeKind {
    Added,
    Deleted,
    Modified,
    /// Same owner and name, different parameter list. `id` is the head symbol, `previous_id` the base one.
    SignatureChanged,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedSymbol {
    pub id: SymbolId,
    pub change: ChangeKind,
    pub previous_id: Option<SymbolId>,
    /// A deleted/added counterpart with an identical fingerprint: likely a move or rename. Reported,
    /// never merged — identity is not inferred from similarity.
    pub probable_move: Option<SymbolId>,
    pub kind: SymbolKind,
    pub name: String,
    pub language: Language,
    pub module: String,
    pub file: String,
    pub span: Span,
    pub visibility: Visibility,
    pub is_test: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactedSymbolReport {
    pub id: SymbolId,
    pub kind: SymbolKind,
    pub name: String,
    pub module: String,
    pub file: String,
    pub span: Span,
    pub is_test: bool,
    pub depth: u32,
    pub root: SymbolId,
    pub weakest_evidence: Evidence,
    /// Which revision's graph produced this explanation: `head`, or `base` for dependents of
    /// deleted symbols.
    pub graph: GraphSide,
    pub path: Vec<Hop>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GraphSide {
    Head,
    Base,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TestReason {
    /// The test, or shared code inside its test class/file, changed.
    ChangedTest,
    /// A static dependency path connects the test to a changed symbol.
    StaticPath,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestRecommendation {
    pub id: SymbolId,
    pub name: String,
    pub file: String,
    pub reason: TestReason,
    pub depth: u32,
    /// The changed symbol the path starts from (the test itself for `CHANGED_TEST`).
    pub root: SymbolId,
    pub weakest_evidence: Evidence,
    pub path: Vec<Hop>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// The analysis may be missing changed or impacted symbols in a changed file.
    High,
    /// An edge may be missing somewhere relevant to this change.
    Medium,
    /// Context worth knowing; does not directly weaken this result.
    Low,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UncertaintyKind {
    SyntaxError,
    ParseFailure,
    FileTooLarge,
    BinaryFile,
    UnsupportedLanguage,
    UnresolvedReference,
    RejectedPath,
    RenameDetectionSkipped,
    ImpactTruncated,
    SymlinkOrSubmodule,
    ExcludedFile,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Uncertainty {
    pub severity: Severity,
    pub kind: UncertaintyKind,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub symbol: Option<SymbolId>,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeRole {
    Changed,
    Impacted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: SymbolId,
    pub name: String,
    pub kind: SymbolKind,
    pub module: String,
    pub file: String,
    pub line: u32,
    pub role: NodeRole,
    pub change: Option<ChangeKind>,
    pub depth: u32,
    pub is_test: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphSlice {
    /// Sorted by id.
    pub nodes: Vec<GraphNode>,
    /// Sorted by (from, to, kind).
    pub edges: Vec<Edge>,
    /// True when nodes were dropped to respect the node cap.
    pub clamped: bool,
    pub node_cap: usize,
}
