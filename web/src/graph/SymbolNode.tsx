import { Handle, Position, type Node, type NodeProps } from "@xyflow/react";
import { memo } from "react";
import type { ViewNode } from "./model";

export type SymbolNodeData = { node: ViewNode; dimmed: boolean; onPath: boolean; selected: boolean };
export type SymbolFlowNode = Node<SymbolNodeData, "symbol">;

const KIND_GLYPH: Record<ViewNode["kind"], string> = {
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

function roleClass(node: ViewNode): string {
  if (node.role === "changed") return `node--changed node--${(node.change ?? "MODIFIED").toLowerCase()}`;
  if (node.is_test) return "node--test";
  return node.depth === 1 ? "node--direct" : "node--transitive";
}

function roleText(node: ViewNode): string {
  if (node.role === "changed") return (node.change ?? "MODIFIED").replace("_", " ").toLowerCase();
  if (node.is_test) return `test · depth ${node.depth}`;
  return node.depth === 1 ? "direct dependent" : `transitive · depth ${node.depth}`;
}

// Edges point dependent → dependency and flow right-to-left, so a node's outgoing handle is on its
// left and its incoming handle on its right.
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
      <Handle type="source" position={Position.Left} className="node__handle" />
      <span className="node__glyph" aria-hidden="true">
        {KIND_GLYPH[node.kind]}
      </span>
      <span className="node__text">
        <span className="node__label">{node.label}</span>
        <span className="node__meta">{roleText(node)}</span>
      </span>
      <Handle type="target" position={Position.Right} className="node__handle" />
    </div>
  );
}

export const SymbolNode = memo(SymbolNodeView);

export type ModuleFlowNode = Node<{ label: string }, "module">;

function ModuleNodeView({ data }: NodeProps<ModuleFlowNode>) {
  return (
    <div className="module-group">
      <span className="module-group__label">{data.label}</span>
    </div>
  );
}

export const ModuleNode = memo(ModuleNodeView);
