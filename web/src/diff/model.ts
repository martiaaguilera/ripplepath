// Unified-diff rows from the report's own hunks and the two file texts. The line diff itself is the
// engine's (`files[].hunks`); this only lays its hunks out with context, so the UI can never show a
// different set of changed lines than the analysis used.

import type { HunkReport } from "../api/types";

export type DiffRow =
  | { type: "gap"; lines: number }
  | { type: "hunk"; index: number; hunk: HunkReport }
  | { type: "context"; old: number; new: number; text: string }
  | { type: "del"; old: number; text: string }
  | { type: "add"; new: number; text: string };

export const CONTEXT_LINES = 3;

/** Lines without their terminators; a final newline does not start an extra empty line. */
export function splitLines(text: string): string[] {
  const lines = text.split("\n").map((line) => (line.endsWith("\r") ? line.slice(0, -1) : line));
  if (lines.length > 0 && lines[lines.length - 1] === "") lines.pop();
  return lines;
}

/** First changed line on a side: a zero-length side names the line *after which* the change sits. */
function firstLine(start: number, len: number): number {
  return len === 0 ? start + 1 : start;
}

/**
 * `base`/`head` are the file's lines on each side (`null` when the side does not exist or could not
 * be read). Context comes from head when available — unchanged lines are identical on both sides.
 */
export function buildDiffRows(
  base: readonly string[] | null,
  head: readonly string[] | null,
  hunks: readonly HunkReport[],
  context = CONTEXT_LINES,
): DiffRow[] {
  const rows: DiffRow[] = [];
  const contextSide = head ?? base ?? [];
  const contextIsHead = head !== null;
  const total = contextSide.length;
  const sorted = [...hunks].sort((a, b) => a.new_start - b.new_start || a.old_start - b.old_start);
  // Position in the context side's numbering of the last line already emitted.
  let shown = 0;

  const contextRow = (oldLine: number, newLine: number): DiffRow => ({
    type: "context",
    old: oldLine,
    new: newLine,
    text: contextSide[(contextIsHead ? newLine : oldLine) - 1] ?? "",
  });

  sorted.forEach((hunk, index) => {
    const oldFirst = firstLine(hunk.old_start, hunk.old_len);
    const newFirst = firstLine(hunk.new_start, hunk.new_len);
    const sideFirst = contextIsHead ? newFirst : oldFirst;
    // Offset between the numberings in the unchanged region before this hunk.
    const before = newFirst - oldFirst;
    const from = Math.max(shown + 1, sideFirst - context);
    if (from > shown + 1) rows.push({ type: "gap", lines: from - shown - 1 });
    rows.push({ type: "hunk", index, hunk });
    for (let line = from; line < sideFirst; line += 1) {
      rows.push(contextIsHead ? contextRow(line - before, line) : contextRow(line, line + before));
    }
    for (let line = oldFirst; line < oldFirst + hunk.old_len; line += 1) {
      rows.push({ type: "del", old: line, text: base?.[line - 1] ?? "" });
    }
    for (let line = newFirst; line < newFirst + hunk.new_len; line += 1) {
      rows.push({ type: "add", new: line, text: head?.[line - 1] ?? "" });
    }
    const after = newFirst + hunk.new_len - (oldFirst + hunk.old_len);
    const sideEnd = contextIsHead ? newFirst + hunk.new_len : oldFirst + hunk.old_len;
    const next = sorted[index + 1];
    const nextFirst = next
      ? contextIsHead
        ? firstLine(next.new_start, next.new_len)
        : firstLine(next.old_start, next.old_len)
      : total + 1;
    const to = Math.min(total, sideEnd + context - 1, nextFirst - 1);
    for (let line = sideEnd; line <= to; line += 1) {
      rows.push(contextIsHead ? contextRow(line - after, line) : contextRow(line, line + after));
    }
    shown = Math.max(shown, to, sideEnd - 1);
  });
  if (sorted.length > 0 && shown < total) rows.push({ type: "gap", lines: total - shown });
  return rows;
}
