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
  config: ConfigReport;
  architecture: ArchitectureReport;
  owners: OwnersReport;
  api_surface: ApiSurfaceReport;
  risk: RiskReport;
  policy: PolicyReport;
}

// ---- Configuration (ripplepath.yml, always read from the base revision) ----

export type Gate =
  | "new_architecture_violation"
  | "architecture_violation"
  | "new_cycle"
  | "parse_failure_in_changed_file"
  | "removed_test_on_critical_path"
  | "breaking_api_change"
  | "config_changed"
  | "config_invalid";

export type SelectionMode = "CONSERVATIVE" | "BALANCED" | "FAST_FEEDBACK";

export interface ConfigReport {
  path: string;
  source: "DEFAULTS" | "BASE_REVISION" | "INVALID_USING_DEFAULTS";
  revision: string;
  blob: string | null;
  head_change: "ABSENT" | "UNCHANGED" | "ADDED" | "MODIFIED" | "REMOVED";
  errors: string[];
  head_errors: string[];
  critical: string[];
  generated: string[];
  tests_mode: SelectionMode | null;
  fail_on: Gate[];
  warn_on: Gate[];
}

// ---- Architecture ----

export type DeltaStatus = "NEW" | "PRE_EXISTING" | "REMOVED";

export interface RuleReport {
  index: number;
  from: string;
  kind: "DENY" | "ALLOW";
  layers: string[];
  description: string;
}

export interface LayerReport {
  name: string;
  patterns: string[];
  base_symbols: number;
  head_symbols: number;
}

export interface Violation {
  status: DeltaStatus;
  rule: number;
  description: string;
  from_layer: string;
  to_layer: string;
  edge: Edge;
  base_edge: Edge | null;
}

export interface LayerEdgeEvidence {
  from: string;
  to: string;
  edges: number;
  example: Edge;
}

export interface LayerCycle {
  status: DeltaStatus;
  layers: string[];
  edges: LayerEdgeEvidence[];
}

export interface LayerDependency {
  from: string;
  to: string;
  base_edges: number;
  head_edges: number;
  direction_reversed: boolean;
}

export interface ModuleCoupling {
  from: string;
  to: string;
  base_edges: number;
  head_edges: number;
}

export interface ArchitectureReport {
  configured: boolean;
  cycles_mode: "off" | "warn" | "forbid";
  rules: RuleReport[];
  layers: LayerReport[];
  summary: {
    new_violations: number;
    new_violations_exact: number;
    pre_existing_violations: number;
    removed_violations: number;
    new_cycles: number;
    pre_existing_cycles: number;
    removed_cycles: number;
  };
  violations: Violation[];
  violations_truncated: boolean;
  cycles: LayerCycle[];
  layer_dependencies: LayerDependency[];
  module_coupling: ModuleCoupling[];
  module_coupling_truncated: boolean;
}

// ---- Owners (CODEOWNERS from head; metadata, not authorization) ----

export interface OwnersReport {
  source: string | null;
  changed_in_head: boolean;
  files: { path: string; role: "changed" | "impacted"; owners: string[]; line: number | null }[];
  files_truncated: boolean;
  owners: { owner: string; changed_files: number; impacted_files: number }[];
  unowned_changed_files: number;
  errors: { line: number; message: string }[];
  note: string;
}

// ---- Public API surface ----

export interface ApiChange {
  kind: "REMOVED" | "SIGNATURE_CHANGED" | "NARROWED" | "ADDED";
  id: string;
  previous_id: string | null;
  symbol_kind: SymbolKind;
  language: "java" | "typescript" | "javascript";
  file: string;
  line: number;
}

export interface ApiSurfaceReport {
  breaking: number;
  added: number;
  changes: ApiChange[];
  truncated: boolean;
}

// ---- Risk model (a decomposition, NOT a probability of failure) ----

export type SignalId =
  | "public_api_changed"
  | "migration_changed"
  | "build_or_dependency_changed"
  | "blast_radius"
  | "changed_symbol_centrality"
  | "critical_path_changed"
  | "new_architecture_violation"
  | "new_layer_cycle"
  | "changed_code_without_coverage"
  | "flaky_impacted_tests"
  | "deleted_tests"
  | "parser_uncertainty"
  | "unresolved_references_in_changed_code"
  | "config_changed";

export interface RiskSignal {
  id: SignalId;
  definition: string;
  evaluated: boolean;
  value: number;
  units: number;
  weight: number;
  cap: number;
  points: number;
  evidence: string[];
  evidence_truncated: number;
  note: string | null;
}

export interface RiskReport {
  model_version: number;
  score: number;
  uncapped_total: number;
  level: "LOW" | "MEDIUM" | "HIGH" | "CRITICAL";
  interpretation: string;
  signals: RiskSignal[];
}

// ---- Merge policy ----

export interface GateResult {
  gate: Gate;
  level: "OFF" | "WARN" | "FAIL";
  status: "PASS" | "WARN" | "FAIL" | "OFF" | "NOT_EVALUATED";
  detail: string;
  evidence: string[];
}

export interface PolicyReport {
  result: "PASS" | "WARN" | "FAIL";
  gates: GateResult[];
  note: string;
}

export interface Health {
  status: string;
  tool_version: string;
  schema_version: number;
  default_base: string;
  default_head: string;
}
