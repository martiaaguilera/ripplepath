// Diff with semantic annotation: each hunk of the engine's line diff, the symbols that contain it,
// and what the report says about those symbols (dependents, tests, coverage, layer, risk signals).
// Source text is only ever rendered as React text nodes, never as HTML.

import { useEffect, useMemo, useState } from "react";
import { fetchFile } from "../api/client";
import type { AnalysisReport, FileBlob, FileChange, HunkReport } from "../api/types";
import {
  dependentsInSlice,
  indexReport,
  layersNamedFor,
  signalsNaming,
  testsReaching,
} from "../app/reportIndex";
import type { Navigate, UrlState } from "../app/url";
import { CoverageBadge, Empty, TierBadge, humanize } from "../components/common";
import { CHANGE_LABEL } from "../components/Panels";
import { buildDiffRows, splitLines, type DiffRow } from "../diff/model";
import { shortLabel } from "../graph/model";

interface Props {
  report: AnalysisReport;
  url: UrlState;
  navigate: Navigate;
}

type Side = { kind: "absent" } | { kind: "blob"; blob: FileBlob };
type Loaded = { status: "ready"; base: Side; head: Side } | { status: "error"; message: string };

function revOf(info: AnalysisReport["base"]): string {
  // The resolved id, not the spec: a branch may have moved since the analysis ran.
  return info.commit ?? info.tree;
}

function loadSide(rev: string, path: string | null): Promise<Side> {
  if (path === null) return Promise.resolve({ kind: "absent" });
  return fetchFile(rev, path).then((blob) => ({ kind: "blob", blob }));
}

function linesOf(side: Side): string[] | null {
  if (side.kind !== "blob" || side.blob.content.kind !== "text") return null;
  return splitLines(side.blob.content.text);
}

function unreadable(side: Side): string | null {
  if (side.kind !== "blob") return null;
  const content = side.blob.content;
  if (content.kind === "binary") return "binary file";
  if (content.kind === "too_large") {
    return `${Math.round(content.size / 1024)} KiB, above the ${Math.round(side.blob.max_bytes / 1024)} KiB display limit`;
  }
  return null;
}

export function DiffView({ report, url, navigate }: Props) {
  const files = report.files;
  const file = files.find((f) => f.path === url.file) ?? files[0] ?? null;

  return (
    <div className="page page--diff">
      <nav className="changed" aria-label="Changed files">
        <h2 className="panel-title">
          Files <span className="count">{files.length}</span>
        </h2>
        <ul className="plain-list">
          {files.map((f) => (
            <li key={f.path}>
              <button
                type="button"
                className={`changed__item changed__item--file ${f.path === file?.path ? "is-selected" : ""}`}
                aria-current={f.path === file?.path ? "page" : undefined}
                title={f.path}
                onClick={() => {
                  navigate({ file: f.path, line: null }, { replace: true });
                }}
              >
                <span className={`badge badge--${f.status.toLowerCase()}`}>{f.status.toLowerCase()}</span>
                <span className="changed__name changed__name--path">{f.path}</span>
                <span className="count">{f.hunks.length}</span>
              </button>
            </li>
          ))}
        </ul>
      </nav>
      <section className="diff-main" aria-labelledby="diff-title">
        {file ? (
          <FileDiff key={file.path} report={report} file={file} line={url.line} navigate={navigate} />
        ) : (
          <>
            <h1 id="diff-title" className="view-title" tabIndex={-1}>
              Diff
            </h1>
            <Empty>No file changed between these revisions.</Empty>
          </>
        )}
      </section>
    </div>
  );
}

function FileDiff({
  report,
  file,
  line,
  navigate,
}: {
  report: AnalysisReport;
  file: FileChange;
  line: number | null;
  navigate: Navigate;
}) {
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const basePath = file.status === "ADDED" ? null : (file.old_path ?? file.path);
  const headPath = file.status === "DELETED" ? null : file.path;
  const baseRev = revOf(report.base);
  const headRev = revOf(report.head);

  useEffect(() => {
    let cancelled = false;
    Promise.all([loadSide(baseRev, basePath), loadSide(headRev, headPath)])
      .then(([base, head]) => {
        if (!cancelled) setLoaded({ status: "ready", base, head });
      })
      .catch((error: unknown) => {
        if (!cancelled) setLoaded({ status: "error", message: error instanceof Error ? error.message : String(error) });
      });
    return () => {
      cancelled = true;
    };
  }, [baseRev, headRev, basePath, headPath]);

  const rows = useMemo<DiffRow[] | null>(() => {
    if (loaded?.status !== "ready") return null;
    const base = linesOf(loaded.base);
    const head = linesOf(loaded.head);
    if ((basePath !== null && base === null) || (headPath !== null && head === null)) return null;
    return buildDiffRows(base, head, file.hunks);
  }, [loaded, basePath, headPath, file.hunks]);

  useEffect(() => {
    if (rows && line !== null) document.getElementById(`diff-L${line}`)?.scrollIntoView({ block: "center" });
  }, [rows, line]);

  const owners = report.owners.files.find((f) => f.path === file.path);
  const layers = layersNamedFor(report, file.path);
  const problem =
    loaded?.status === "ready" ? (unreadable(loaded.base) ?? unreadable(loaded.head)) : null;

  return (
    <>
      <header className="diff-header">
        <h1 id="diff-title" className="view-title view-title--path" tabIndex={-1}>
          {file.path}
        </h1>
        <div className="diff-header__meta">
          <span className={`badge badge--${file.status.toLowerCase()}`}>{file.status.toLowerCase()}</span>
          {file.old_path && (
            <span>
              from <code>{file.old_path}</code>
              {file.similarity !== null && ` (${file.similarity}% similar)`}
            </span>
          )}
          {file.language && <span className="muted">{file.language}</span>}
          {file.category && <span className="badge badge--category">{file.category.toLowerCase()}</span>}
          {layers.length > 0 && <span className="muted">layer {layers.join(", ")}</span>}
          {owners && owners.owners.length > 0 && <span className="muted">owners {owners.owners.join(", ")}</span>}
          <span className="muted">
            {file.hunks.length} hunks · <code>{baseRev.slice(0, 10)}</code> → <code>{headRev.slice(0, 10)}</code>
          </span>
        </div>
        {file.language === null && (
          <p className="note">
            Not a supported language: its effects are not traced through code
            {file.category ? `; recognised as ${file.category.toLowerCase()}` : ""}.
          </p>
        )}
      </header>

      {loaded === null && <p className="status">Loading file contents…</p>}
      {loaded?.status === "error" && (
        <p className="status status--error" role="alert">
          Could not load the file: {loaded.message}
        </p>
      )}
      {problem && (
        <p className="note">
          Contents not shown ({problem}). The hunks and their symbols below come from the report.
        </p>
      )}
      {file.hunks.length === 0 && loaded?.status === "ready" && (
        <Empty>No line hunks for this file (binary, oversized or not diffed).</Empty>
      )}

      {rows ? (
        <DiffTable rows={rows} report={report} file={file} line={line} navigate={navigate} />
      ) : (
        loaded?.status === "ready" &&
        file.hunks.map((hunk, index) => (
          <HunkAnnotation
            key={`${hunk.old_start}:${hunk.new_start}`}
            report={report}
            hunk={hunk}
            index={index}
            navigate={navigate}
          />
        ))
      )}
    </>
  );
}

function DiffTable({
  rows,
  report,
  file,
  line,
  navigate,
}: {
  rows: DiffRow[];
  report: AnalysisReport;
  file: FileChange;
  line: number | null;
  navigate: Navigate;
}) {
  return (
    <table className="diff">
      <caption className="sr-only">
        Unified diff of {file.path}: removed lines marked minus, added lines marked plus
      </caption>
      <colgroup>
        <col className="diff__num" />
        <col className="diff__num" />
        <col className="diff__mark" />
        <col />
      </colgroup>
      <tbody>
        {rows.map((row, i) => {
          switch (row.type) {
            case "gap":
              return (
                <tr key={`gap-${i}`} className="diff__gap">
                  <td colSpan={4}>⋯ {row.lines} unchanged lines</td>
                </tr>
              );
            case "hunk":
              return (
                <tr key={`hunk-${row.index}`} className="diff__hunk">
                  <td colSpan={4}>
                    <HunkAnnotation report={report} hunk={row.hunk} index={row.index} navigate={navigate} />
                  </td>
                </tr>
              );
            case "context":
              return (
                <tr key={`c-${row.new}-${row.old}`} id={`diff-L${row.new}`} className={row.new === line ? "is-target" : ""}>
                  <td className="diff__ln">{row.old}</td>
                  <td className="diff__ln">{row.new}</td>
                  <td className="diff__marker" aria-hidden="true" />
                  <td className="diff__code">
                    <code>{row.text}</code>
                  </td>
                </tr>
              );
            case "del":
              return (
                <tr key={`d-${row.old}`} className="diff__del">
                  <td className="diff__ln">{row.old}</td>
                  <td className="diff__ln" />
                  <td className="diff__marker">
                    <span aria-label="removed">−</span>
                  </td>
                  <td className="diff__code">
                    <code>{row.text}</code>
                  </td>
                </tr>
              );
            case "add":
              return (
                <tr
                  key={`a-${row.new}`}
                  id={`diff-L${row.new}`}
                  className={`diff__add ${row.new === line ? "is-target" : ""}`}
                >
                  <td className="diff__ln" />
                  <td className="diff__ln">{row.new}</td>
                  <td className="diff__marker">
                    <span aria-label="added">+</span>
                  </td>
                  <td className="diff__code">
                    <code>{row.text}</code>
                  </td>
                </tr>
              );
          }
        })}
      </tbody>
    </table>
  );
}

function HunkAnnotation({
  report,
  hunk,
  index,
  navigate,
}: {
  report: AnalysisReport;
  hunk: HunkReport;
  index: number;
  navigate: Navigate;
}) {
  const lookup = indexReport(report);
  const ids = [...new Set([...hunk.head_symbols, ...hunk.base_symbols])];
  const range = `@@ −${hunk.old_start},${hunk.old_len} +${hunk.new_start},${hunk.new_len} @@`;
  return (
    <div className="hunk" aria-label={`Hunk ${index + 1}`}>
      <code className="hunk__range">{range}</code>
      {ids.length === 0 ? (
        <span className="muted">no containing symbol (not parsed source)</span>
      ) : (
        <ul className="hunk__symbols">
          {ids.map((id) => {
            const changed = lookup.changed.get(id);
            const onlyBase = !hunk.head_symbols.includes(id);
            const dependents = dependentsInSlice(report, id).length;
            const tests = testsReaching(report, id).filter((t) => t.id !== id);
            const strongest = tests.find((t) => t.tier === "STRONG") ?? tests.find((t) => t.tier === "MEDIUM") ?? tests[0];
            const signals = signalsNaming(report, [id]);
            const fileLevel = id.startsWith("file:");
            return (
              <li key={id} className="hunk__symbol">
                {changed ? (
                  <span className={`badge badge--${changed.change.toLowerCase()}`}>{CHANGE_LABEL[changed.change]}</span>
                ) : (
                  <span className="badge badge--neutral">context</span>
                )}
                <button
                  type="button"
                  className="link sym"
                  title={`${id} — show in the blast radius`}
                  onClick={() => {
                    navigate({ view: "overview", sel: id });
                  }}
                >
                  {fileLevel ? "file scope" : shortLabel(id)}
                </button>
                {onlyBase && <span className="muted small">(base)</span>}
                {!fileLevel && (
                  <span className="hunk__fact" title="Distinct dependents in the visualised graph">
                    {dependents} dependents
                  </span>
                )}
                {!fileLevel && (
                  <span className="hunk__fact" title="Recommended tests reaching this symbol">
                    {tests.length} tests{strongest && <> · best <TierBadge tier={strongest.tier} /></>}
                  </span>
                )}
                <CoverageBadge status={changed?.coverage ?? null} />
                {signals.map((s) => (
                  <span key={s} className="chip-static" title="Risk signal whose evidence names this symbol">
                    {humanize(s)}
                  </span>
                ))}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
