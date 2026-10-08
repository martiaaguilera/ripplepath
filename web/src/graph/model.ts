// View-model for the impact graph. Pure functions: everything here is decided by the report; the UI
// only filters and highlights, it never infers new relationships.

import type { AnalysisReport, Edge, EdgeKind, Evidence, GraphNode, Hop } from "../api/types";

export const IMPACT_EDGE_KINDS: readonly EdgeKind[] = [
  "CALLS",
  "OVERRIDES",
  "INSTANTIATES",
  "REFERENCES",
  "EXTENDS",
  "IMPLEMENTS",
  "IMPORTS",
  "TESTS",
];

export const EVIDENCE_LABEL: Record<Evidence, string> = {
  RESOLVED_EXACT: "Resolved exactly",
  COVERAGE_OBSERVED: "Observed by coverage",
  STATIC_INFERRED: "Statically inferred",
  HISTORY_COCHANGE: "Historical co-change",
  NAMING_HEURISTIC: "Naming heuristic",
};

export const EVIDENCE_CLASSES: readonly Evidence[] = [
  "RESOLVED_EXACT",
  "COVERAGE_OBSERVED",
  "STATIC_INFERRED",
  "HISTORY_COCHANGE",
  "NAMING_HEURISTIC",
];

export interface Filters {
  maxDepth: number;
  edgeKinds: ReadonlySet<EdgeKind>;
  evidence: ReadonlySet<Evidence>;
  query: string;
  /** Hide changed symbols that have no visible edge: they have no blast radius to show. */
  hideIsolated: boolean;
}

export interface ViewNode extends GraphNode {
  label: string;
  matchesQuery: boolean;
}

export interface ViewGraph {
  nodes: ViewNode[];
  edges: Edge[];
  /** Number of nodes removed by `hideIsolated`. */
  hiddenIsolated: number;
}

export function edgeKey(edge: Pick<Edge, "from" | "to" | "kind">): string {
  return `${edge.from}|${edge.to}|${edge.kind}`;
}

/**
 * Short display label:
 * - `java:com.acme.bank.domain.Account#withdraw(Money)` → `Account.withdraw(Money)`
 * - `ts:src/cart.ts#Cart.total` → `Cart.total`; test cases keep their title
 * - `file:src/main/java/A.java` → `A.java`
 */
export function shortLabel(id: string): string {
  const colon = id.indexOf(":");
  const prefix = colon >= 0 ? id.slice(0, colon) : "";
  const body = colon >= 0 ? id.slice(colon + 1) : id;
  const hash = body.indexOf("#");
  const qualified = hash >= 0 ? body.slice(0, hash) : body;
  const member = hash >= 0 ? body.slice(hash + 1) : undefined;
  if (prefix === "file") return qualified.split("/").pop() ?? qualified;
  if (prefix === "ts") {
    if (member === undefined) return qualified.split("/").pop() ?? qualified;
    return member.startsWith("test:") ? member.slice("test:".length) : member;
  }
  const owner = qualified.split(".").pop() ?? qualified;
  if (member === undefined) return owner;
  return member.startsWith("<init>") ? `${owner}${member.slice("<init>".length)}` : `${owner}.${member}`;
}

export function buildView(report: AnalysisReport, filters: Filters): ViewGraph {
  const query = filters.query.trim().toLowerCase();
  const nodes = report.graph.nodes
    .filter((node) => node.depth <= filters.maxDepth)
    .map((node) => {
      const label = shortLabel(node.id);
      return {
        ...node,
        label,
        matchesQuery: query.length > 0 && (label.toLowerCase().includes(query) || node.id.toLowerCase().includes(query)),
      };
    });
  const visible = new Set(nodes.map((n) => n.id));
  const edges = report.graph.edges.filter(
    (edge) =>
      filters.edgeKinds.has(edge.kind) &&
      filters.evidence.has(edge.evidence) &&
      visible.has(edge.from) &&
      visible.has(edge.to),
  );
  if (!filters.hideIsolated) return { nodes, edges, hiddenIsolated: 0 };
  const connected = new Set(edges.flatMap((edge) => [edge.from, edge.to]));
  const kept = nodes.filter((node) => connected.has(node.id) || node.matchesQuery);
  return { nodes: kept, edges, hiddenIsolated: nodes.length - kept.length };
}

export interface Highlight {
  nodes: ReadonlySet<string>;
  edges: ReadonlySet<string>;
}

export const EMPTY_HIGHLIGHT: Highlight = { nodes: new Set(), edges: new Set() };

/** The report's own explaining path for a symbol: changed root → … → symbol. */
export function explainingPath(report: AnalysisReport, id: string): { root: string; hops: Hop[] } | null {
  const impacted = report.impacted_symbols.find((s) => s.id === id);
  if (impacted) return { root: impacted.root, hops: impacted.path };
  const test = report.tests.find((t) => t.id === id);
  if (test) return { root: test.root, hops: test.path };
  return null;
}

export function highlightFor(report: AnalysisReport, id: string): Highlight {
  const path = explainingPath(report, id);
  const nodes = new Set<string>([id]);
  const edges = new Set<string>();
  if (path) {
    nodes.add(path.root);
    for (const hop of path.hops) {
      nodes.add(hop.edge.from);
      nodes.add(hop.edge.to);
      edges.add(edgeKey(hop.edge));
    }
  }
  return { nodes, edges };
}

/** Why an edge propagates impact, phrased for the inspector. */
export function edgeExplanation(edge: Edge, viaDispatch = false): string {
  if (viaDispatch || edge.kind === "OVERRIDES") {
    return "Callers bound to the overridden declaration can dispatch to this implementation at runtime, so a change to the implementation reaches them.";
  }
  switch (edge.kind) {
    case "CALLS":
      return "The caller executes the callee; a behaviour change in the callee is observable by the caller.";
    case "INSTANTIATES":
      return "The constructor runs whenever this code creates the object.";
    case "REFERENCES":
      return "The code names this type or field; changing its shape can change or break the referencing code.";
    case "EXTENDS":
    case "IMPLEMENTS":
      return "Subtypes inherit the supertype's contract; changing it changes what the subtype must satisfy.";
    case "IMPORTS":
      return "The file imports the type; this alone rarely changes behaviour.";
    case "TESTS":
      return "Recorded evidence that the test exercises this code.";
    case "CONTAINS":
      return "Structural containment; not a dependency.";
  }
}
