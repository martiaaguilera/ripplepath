// Fixed-height row windowing for long tables. Large repositories recommend thousands of tests; a
// dependency would be justified for variable heights or horizontal virtualisation, but fixed rows in
// one scroll container need only arithmetic, and real <table> markup stays intact (spacer rows), so
// screen readers still see a table.

import { useEffect, useState } from "react";

export const WINDOWING_THRESHOLD = 150;
const OVERSCAN = 10;

export interface WindowedRows {
  /** Callback ref for the scroll container. A callback (not a ref object) so the returned window
   * is plain render data: nothing here reads a ref during render. */
  setContainer: (element: HTMLDivElement | null) => void;
  start: number;
  end: number;
  padTop: number;
  padBottom: number;
  windowed: boolean;
}

export function useWindowedRows(count: number, rowHeight: number): WindowedRows {
  const [container, setContainer] = useState<HTMLDivElement | null>(null);
  const windowed = count > WINDOWING_THRESHOLD;
  const [range, setRange] = useState({ start: 0, end: 60 });

  useEffect(() => {
    if (!container || !windowed) return;
    const update = () => {
      const start = Math.max(0, Math.floor(container.scrollTop / rowHeight) - OVERSCAN);
      const visible = Math.ceil(container.clientHeight / rowHeight) + 2 * OVERSCAN;
      setRange((current) =>
        current.start === start && current.end === start + visible ? current : { start, end: start + visible },
      );
    };
    // Measured asynchronously (scroll and resize callbacks) so the effect itself never sets state.
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(update);
    observer?.observe(container);
    container.addEventListener("scroll", update, { passive: true });
    return () => {
      observer?.disconnect();
      container.removeEventListener("scroll", update);
    };
  }, [container, windowed, rowHeight]);

  if (!windowed) return { setContainer, start: 0, end: count, padTop: 0, padBottom: 0, windowed };
  const start = Math.min(range.start, count);
  const end = Math.min(range.end, count);
  return { setContainer, start, end, padTop: start * rowHeight, padBottom: (count - end) * rowHeight, windowed };
}
