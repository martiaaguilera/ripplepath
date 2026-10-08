// Geometry for the layered architecture diagram. Pure: layers stack top to bottom in the order the
// configuration declares them; layer-to-layer dependencies become bracket-shaped arcs, downward ones
// on the right and upward ones (against the stack) on the left, each in its own lane so arcs that
// span overlapping layers never share a vertical segment.

import type { ArchitectureReport, DeltaStatus } from "../api/types";

export type DiagramMode = "head" | "base" | "delta";

export const BOX_WIDTH = 340;
export const BOX_HEIGHT = 58;
const ROW_GAP = 40;
const TOP = 16;
const LANE_GAP = 34;
const FIRST_LANE = 30;

export interface LayerBox {
  name: string;
  x: number;
  y: number;
  base: number;
  head: number;
}

export interface LayerArc {
  key: string;
  from: string;
  to: string;
  base: number;
  head: number;
  /** Strongest violation status on this layer pair, if any rule is broken by it. */
  status: DeltaStatus | null;
  violations: number;
  side: "down" | "up";
  lane: number;
  /** Polyline points: out of the source box, along the lane, into the target box. */
  points: [number, number][];
  labelX: number;
  labelY: number;
}

export interface Diagram {
  width: number;
  height: number;
  boxes: LayerBox[];
  arcs: LayerArc[];
}

const STATUS_RANK: Record<DeltaStatus, number> = { NEW: 0, PRE_EXISTING: 1, REMOVED: 2 };

export function arcVisible(arc: Pick<LayerArc, "base" | "head" | "status">, mode: DiagramMode): boolean {
  if (mode === "head") return arc.head > 0;
  if (mode === "base") return arc.base > 0;
  return arc.base > 0 || arc.head > 0;
}

export function buildDiagram(arch: ArchitectureReport, mode: DiagramMode): Diagram {
  const order = new Map(arch.layers.map((layer, index) => [layer.name, index]));
  const statusByPair = new Map<string, { status: DeltaStatus; count: number }>();
  for (const violation of arch.violations) {
    const key = `${violation.from_layer}\u0000${violation.to_layer}`;
    const current = statusByPair.get(key);
    const status =
      current && STATUS_RANK[current.status] <= STATUS_RANK[violation.status] ? current.status : violation.status;
    statusByPair.set(key, { status, count: (current?.count ?? 0) + 1 });
  }

  type Pending = Omit<LayerArc, "lane" | "points" | "labelX" | "labelY"> & { a: number; b: number };
  const pending: Pending[] = [];
  for (const dep of arch.layer_dependencies) {
    const a = order.get(dep.from);
    const b = order.get(dep.to);
    if (a === undefined || b === undefined || a === b) continue;
    const key = `${dep.from}\u0000${dep.to}`;
    const violation = statusByPair.get(key);
    const arc = {
      key,
      from: dep.from,
      to: dep.to,
      base: dep.base_edges,
      head: dep.head_edges,
      status: violation?.status ?? null,
      violations: violation?.count ?? 0,
      side: a < b ? ("down" as const) : ("up" as const),
      a,
      b,
    };
    if (arcVisible(arc, mode)) pending.push(arc);
  }

  // Lanes per side: shorter spans hug the boxes; an arc takes the first lane whose arcs do not
  // overlap its vertical extent. Sorted first so the same report always yields the same drawing.
  pending.sort(
    (p, q) =>
      Math.abs(p.a - p.b) - Math.abs(q.a - q.b) ||
      Math.min(p.a, p.b) - Math.min(q.a, q.b) ||
      p.key.localeCompare(q.key),
  );
  const lanes: Record<"down" | "up", [number, number][][]> = { down: [], up: [] };
  const laneOf = (arc: Pending): number => {
    const lo = Math.min(arc.a, arc.b);
    const hi = Math.max(arc.a, arc.b);
    const sideLanes = lanes[arc.side];
    for (let lane = 0; ; lane += 1) {
      const used = sideLanes[lane] ?? [];
      if (used.every(([l, h]) => hi <= l || lo >= h)) {
        used.push([lo, hi]);
        sideLanes[lane] = used;
        return lane;
      }
    }
  };
  const assigned = pending.map((arc) => ({ arc, lane: laneOf(arc) }));
  const leftLanes = lanes.up.length;
  const rightLanes = lanes.down.length;
  const boxX = FIRST_LANE + Math.max(leftLanes, 1) * LANE_GAP + 24;
  const rowY = (index: number) => TOP + index * (BOX_HEIGHT + ROW_GAP);

  const boxes: LayerBox[] = arch.layers.map((layer, index) => ({
    name: layer.name,
    x: boxX,
    y: rowY(index),
    base: layer.base_symbols,
    head: layer.head_symbols,
  }));

  const arcs: LayerArc[] = assigned.map(({ arc, lane }) => {
    const { a, b, ...rest } = arc;
    const down = arc.side === "down";
    const edgeX = down ? boxX + BOX_WIDTH : boxX;
    const laneX = down ? edgeX + FIRST_LANE + lane * LANE_GAP : edgeX - FIRST_LANE - lane * LANE_GAP;
    // Leave the source below its centre and enter the target above it (mirrored upwards), so an
    // arc's two ends never coincide with the opposite arc between the same layers.
    const y1 = rowY(a) + BOX_HEIGHT * (down ? 0.68 : 0.32);
    const y2 = rowY(b) + BOX_HEIGHT * (down ? 0.32 : 0.68);
    return {
      ...rest,
      lane,
      points: [
        [edgeX, y1],
        [laneX, y1],
        [laneX, y2],
        [edgeX, y2],
      ],
      labelX: laneX,
      labelY: (y1 + y2) / 2,
    };
  });
  arcs.sort((p, q) => p.key.localeCompare(q.key));

  return {
    width: boxX + BOX_WIDTH + FIRST_LANE + Math.max(rightLanes, 1) * LANE_GAP + 24,
    height: rowY(arch.layers.length) - ROW_GAP + TOP,
    boxes,
    arcs,
  };
}
