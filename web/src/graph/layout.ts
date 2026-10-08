// Layered layout with ELK. Edges point from dependent to dependency, and ELK's `UP` direction puts
// edge targets above their sources, so the changed code sits on top and its blast radius grows
// downwards, one row per depth. Rows rather than columns: a blast radius is usually deeper than it
// is wide per level, and a left-to-right layout of six levels had to be zoomed out until labels
// were unreadable in the centre column.

import type { ElkExtendedEdge, ElkNode } from "elkjs/lib/elk-api";
import type { DisplayGraph } from "./clusters";
import type { ViewNode } from "./model";

export const NODE_HEIGHT = 46;
const MIN_WIDTH = 150;
const MAX_WIDTH = 220;
const CHAR_WIDTH = 7;

export function nodeWidth(node: Pick<ViewNode, "label">): number {
  return Math.round(Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, node.label.length * CHAR_WIDTH + 36)));
}

export interface Positioned {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface LayoutResult {
  nodes: Map<string, Positioned>;
  /** Module group boxes, present when grouping is on. */
  groups: Positioned[];
}

let elkInstance: Promise<{ layout: (graph: ElkNode) => Promise<ElkNode> }> | null = null;

function elk() {
  // Loaded on demand: ELK is the single largest dependency and only the graph view needs it.
  elkInstance ??= import("elkjs/lib/elk.bundled.js").then(({ default: ELK }) => new ELK());
  return elkInstance;
}

const LAYOUT_OPTIONS = {
  "elk.algorithm": "layered",
  "elk.direction": "UP",
  "elk.layered.spacing.nodeNodeBetweenLayers": "56",
  "elk.spacing.nodeNode": "16",
  "elk.layered.nodePlacement.strategy": "NETWORK_SIMPLEX",
  "elk.layered.considerModelOrder.strategy": "NODES_AND_EDGES",
  "elk.hierarchyHandling": "INCLUDE_CHILDREN",
  // Unrelated changes form separate components; pack them into rows instead of one long strip.
  "elk.separateConnectedComponents": "true",
  "elk.aspectRatio": "1.6",
  "elk.padding": "[top=34,left=14,bottom=14,right=14]",
};

/** Last segment of a module name, for compact cluster labels. */
export function clusterLabel(module: string): string {
  return module.split(/[./]/).pop() ?? module;
}

/**
 * Identity of a layout. Highlighting, search matches and selection do not change it, so the
 * previous positions are kept and the picture does not jump while the user explores.
 */
export function layoutKey(graph: DisplayGraph, groupByModule: boolean): string {
  return [
    groupByModule ? "grouped" : "flat",
    graph.symbols.map((n) => n.id).join("\n"),
    graph.clusters.map((c) => `${c.id}#${c.members.length}`).join("\n"),
    graph.edges.map((e) => e.id).join("\n"),
  ].join("\u0000");
}

export async function layoutGraph(graph: DisplayGraph, groupByModule: boolean): Promise<LayoutResult> {
  // Sorted input so ELK's model-order tie breaking gives the same picture for the same report.
  const nodes = [...graph.symbols].sort((a, b) => a.id.localeCompare(b.id));
  const leaf = (node: ViewNode): ElkNode => ({ id: node.id, width: nodeWidth(node), height: NODE_HEIGHT });
  const clusterLeaves: ElkNode[] = [...graph.clusters]
    .sort((a, b) => a.id.localeCompare(b.id))
    .map((c) => ({
      id: c.id,
      width: nodeWidth({ label: `${clusterLabel(c.module)} · ${c.members.length} symbols` }),
      height: NODE_HEIGHT,
    }));
  const edges: ElkExtendedEdge[] = graph.edges.map((edge) => ({
    id: edge.id,
    sources: [edge.from],
    targets: [edge.to],
  }));

  let children: ElkNode[];
  if (groupByModule) {
    const modules = new Map<string, ViewNode[]>();
    for (const node of nodes) {
      const members = modules.get(node.module) ?? [];
      members.push(node);
      modules.set(node.module, members);
    }
    children = [
      ...[...modules.entries()]
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([module, members]) => ({
          id: groupId(module),
          layoutOptions: { "elk.padding": "[top=30,left=12,bottom=12,right=12]" },
          children: members.map(leaf),
        })),
      ...clusterLeaves,
    ];
  } else {
    children = [...nodes.map(leaf), ...clusterLeaves];
  }

  const result = await (await elk()).layout({
    id: "root",
    layoutOptions: LAYOUT_OPTIONS,
    children,
    edges,
  });

  const positioned = new Map<string, Positioned>();
  const groups: Positioned[] = [];
  for (const child of result.children ?? []) {
    const x = child.x ?? 0;
    const y = child.y ?? 0;
    if (child.children && child.children.length > 0) {
      groups.push({ id: child.id, x, y, width: child.width ?? 0, height: child.height ?? 0 });
      for (const inner of child.children) {
        positioned.set(inner.id, {
          id: inner.id,
          x: x + (inner.x ?? 0),
          y: y + (inner.y ?? 0),
          width: inner.width ?? 0,
          height: inner.height ?? 0,
        });
      }
    } else {
      positioned.set(child.id, { id: child.id, x, y, width: child.width ?? 0, height: child.height ?? 0 });
    }
  }
  return { nodes: positioned, groups };
}

export function groupId(module: string): string {
  return `module:${module}`;
}

export function moduleOfGroup(id: string): string {
  return id.slice("module:".length);
}
