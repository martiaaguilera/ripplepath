import type { HunkReport } from "../api/types";
import { buildDiffRows, splitLines } from "./model";

const hunk = (old_start: number, old_len: number, new_start: number, new_len: number): HunkReport => ({
  old_start,
  old_len,
  new_start,
  new_len,
  base_symbols: [],
  head_symbols: [],
});

describe("splitLines", () => {
  it("drops terminators and the empty line after a final newline", () => {
    expect(splitLines("a\r\nb\n")).toEqual(["a", "b"]);
    expect(splitLines("a\n\nb")).toEqual(["a", "", "b"]);
    expect(splitLines("")).toEqual([]);
  });
});

describe("buildDiffRows", () => {
  const base = ["a", "b", "c", "d", "e", "f", "g", "h"];
  const head = ["a", "b", "X", "d", "e", "f", "g", "h", "i"];

  it("lays out the engine's hunks with context and gaps, numbering both sides", () => {
    const rows = buildDiffRows(base, head, [hunk(3, 1, 3, 1), hunk(8, 0, 9, 1)], 2);
    expect(rows.map((r) => (r.type === "hunk" ? `@${r.index}` : r.type === "gap" ? `…${r.lines}` : r.type))).toEqual([
      "@0",
      "context",
      "context",
      "del",
      "add",
      "context",
      "context",
      "…1",
      "@1",
      "context",
      "context",
      "add",
    ]);
    expect(rows[3]).toEqual({ type: "del", old: 3, text: "c" });
    expect(rows[4]).toEqual({ type: "add", new: 3, text: "X" });
    expect(rows[9]).toEqual({ type: "context", old: 7, new: 7, text: "g" });
    expect(rows[11]).toEqual({ type: "add", new: 9, text: "i" });
  });

  it("keeps old and new numbering apart after an insertion", () => {
    const rows = buildDiffRows(["a", "b", "c"], ["a", "new", "b", "c"], [hunk(1, 0, 2, 1)], 3);
    expect(rows.filter((r) => r.type === "context")).toEqual([
      { type: "context", old: 1, new: 1, text: "a" },
      { type: "context", old: 2, new: 3, text: "b" },
      { type: "context", old: 3, new: 4, text: "c" },
    ]);
  });

  it("shows an added file as additions only and a deleted file as deletions only", () => {
    expect(buildDiffRows(null, ["x", "y"], [hunk(0, 0, 1, 2)]).map((r) => r.type)).toEqual(["hunk", "add", "add"]);
    expect(buildDiffRows(["x", "y"], null, [hunk(1, 2, 0, 0)]).map((r) => r.type)).toEqual(["hunk", "del", "del"]);
  });

  it("reports trailing unchanged lines as a gap", () => {
    const lines = Array.from({ length: 20 }, (_, i) => `l${i + 1}`);
    const changed = [...lines];
    changed[0] = "L1";
    const rows = buildDiffRows(lines, changed, [hunk(1, 1, 1, 1)], 3);
    expect(rows[rows.length - 1]).toEqual({ type: "gap", lines: 16 });
  });
});
