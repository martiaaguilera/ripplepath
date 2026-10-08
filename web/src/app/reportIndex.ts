// Lookup tables over one report. Pure indexing: the same facts, keyed for O(1) access by views.

import type {
  AnalysisReport,
  ChangedSymbol,
  FileChange,
  GraphNode,
  ImpactedSymbol,
  TestRecommendation,
} from "../api/types";

export interface ReportIndex {
  changed: ReadonlyMap<string, ChangedSymbol>;
  impacted: ReadonlyMap<string, ImpactedSymbol>;
  tests: ReadonlyMap<string, TestRecommendation>;
  nodes: ReadonlyMap<string, GraphNode>;
  files: ReadonlyMap<string, FileChange>;
  /** Every symbol id the report names anywhere a user could navigate to. */
  symbolIds: ReadonlySet<string>;
  /** Changed paths, including the old side of renames. */
  filePaths: ReadonlySet<string>;
}

const cache = new WeakMap<AnalysisReport, ReportIndex>();

export function indexReport(report: AnalysisReport): ReportIndex {
  const hit = cache.get(report);
  if (hit) return hit;
  const changed = new Map(report.changed_symbols.map((s) => [s.id, s]));
  const impacted = new Map(report.impacted_symbols.map((s) => [s.id, s]));
  const tests = new Map(report.tests.map((t) => [t.id, t]));
  const nodes = new Map(report.graph.nodes.map((n) => [n.id, n]));
  const files = new Map(report.files.map((f) => [f.path, f]));
  const symbolIds = new Set([...changed.keys(), ...impacted.keys(), ...tests.keys(), ...nodes.keys()]);
  const filePaths = new Set(report.files.flatMap((f) => (f.old_path ? [f.path, f.old_path] : [f.path])));
  const index = { changed, impacted, tests, nodes, files, symbolIds, filePaths };
  cache.set(report, index);
  return index;
}

/** Tests whose explaining path starts at, or passes through, `id` (as reported). */
export function testsReaching(report: AnalysisReport, id: string): TestRecommendation[] {
  return report.tests.filter((t) => t.root === id || t.id === id || t.path.some((hop) => hop.symbol === id));
}

/** Distinct symbols with an edge into `id` in the visualised slice, excluding structure and test evidence. */
export function dependentsInSlice(report: AnalysisReport, id: string): string[] {
  const from = new Set<string>();
  for (const edge of report.graph.edges) {
    if (edge.to === id && edge.kind !== "CONTAINS" && edge.kind !== "TESTS") from.add(edge.from);
  }
  return [...from].sort();
}

/** Layer names the architecture findings attribute to a file, if any finding names it. */
export function layersNamedFor(report: AnalysisReport, file: string): string[] {
  const layers = new Set<string>();
  for (const v of report.architecture.violations) {
    if (v.edge.file === file) layers.add(v.from_layer);
  }
  return [...layers].sort();
}

/**
 * Splits a whitespace-delimited token of engine prose into surrounding punctuation and a core that
 * `accept` recognises. Symbol ids end in `)` themselves (`…#withdraw(Money)`), so punctuation is
 * peeled one character at a time and the longest accepted core wins: stripping every trailing `)`
 * up front would turn the id into something no report names.
 */
export function matchToken(
  token: string,
  accept: (core: string) => boolean,
): { lead: string; core: string; trail: string } | null {
  for (let start = 0; start < token.length; start += 1) {
    if (start > 0 && !"([".includes(token.charAt(start - 1))) break;
    for (let end = token.length; end > start; end -= 1) {
      if (end < token.length && !")],;".includes(token.charAt(end))) break;
      const core = token.slice(start, end);
      if (accept(core)) return { lead: token.slice(0, start), core, trail: token.slice(end) };
    }
  }
  return null;
}

/**
 * Risk signals with points whose evidence names one of `needles` (a symbol id, or a path, possibly
 * followed by `:line`). Whole-token comparison: `Account` must not match `Account#withdraw`.
 */
export function signalsNaming(report: AnalysisReport, needles: readonly string[]): string[] {
  const wanted = needles.filter((n) => n.length > 0);
  const accept = (core: string) => wanted.some((n) => core === n || core.startsWith(`${n}:`));
  const names = (evidence: string) => evidence.split(/\s+/).some((token) => matchToken(token, accept) !== null);
  return report.risk.signals.filter((s) => s.points > 0 && s.evidence.some(names)).map((s) => s.id);
}
