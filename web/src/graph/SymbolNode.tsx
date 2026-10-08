import { Handle, Position, type Node, type NodeProps } from "@xyflow/react";
import { memo } from "react";
import type { ClusterNode } from "./clusters";
import { clusterLabel } from "./layout";
import type { ViewNode } from "./model";

export type SymbolNodeData = { node: ViewNode; dimmed: boolean; onPath: boolean; selected: boolean };
export type SymbolFlowNode = Node<SymbolNodeData, "symbol">;

export const KIND_GLYPH: Record<ViewNode["kind"], string> = {
  file: "F",
  class: "C",
  interface: "I",
  enum: "E",
  record: "R",
  annotation: "@",
  method: "m",
  constructor: "c",
  field: "f",
  function: "ƒ",
  variable: "v",
  type_alias: "T",
  test_case: "t",
};

export function roleClass(node: Pick<ViewNode, "role" | "change" | "is_test" | "depth">): string {
  if (node.role === "changed") return `node--changed node--${(node.change ?? "MODIFIED").toLowerCase()}`;
  if (node.is_test) return "node--test";
  return node.depth === 1 ? "node--direct" : "node--transitive";
}

export function roleText(node: Pick<ViewNode, "role" | "change" | "is_test" | "depth">): string {
  if (node.role === "changed") return (node.change ?? "MODIFIED").replace("_", " ").toLowerCase();
  if (node.is_test) return `test · depth ${node.depth}`;
  return node.depth === 1 ? "direct dependent" : `transitive · depth ${node.depth}`;
}

// Edges point dependent → dependency and flow bottom-to-top (layout.ts), so a node's outgoing
// handle is on its top edge and its incoming handle on its bottom edge.
function SymbolNodeView({ data }: NodeProps<SymbolFlowNode>) {
  const { node } = data;
  const classes = [
    "node",
    roleClass(node),
    data.dimmed ? "node--dimmed" : "",
    data.onPath ? "node--on-path" : "",
    data.selected ? "node--selected" : "",
    node.matchesQuery ? "node--match" : "",
  ].join(" ");
  return (
    <div className={classes} title={node.id}>
      <Handle type="source" position={Position.Top} className="node__handle" />
      <span className="node__glyph" aria-hidden="true">
        {KIND_GLYPH[node.kind]}
      </span>
      <span className="node__text">
        <span className="node__label">{node.label}</span>
        <span className="node__meta">{roleText(node)}</span>
      </span>
      <Handle type="target" position={Position.Bottom} className="node__handle" />
    </div>
  );
}

export const SymbolNode = memo(SymbolNodeView);

export type ClusterNodeData = { cluster: ClusterNode; dimmed: boolean; onPath: boolean };
export type ClusterFlowNode = Node<ClusterNodeData, "cluster">;

function ClusterNodeView({ data }: NodeProps<ClusterFlowNode>) {
  const { cluster } = data;
  const classes = [
    "node",
    "node--cluster",
    cluster.changed > 0 ? "node--cluster-changed" : "",
    data.dimmed ? "node--dimmed" : "",
    data.onPath ? "node--on-path" : "",
    cluster.matchesQuery ? "node--match" : "",
  ].join(" ");
  const details = [
    `${cluster.members.length} symbols`,
    cluster.changed > 0 ? `${cluster.changed} changed` : "",
    cluster.tests > 0 ? `${cluster.tests} tests` : "",
  ].filter(Boolean);
  return (
    <div className={classes} title={`${cluster.module} — click to expand`}>
      <Handle type="source" position={Position.Top} className="node__handle" />
      <span className="node__glyph node__glyph--cluster" aria-hidden="true">
        {cluster.members.length}
      </span>
      <span className="node__text">
        <span className="node__label">{clusterLabel(cluster.module)}</span>
        <span className="node__meta">{details.join(" · ")}</span>
      </span>
      <Handle type="target" position={Position.Bottom} className="node__handle" />
    </div>
  );
}

export const ClusterNodeComponent = memo(ClusterNodeView);

export type ModuleFlowNode = Node<{ label: string }, "module">;

function ModuleNodeView({ data }: NodeProps<ModuleFlowNode>) {
  return (
    <div className="module-group">
      <span className="module-group__label">{data.label}</span>
    </div>
  );
}

export const ModuleNode = memo(ModuleNodeView);
