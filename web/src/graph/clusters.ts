// Module collapsing for the impact graph. A collapsed module is drawn as one cluster node; edges that
// touch its members are re-attached to the cluster and merged per endpoint pair. This only changes
// how the report's edges are grouped on screen: every underlying edge stays reachable (aggregated
// edges keep their members, and the edge table lists them all).

import type { Edge } from "../api/types";
import { edgeKey, type ViewGraph, type ViewNode } from "./model";

export interface ClusterNode {
  id: string;
  module: string;
  /** Member symbol ids, sorted. */
  members: string[];
  changed: number;
  tests: number;
  minDepth: number;
  matchesQuery: boolean;
}

export interface DisplayEdge {
  id: string;
  from: string;
  to: string;
  /** The report edges this line stands for, sorted by key; one unless endpoints are clusters. */
  edges: Edge[];
}

export interface DisplayGraph {
  symbols: ViewNode[];
  clusters: ClusterNode[];
  edges: DisplayEdge[];
}

const CLUSTER_PREFIX = "cluster:";

export function clusterId(module: string): string {
  return `${CLUSTER_PREFIX}${module}`;
}

export function isClusterId(id: string): boolean {
  return id.startsWith(CLUSTER_PREFIX);
}

export function moduleOfCluster(id: string): string {
  return id.slice(CLUSTER_PREFIX.length);
}

function single(edge: Edge): DisplayEdge {
  return { id: edgeKey(edge), from: edge.from, to: edge.to, edges: [edge] };
}

/**
 * `collapse`: draw every module with two or more visible symbols as a cluster, except those in
 * `expanded`. A one-symbol module is never collapsed: a cluster would hide its name and save nothing.
 */
export function displayGraph(view: ViewGraph, collapse: boolean, expanded: ReadonlySet<string>): DisplayGraph {
  if (!collapse) return { symbols: view.nodes, clusters: [], edges: view.edges.map(single) };

  const byModule = new Map<string, ViewNode[]>();
  for (const node of view.nodes) {
    const members = byModule.get(node.module) ?? [];
    members.push(node);
    byModule.set(node.module, members);
  }
  const owner = new Map<string, string>();
  const symbols: ViewNode[] = [];
  const clusters: ClusterNode[] = [];
  for (const [module, members] of [...byModule.entries()].sort(([a], [b]) => a.localeCompare(b))) {
    if (members.length < 2 || expanded.has(module)) {
      for (const node of members) owner.set(node.id, node.id);
      symbols.push(...members);
      continue;
    }
    const id = clusterId(module);
    for (const node of members) owner.set(node.id, id);
    clusters.push({
      id,
      module,
      members: members.map((n) => n.id).sort(),
      changed: members.filter((n) => n.role === "changed").length,
      tests: members.filter((n) => n.is_test).length,
      minDepth: Math.min(...members.map((n) => n.depth)),
      matchesQuery: members.some((n) => n.matchesQuery),
    });
  }

  const merged = new Map<string, DisplayEdge>();
  for (const edge of view.edges) {
    const from = owner.get(edge.from);
    const to = owner.get(edge.to);
    // Both endpoints are visible by construction of the view; edges inside one cluster are not
    // drawn (they are still listed in the edge table).
    if (from === undefined || to === undefined || from === to) continue;
    if (from === edge.from && to === edge.to) {
      merged.set(edgeKey(edge), single(edge));
      continue;
    }
    const id = `agg:${from}|${to}`;
    const existing = merged.get(id);
    if (existing) existing.edges.push(edge);
    else merged.set(id, { id, from, to, edges: [edge] });
  }
  const edges = [...merged.values()].sort((a, b) => a.id.localeCompare(b.id));
  for (const edge of edges) edge.edges.sort((a, b) => edgeKey(a).localeCompare(edgeKey(b)));
  symbols.sort((a, b) => a.id.localeCompare(b.id));
  return { symbols, clusters, edges };
}
