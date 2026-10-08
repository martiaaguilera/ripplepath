import type { AnalysisReport, Edge } from "../api/types";

const edge = (from: string, to: string, kind: Edge["kind"], evidence: Edge["evidence"] = "RESOLVED_EXACT"): Edge => ({
  from,
  to,
  kind,
  evidence,
  file: "src/X.java",
  line: 10,
  rule: "test.rule",
});

export const FEE_IMPL = "java:bank.StandardFeePolicy#feeFor(Money)";
export const FEE_DECL = "java:bank.FeePolicy#feeFor(Money)";
export const SERVICE = "java:bank.TransferService#transfer(String,String,Money)";
export const CONTROLLER = "java:bank.TransferController#transfer(String)";
export const TEST = "java:bank.TransferControllerTest#returnsOk()";

const overrides = edge(FEE_IMPL, FEE_DECL, "OVERRIDES");
const serviceCalls = edge(SERVICE, FEE_DECL, "CALLS");
const controllerCalls = edge(CONTROLLER, SERVICE, "CALLS", "STATIC_INFERRED");
const testCalls = edge(TEST, CONTROLLER, "CALLS");

export function sampleReport(): AnalysisReport {
  return {
    schema_version: 1,
    tool_version: "0.1.0",
    base: { spec: "main~1", commit: "aaaaaaaaaaaa", tree: "t1" },
    head: { spec: "main", commit: "bbbbbbbbbbbb", tree: "t2" },
    summary: {
      files_changed: 1,
      symbols_changed: 1,
      symbols_impacted: 4,
      modules_impacted: 1,
      tests_recommended: 1,
      tests_total: 3,
      uncertainty_items: 1,
      impact_truncated: false,
      max_depth: 6,
    },
    files: [],
    changed_symbols: [
      {
        id: FEE_IMPL,
        change: "MODIFIED",
        previous_id: null,
        probable_move: null,
        kind: "method",
        name: "feeFor(Money)",
        language: "java",
        module: "bank",
        file: "src/StandardFeePolicy.java",
        span: { start_line: 4, end_line: 9 },
        visibility: "public",
        is_test: false,
      },
    ],
    impacted_symbols: [
      impacted(FEE_DECL, 1, [{ symbol: FEE_DECL, edge: overrides, via_dispatch: true }]),
      impacted(SERVICE, 2, [
        { symbol: FEE_DECL, edge: overrides, via_dispatch: true },
        { symbol: SERVICE, edge: serviceCalls, via_dispatch: false },
      ]),
      impacted(CONTROLLER, 3, [
        { symbol: FEE_DECL, edge: overrides, via_dispatch: true },
        { symbol: SERVICE, edge: serviceCalls, via_dispatch: false },
        { symbol: CONTROLLER, edge: controllerCalls, via_dispatch: false },
      ]),
    ],
    tests: [
      {
        id: TEST,
        name: "returnsOk()",
        file: "src/TransferControllerTest.java",
        reason: "STATIC_PATH",
        depth: 4,
        root: FEE_IMPL,
        weakest_evidence: "STATIC_INFERRED",
        path: [
          { symbol: FEE_DECL, edge: overrides, via_dispatch: true },
          { symbol: SERVICE, edge: serviceCalls, via_dispatch: false },
          { symbol: CONTROLLER, edge: controllerCalls, via_dispatch: false },
          { symbol: TEST, edge: testCalls, via_dispatch: false },
        ],
      },
    ],
    uncertainty: [
      {
        severity: "low",
        kind: "UNSUPPORTED_LANGUAGE",
        file: "db/V2__x.sql",
        line: null,
        symbol: null,
        detail: "changed file is not in a supported language; its effects are not traced",
      },
    ],
    graph: {
      nodes: [
        node(FEE_IMPL, "changed", 0),
        node(FEE_DECL, "impacted", 1),
        node(SERVICE, "impacted", 2),
        node(CONTROLLER, "impacted", 3),
        { ...node(TEST, "impacted", 4), is_test: true },
      ],
      edges: [overrides, serviceCalls, controllerCalls, testCalls],
      clamped: false,
      node_cap: 400,
    },
    config: {
      path: "ripplepath.yml",
      source: "DEFAULTS",
      revision: "main~1",
      blob: null,
      head_change: "ABSENT",
      errors: [],
      head_errors: [],
      critical: [],
      generated: [],
      tests_mode: null,
      fail_on: [],
      warn_on: [],
    },
    architecture: {
      configured: false,
      cycles_mode: "warn",
      rules: [],
      layers: [],
      summary: {
        new_violations: 0,
        new_violations_exact: 0,
        pre_existing_violations: 0,
        removed_violations: 0,
        new_cycles: 0,
        pre_existing_cycles: 0,
        removed_cycles: 0,
      },
      violations: [],
      violations_truncated: false,
      cycles: [],
      layer_dependencies: [],
      module_coupling: [],
      module_coupling_truncated: false,
    },
    owners: {
      source: null,
      changed_in_head: false,
      files: [],
      files_truncated: false,
      owners: [],
      unowned_changed_files: 0,
      errors: [],
      note: "Ownership is review-routing metadata from CODEOWNERS, not authorization.",
    },
    api_surface: { breaking: 0, added: 0, changes: [], truncated: false },
    risk: {
      model_version: 1,
      score: 0,
      uncapped_total: 0,
      level: "LOW",
      interpretation: "NOT a probability of failure",
      signals: [],
    },
    policy: { result: "PASS", gates: [], note: "the risk score is never a gate" },
  };

  function impacted(id: string, depth: number, path: AnalysisReport["impacted_symbols"][number]["path"]) {
    return {
      id,
      kind: "method" as const,
      name: id.split("#")[1] ?? id,
      module: "bank",
      file: "src/X.java",
      span: { start_line: 1, end_line: 2 },
      is_test: false,
      depth,
      root: FEE_IMPL,
      weakest_evidence: depth >= 3 ? ("STATIC_INFERRED" as const) : ("RESOLVED_EXACT" as const),
      graph: "head" as const,
      path,
    };
  }

  function node(id: string, role: "changed" | "impacted", depth: number) {
    return {
      id,
      name: id.split("#")[1] ?? id,
      kind: "method" as const,
      module: "bank",
      file: "src/X.java",
      line: 1,
      role,
      change: role === "changed" ? ("MODIFIED" as const) : null,
      depth,
      is_test: false,
    };
  }
}
