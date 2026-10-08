// Hand-written report for component tests. Shaped like the java-banking demo, but small and with
// every section populated so each view has something to render. Test data only — never shown in
// the product.

import type { AnalysisReport, Edge, FileBlob } from "../api/types";

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
export const COV_TEST = "java:bank.TransferServiceTest";
export const ACCOUNT = "java:bank.domain.Account#withdraw(Money)";
export const API_ERRORS = "java:bank.api.ApiErrors#accountFrozen(String)";
export const FEE_FILE = "src/StandardFeePolicy.java";
export const MIGRATION = "db/V2__x.sql";

const overrides = edge(FEE_IMPL, FEE_DECL, "OVERRIDES");
const serviceCalls = edge(SERVICE, FEE_DECL, "CALLS");
const controllerCalls = edge(CONTROLLER, SERVICE, "CALLS", "STATIC_INFERRED");
const testCalls = edge(TEST, CONTROLLER, "CALLS");
const covered: Edge = {
  from: COV_TEST,
  to: FEE_IMPL,
  kind: "TESTS",
  evidence: "COVERAGE_OBSERVED",
  file: "src/TransferServiceTest.java",
  line: 12,
  rule: "coverage.jacoco@aaaaaaaaaa",
};
const violationEdge: Edge = {
  from: ACCOUNT,
  to: API_ERRORS,
  kind: "CALLS",
  evidence: "RESOLVED_EXACT",
  file: "src/domain/Account.java",
  line: 25,
  rule: "java.call.static",
};

export function sampleReport(): AnalysisReport {
  return {
    schema_version: 1,
    tool_version: "0.1.0",
    base: { spec: "main~1", commit: "aaaaaaaaaaaa", tree: "t1" },
    head: { spec: "main", commit: "bbbbbbbbbbbb", tree: "t2" },
    summary: {
      files_changed: 2,
      symbols_changed: 1,
      symbols_impacted: 4,
      modules_impacted: 1,
      tests_recommended: 2,
      tests_total: 3,
      uncertainty_items: 1,
      impact_truncated: false,
      max_depth: 6,
    },
    files: [
      {
        path: MIGRATION,
        old_path: null,
        status: "ADDED",
        similarity: null,
        language: null,
        category: "MIGRATION",
        hunks: [{ old_start: 0, old_len: 0, new_start: 1, new_len: 1, base_symbols: [], head_symbols: [] }],
      },
      {
        path: FEE_FILE,
        old_path: null,
        status: "MODIFIED",
        similarity: null,
        language: "java",
        category: null,
        hunks: [
          { old_start: 5, old_len: 1, new_start: 5, new_len: 2, base_symbols: [FEE_IMPL], head_symbols: [FEE_IMPL] },
        ],
      },
    ],
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
        file: FEE_FILE,
        span: { start_line: 4, end_line: 9 },
        visibility: "public",
        is_test: false,
        coverage: "COVERED",
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
        tier: "WEAK",
        coverage_observed: false,
        history: {
          runs: 8,
          failures: 3,
          flaky_commits: 1,
          median_duration_ms: 1520,
          last_outcome: "FAILED",
          reliability: "FLAKY",
        },
      },
      {
        id: COV_TEST,
        name: "TransferServiceTest",
        file: "src/TransferServiceTest.java",
        reason: "STATIC_PATH",
        depth: 1,
        root: FEE_IMPL,
        weakest_evidence: "COVERAGE_OBSERVED",
        path: [{ symbol: COV_TEST, edge: covered, via_dispatch: false }],
        tier: "STRONG",
        coverage_observed: true,
        history: null,
      },
    ],
    uncertainty: [
      {
        severity: "low",
        kind: "UNSUPPORTED_LANGUAGE",
        file: MIGRATION,
        line: null,
        symbol: null,
        detail: "changed file is not in a supported language; its effects are not traced",
      },
    ],
    test_selection: {
      mode: "BALANCED",
      decision: "FULL_SUITE",
      fallback_reasons: [
        {
          severity: "high",
          code: "MIGRATION_CHANGED",
          detail: `${MIGRATION} changed; its effects are not traced through code`,
        },
      ],
      ordered: [COV_TEST, TEST],
      selected_units: 2,
      total_units: 3,
      selected_runtime_ms: null,
      full_runtime_ms: null,
      notes: ["Evidence is not strong enough to skip tests: run the full suite."],
    },
    evidence: {
      coverage_reports: 1,
      coverage_reports_other_commits: 0,
      coverage_edges: 1,
      test_runs: 8,
      tests_with_history: 1,
    },
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
      source: "BASE_REVISION",
      revision: "main~1",
      blob: "c0ffee",
      head_change: "UNCHANGED",
      errors: [],
      head_errors: [],
      critical: ["src/domain/**"],
      generated: [],
      tests_mode: null,
      fail_on: ["new_architecture_violation"],
      warn_on: [],
    },
    architecture: {
      configured: true,
      cycles_mode: "warn",
      rules: [{ index: 0, from: "domain", kind: "DENY", layers: ["api"], description: "domain must not depend on api" }],
      layers: [
        { name: "api", patterns: ["src/api/**"], base_symbols: 4, head_symbols: 6 },
        { name: "domain", patterns: ["src/domain/**"], base_symbols: 9, head_symbols: 10 },
      ],
      summary: {
        new_violations: 1,
        new_violations_exact: 1,
        pre_existing_violations: 0,
        removed_violations: 0,
        new_cycles: 1,
        pre_existing_cycles: 0,
        removed_cycles: 0,
      },
      violations: [
        {
          status: "NEW",
          rule: 0,
          description: "domain must not depend on api",
          from_layer: "domain",
          to_layer: "api",
          edge: violationEdge,
          base_edge: null,
        },
      ],
      violations_truncated: false,
      cycles: [
        {
          status: "NEW",
          layers: ["api", "domain"],
          edges: [
            { from: "api", to: "domain", edges: 3, example: edge(CONTROLLER, SERVICE, "CALLS") },
            { from: "domain", to: "api", edges: 1, example: violationEdge },
          ],
        },
      ],
      layer_dependencies: [
        { from: "api", to: "domain", base_edges: 3, head_edges: 3, direction_reversed: false },
        { from: "domain", to: "api", base_edges: 0, head_edges: 1, direction_reversed: true },
      ],
      module_coupling: [{ from: "bank.domain", to: "bank.api", base_edges: 0, head_edges: 1 }],
      module_coupling_truncated: false,
    },
    owners: {
      source: ".github/CODEOWNERS",
      changed_in_head: false,
      files: [
        { path: FEE_FILE, role: "changed", owners: ["@acme/payments"], line: 3 },
        { path: MIGRATION, role: "changed", owners: [], line: null },
      ],
      files_truncated: false,
      owners: [{ owner: "@acme/payments", changed_files: 1, impacted_files: 0 }],
      unowned_changed_files: 1,
      errors: [],
      note: "Ownership is review-routing metadata from CODEOWNERS, not authorization.",
    },
    api_surface: {
      breaking: 0,
      added: 1,
      changes: [
        {
          kind: "ADDED",
          id: API_ERRORS,
          previous_id: null,
          symbol_kind: "method",
          language: "java",
          file: "src/api/ApiErrors.java",
          line: 4,
        },
      ],
      truncated: false,
    },
    risk: {
      model_version: 1,
      score: 25,
      uncapped_total: 25,
      level: "MEDIUM",
      interpretation:
        "Risk score: a sum of capped, versioned signal points for prioritising review. It is NOT a probability of failure and is not calibrated against outcomes.",
      signals: [
        {
          id: "migration_changed",
          definition: "changed files classified MIGRATION",
          evaluated: true,
          value: 1,
          units: 1,
          weight: 15,
          cap: 15,
          points: 15,
          evidence: [MIGRATION],
          evidence_truncated: 0,
          note: null,
        },
        {
          id: "new_architecture_violation",
          definition: "NEW layer-rule violations",
          evaluated: true,
          value: 1,
          units: 1,
          weight: 10,
          cap: 20,
          points: 10,
          evidence: [`${ACCOUNT} -> ${API_ERRORS} (Calls) src/domain/Account.java:25`],
          evidence_truncated: 0,
          note: null,
        },
        {
          id: "flaky_impacted_tests",
          definition: "recommended tests classified FLAKY",
          evaluated: false,
          value: 0,
          units: 0,
          weight: 3,
          cap: 9,
          points: 0,
          evidence: [],
          evidence_truncated: 0,
          note: "no CI history ingested",
        },
      ],
    },
    policy: {
      result: "FAIL",
      gates: [
        {
          gate: "new_architecture_violation",
          level: "FAIL",
          status: "FAIL",
          detail: "1 new layer-rule violation(s)",
          evidence: [`domain must not depend on api: ${ACCOUNT} -> ${API_ERRORS}`],
        },
        { gate: "config_changed", level: "WARN", status: "PASS", detail: "ripplepath.yml unchanged", evidence: [] },
      ],
      note: "The risk score is never a gate.",
    },
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
      coverage: null,
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

/** File contents for the diff view: line 5 of base becomes lines 5–6 of head. */
export const FEE_BASE = ["package bank;", "", "class StandardFeePolicy {", "  Money feeFor(Money m) {", "    return m.times(1);", "  }", "}"];
export const FEE_HEAD = [
  "package bank;",
  "",
  "class StandardFeePolicy {",
  "  Money feeFor(Money m) {",
  "    var rate = 2;",
  "    return m.times(rate);",
  "  }",
  "}",
];

export function blob(rev: string, path: string, lines: string[] | null): FileBlob {
  return {
    rev,
    commit: rev,
    path,
    blob: `${rev}:${path}`,
    max_bytes: 1024 * 1024,
    content: lines === null ? { kind: "binary" } : { kind: "text", text: `${lines.join("\n")}\n` },
  };
}
