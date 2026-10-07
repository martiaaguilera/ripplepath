// Mirror of `analysis.json` schema version 1 (crates/engine/src/report.rs).
// Kept by hand on purpose: the contract is small, and a reviewer should be able to read it here.

export const SUPPORTED_SCHEMA_VERSION = 1;

export type Evidence =
  | "RESOLVED_EXACT"
  | "COVERAGE_OBSERVED"
  | "STATIC_INFERRED"
  | "HISTORY_COCHANGE"
  | "NAMING_HEURISTIC";

export type EdgeKind =
  | "CONTAINS"
  | "IMPORTS"
  | "EXTENDS"
  | "IMPLEMENTS"
  | "OVERRIDES"
  | "CALLS"
  | "INSTANTIATES"
  | "REFERENCES"
  | "TESTS";

export type SymbolKind =
  | "file"
  | "class"
  | "interface"
  | "enum"
  | "record"
  | "annotation"
  | "method"
  | "constructor"
  | "field"
  | "function"
  | "variable"
  | "type_alias"
  | "test_case";

export type ChangeKind = "ADDED" | "DELETED" | "MODIFIED" | "SIGNATURE_CHANGED";

export interface Span {
  start_line: number;
  end_line: number;
}

export interface Edge {
  from: string;
  to: string;
  kind: EdgeKind;
  evidence: Evidence;
  file: string;
  line: number;
  rule: string;
}

export interface Hop {
  symbol: string;
  edge: Edge;
  via_dispatch: boolean;
}

export interface RevisionInfo {
  spec: string;
  commit: string | null;
  tree: string;
}

export interface Summary {
  files_changed: number;
  symbols_changed: number;
  symbols_impacted: number;
  modules_impacted: number;
  tests_recommended: number;
  tests_total: number;
  uncertainty_items: number;
  impact_truncated: boolean;
  max_depth: number;
}

export interface HunkReport {
  old_start: number;
  old_len: number;
  new_start: number;
  new_len: number;
  base_symbols: string[];
  head_symbols: string[];
}

export interface FileChange {
  path: string;
  old_path: string | null;
  status: "ADDED" | "DELETED" | "MODIFIED" | "RENAMED";
  similarity: number | null;
  language: "java" | "typescript" | "javascript" | null;
  hunks: HunkReport[];
}

export interface ChangedSymbol {
  id: string;
  change: ChangeKind;
  previous_id: string | null;
  probable_move: string | null;
  kind: SymbolKind;
  name: string;
  language: "java" | "typescript" | "javascript";
  module: string;
  file: string;
  span: Span;
  visibility: "public" | "protected" | "package" | "private";
  is_test: boolean;
}

export interface ImpactedSymbol {
  id: string;
  kind: SymbolKind;
  name: string;
  module: string;
  file: string;
  span: Span;
  is_test: boolean;
  depth: number;
  root: string;
  weakest_evidence: Evidence;
  graph: "head" | "base";
  path: Hop[];
}

export interface TestRecommendation {
  id: string;
  name: string;
  file: string;
  reason: "CHANGED_TEST" | "STATIC_PATH";
  depth: number;
  root: string;
  weakest_evidence: Evidence;
  path: Hop[];
}

export type Severity = "high" | "medium" | "low";

export interface Uncertainty {
  severity: Severity;
  kind: string;
  file: string | null;
  line: number | null;
  symbol: string | null;
  detail: string;
}

export interface GraphNode {
  id: string;
  name: string;
  kind: SymbolKind;
  module: string;
  file: string;
  line: number;
  role: "changed" | "impacted";
  change: ChangeKind | null;
  depth: number;
  is_test: boolean;
}

export interface GraphSlice {
  nodes: GraphNode[];
  edges: Edge[];
  clamped: boolean;
  node_cap: number;
}

export interface AnalysisReport {
  schema_version: number;
  tool_version: string;
  base: RevisionInfo;
  head: RevisionInfo;
  summary: Summary;
  files: FileChange[];
  changed_symbols: ChangedSymbol[];
  impacted_symbols: ImpactedSymbol[];
  tests: TestRecommendation[];
  uncertainty: Uncertainty[];
  graph: GraphSlice;
}

export interface Health {
  status: string;
  tool_version: string;
  schema_version: number;
  default_base: string;
  default_head: string;
}
