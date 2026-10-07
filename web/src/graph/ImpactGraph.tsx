import {
  Background,
  Controls,
  MarkerType,
  ReactFlow,
  type Edge as FlowEdge,
  type Node as FlowNode,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { useEffect, useMemo, useState } from "react";
import type { Edge } from "../api/types";
import { layoutGraph, moduleOfGroup, type LayoutResult } from "./layout";
import { edgeKey, type Highlight, type ViewGraph } from "./model";
import { ModuleNode, SymbolNode, type ModuleFlowNode, type SymbolFlowNode } from "./SymbolNode";

const nodeTypes = { symbol: SymbolNode, module: ModuleNode };

export type Selection = { type: "node"; id: string } | { type: "edge"; edge: Edge } | null;

interface Props {
  view: ViewGraph;
  highlight: Highlight;
  selection: Selection;
  groupByModule: boolean;
  onSelect: (selection: Selection) => void;
}

function edgeClass(edge: Edge, onPath: boolean, dimmed: boolean, selected: boolean): string {
  return [
    "edge",
    `edge--${edge.kind.toLowerCase()}`,
    edge.evidence === "RESOLVED_EXACT" ? "edge--exact" : "edge--inferred",
    onPath ? "edge--on-path" : "",
    dimmed ? "edge--dimmed" : "",
    selected ? "edge--selected" : "",
  ].join(" ");
}

export function ImpactGraph({ view, highlight, selection, groupByModule, onSelect }: Props) {
  const [layout, setLayout] = useState<LayoutResult | null>(null);
  const [layoutError, setLayoutError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    layoutGraph(view, groupByModule)
      .then((result) => {
        if (!cancelled) setLayout(result);
      })
      .catch((error: unknown) => {
        if (!cancelled) setLayoutError(error instanceof Error ? error.message : String(error));
      });
    return () => {
      cancelled = true;
    };
  }, [view, groupByModule]);

  const hasHighlight = highlight.nodes.size > 0;
  const selectedNode = selection?.type === "node" ? selection.id : null;
  const selectedEdge = selection?.type === "edge" ? edgeKey(selection.edge) : null;

  const nodes = useMemo<FlowNode[]>(() => {
    if (!layout) return [];
    const groups: ModuleFlowNode[] = layout.groups.map((group) => ({
      id: group.id,
      type: "module" as const,
      position: { x: group.x, y: group.y },
      data: { label: moduleOfGroup(group.id) },
      style: { width: group.width, height: group.height },
      selectable: false,
      draggable: false,
      focusable: false,
      zIndex: -1,
    }));
    const symbols: SymbolFlowNode[] = view.nodes.flatMap((node) => {
      const position = layout.nodes.get(node.id);
      if (!position) return [];
      return [
        {
          id: node.id,
          type: "symbol" as const,
          position: { x: position.x, y: position.y },
          data: {
            node,
            dimmed: hasHighlight && !highlight.nodes.has(node.id),
            onPath: hasHighlight && highlight.nodes.has(node.id),
            selected: selectedNode === node.id,
          },
          style: { width: position.width },
          draggable: false,
        },
      ];
    });
    return [...groups, ...symbols];
  }, [layout, view.nodes, highlight, hasHighlight, selectedNode]);

  const edges = useMemo<FlowEdge[]>(
    () =>
      view.edges.map((edge) => {
        const key = edgeKey(edge);
        const onPath = highlight.edges.has(key);
        return {
          id: key,
          source: edge.from,
          target: edge.to,
          label: edge.kind.toLowerCase(),
          className: edgeClass(edge, onPath, hasHighlight && !onPath, selectedEdge === key),
          markerEnd: { type: MarkerType.ArrowClosed, width: 14, height: 14 },
          data: { edge },
          focusable: true,
          zIndex: onPath ? 5 : 0,
        };
      }),
    [view.edges, highlight, hasHighlight, selectedEdge],
  );

  if (layoutError) {
    return <p className="graph__message">Graph layout failed: {layoutError}. The lists below remain complete.</p>;
  }
  if (view.nodes.length === 0) {
    return <p className="graph__message">No symbols to draw for the current filters.</p>;
  }

  return (
    <div className="graph" aria-label="Impact graph. The lists below contain the same information.">
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        fitView
        fitViewOptions={{ padding: 0.15 }}
        minZoom={0.15}
        maxZoom={2}
        nodesConnectable={false}
        elementsSelectable
        onlyRenderVisibleElements
        proOptions={{ hideAttribution: true }}
        onNodeClick={(_, node) => {
          if (node.type === "symbol") onSelect({ type: "node", id: node.id });
        }}
        onEdgeClick={(_, flowEdge) => {
          const edge = (flowEdge.data as { edge?: Edge } | undefined)?.edge;
          if (edge) onSelect({ type: "edge", edge });
        }}
        onPaneClick={() => {
          onSelect(null);
        }}
      >
        <Background gap={24} size={1} />
        <Controls showInteractive={false} />
      </ReactFlow>
    </div>
  );
}
