import {
  Background,
  Controls,
  MarkerType,
  ReactFlow,
  type Edge as FlowEdge,
  type Node as FlowNode,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { useEffect, useMemo, useRef, useState } from "react";
import type { Edge } from "../api/types";
import { isClusterId, moduleOfCluster, type DisplayEdge, type DisplayGraph } from "./clusters";
import { layoutGraph, layoutKey, moduleOfGroup, type LayoutResult } from "./layout";
import { Legend } from "./Legend";
import { edgeKey, type Highlight } from "./model";
import {
  ClusterNodeComponent,
  ModuleNode,
  SymbolNode,
  type ClusterFlowNode,
  type ModuleFlowNode,
  type SymbolFlowNode,
} from "./SymbolNode";

const nodeTypes = { symbol: SymbolNode, module: ModuleNode, cluster: ClusterNodeComponent };

export type Selection = { type: "node"; id: string } | { type: "edge"; edge: Edge } | null;

interface Props {
  graph: DisplayGraph;
  highlight: Highlight;
  selection: Selection;
  groupByModule: boolean;
  onSelect: (selection: Selection) => void;
  onExpandModule: (module: string) => void;
}

function edgeClass(edge: DisplayEdge, onPath: boolean, dimmed: boolean, selected: boolean): string {
  const first = edge.edges[0];
  const kinds = new Set(edge.edges.map((e) => e.kind));
  const evidence = new Set(edge.edges.map((e) => e.evidence));
  return [
    "edge",
    kinds.size === 1 && first ? `edge--${first.kind.toLowerCase()}` : "edge--mixed",
    // An aggregated line is only drawn solid when every edge it stands for is exact.
    evidence.size === 1 && first ? `edge--ev-${first.evidence.toLowerCase()}` : "edge--ev-mixed",
    edge.edges.length > 1 ? "edge--aggregate" : "",
    onPath ? "edge--on-path" : "",
    dimmed ? "edge--dimmed" : "",
    selected ? "edge--selected" : "",
  ].join(" ");
}

export function ImpactGraph({ graph, highlight, selection, groupByModule, onSelect, onExpandModule }: Props) {
  const [layout, setLayout] = useState<{ key: string; result: LayoutResult } | null>(null);
  const [layoutError, setLayoutError] = useState<string | null>(null);
  const key = useMemo(() => layoutKey(graph, groupByModule), [graph, groupByModule]);
  // The latest graph for the layout effect, which must only re-run when the key changes.
  const graphRef = useRef(graph);
  useEffect(() => {
    graphRef.current = graph;
  }, [graph]);

  useEffect(() => {
    let cancelled = false;
    layoutGraph(graphRef.current, groupByModule)
      .then((result) => {
        if (!cancelled) setLayout({ key, result });
      })
      .catch((error: unknown) => {
        if (!cancelled) setLayoutError(error instanceof Error ? error.message : String(error));
      });
    return () => {
      cancelled = true;
    };
  }, [key, groupByModule]);

  const hasHighlight = highlight.nodes.size > 0;
  const selectedNode = selection?.type === "node" ? selection.id : null;
  const selectedEdge = selection?.type === "edge" ? edgeKey(selection.edge) : null;
  const positions = layout?.result;

  const nodes = useMemo<FlowNode[]>(() => {
    if (!positions) return [];
    const groups: ModuleFlowNode[] = positions.groups.map((group) => ({
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
    const symbols: SymbolFlowNode[] = graph.symbols.flatMap((node) => {
      const position = positions.nodes.get(node.id);
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
          ariaLabel: `${node.label}, ${node.role}${node.change ? ` ${node.change.toLowerCase()}` : ""}`,
        },
      ];
    });
    const clusters: ClusterFlowNode[] = graph.clusters.flatMap((cluster) => {
      const position = positions.nodes.get(cluster.id);
      if (!position) return [];
      const onPath = hasHighlight && cluster.members.some((m) => highlight.nodes.has(m));
      return [
        {
          id: cluster.id,
          type: "cluster" as const,
          position: { x: position.x, y: position.y },
          data: { cluster, dimmed: hasHighlight && !onPath, onPath },
          style: { width: position.width },
          draggable: false,
          ariaLabel: `Module ${cluster.module}, ${cluster.members.length} symbols, collapsed`,
        },
      ];
    });
    return [...groups, ...symbols, ...clusters];
  }, [positions, graph.symbols, graph.clusters, highlight, hasHighlight, selectedNode]);

  const edges = useMemo<FlowEdge[]>(
    () =>
      graph.edges.map((edge) => {
        const onPath = edge.edges.some((e) => highlight.edges.has(edgeKey(e)));
        const first = edge.edges[0];
        // Relation labels only where the eye is: on the highlighted path, the selected edge and
        // aggregates. Labelling every edge piled words on each crossing; line styles carry the
        // evidence class everywhere and the Edges tab lists every relation as text.
        const label =
          edge.edges.length > 1
            ? `${edge.edges.length} edges`
            : first && (onPath || selectedEdge === edge.id)
              ? first.kind.toLowerCase()
              : undefined;
        return {
          id: edge.id,
          source: edge.from,
          target: edge.to,
          ...(label === undefined ? {} : { label }),
          className: edgeClass(edge, onPath, hasHighlight && !onPath, selectedEdge === edge.id),
          markerEnd: { type: MarkerType.ArrowClosed, width: 14, height: 14 },
          data: { edge },
          focusable: true,
          zIndex: onPath ? 5 : 0,
        };
      }),
    [graph.edges, highlight, hasHighlight, selectedEdge],
  );

  if (layoutError) {
    return <p className="graph__message">Graph layout failed: {layoutError}. The tables below remain complete.</p>;
  }
  if (graph.symbols.length === 0 && graph.clusters.length === 0) {
    return <p className="graph__message">No symbols to draw for the current filters.</p>;
  }

  return (
    <div
      className={`graph ${layout?.key === key ? "" : "graph--stale"}`}
      role="figure"
      aria-label="Impact graph. The Nodes and Edges tabs below list the same information."
    >
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        fitView
        fitViewOptions={{ padding: 0.06 }}
        minZoom={0.15}
        maxZoom={2}
        nodesConnectable={false}
        elementsSelectable
        onlyRenderVisibleElements
        proOptions={{ hideAttribution: true }}
        onNodeClick={(_, node) => {
          if (node.type === "symbol") onSelect({ type: "node", id: node.id });
          else if (node.type === "cluster" && isClusterId(node.id)) onExpandModule(moduleOfCluster(node.id));
        }}
        onEdgeClick={(_, flowEdge) => {
          const display = (flowEdge.data as { edge?: DisplayEdge } | undefined)?.edge;
          if (!display) return;
          const [only, ...rest] = display.edges;
          if (only && rest.length === 0) {
            onSelect({ type: "edge", edge: only });
            return;
          }
          // An aggregate stands for several edges: open the clusters so each can be inspected.
          for (const end of [display.from, display.to]) {
            if (isClusterId(end)) onExpandModule(moduleOfCluster(end));
          }
        }}
        onPaneClick={() => {
          onSelect(null);
        }}
      >
        <Background gap={24} size={1} />
        <Controls showInteractive={false} />
      </ReactFlow>
      <Legend />
    </div>
  );
}
