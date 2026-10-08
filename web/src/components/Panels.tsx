import type { AnalysisReport, ChangeKind, TestRecommendation } from "../api/types";
import { indexReport } from "../app/reportIndex";
import { shortLabel } from "../graph/model";
import { CoverageBadge, EvidenceBadge, Location, SeverityBadge, TierBadge, humanize } from "./common";
import { EvidencePath } from "./EvidencePath";

export const CHANGE_LABEL: Record<ChangeKind, string> = {
  ADDED: "added",
  DELETED: "deleted",
  MODIFIED: "modified",
  SIGNATURE_CHANGED: "signature",
};

interface ListProps {
  report: AnalysisReport;
  selectedId: string | null;
  onSelectSymbol: (id: string) => void;
}

function fileName(path: string): string {
  return path.split("/").pop() ?? path;
}

export function ChangedList({
  report,
  selectedId,
  onSelectSymbol,
  onOpenFile,
}: ListProps & { onOpenFile: (path: string) => void }) {
  const byFile = new Map<string, AnalysisReport["changed_symbols"]>();
  for (const symbol of report.changed_symbols) {
    if (symbol.kind === "file") continue;
    const list = byFile.get(symbol.file) ?? [];
    list.push(symbol);
    byFile.set(symbol.file, list);
  }
  const files = indexReport(report).files;
  const unanalysed = report.files.filter((f) => !byFile.has(f.path));
  return (
    <nav className="changed" aria-label="Changed files and symbols">
      <h2 className="panel-title">
        Changed <span className="count">{report.summary.files_changed} files</span>
      </h2>
      {[...byFile.entries()].map(([path, symbols]) => {
        const file = files.get(path);
        return (
          <section key={path} className="changed__file" aria-label={path}>
            <h3 className="changed__path">
              <button
                type="button"
                className="link changed__open"
                title={`${path} — open the diff`}
                onClick={() => {
                  onOpenFile(path);
                }}
              >
                {fileName(path)}
              </button>
              {file?.category && <span className="badge badge--category">{file.category.toLowerCase()}</span>}
            </h3>
            <ul className="plain-list">
              {symbols.map((symbol) => (
                <li key={symbol.id}>
                  <button
                    type="button"
                    className={`changed__item ${selectedId === symbol.id ? "is-selected" : ""}`}
                    onClick={() => {
                      onSelectSymbol(symbol.id);
                    }}
                    aria-pressed={selectedId === symbol.id}
                    title={symbol.id}
                  >
                    <span className={`badge badge--${symbol.change.toLowerCase()}`}>{CHANGE_LABEL[symbol.change]}</span>
                    <span className="changed__name">{shortLabel(symbol.id)}</span>
                    {symbol.is_test && <span className="badge badge--test">test</span>}
                    <CoverageBadge status={symbol.coverage} />
                  </button>
                </li>
              ))}
            </ul>
          </section>
        );
      })}
      {unanalysed.length > 0 && (
        <section className="changed__file" aria-label="Files without changed symbols">
          <h3 className="changed__path changed__path--muted">No symbols traced</h3>
          <ul className="plain-list">
            {unanalysed.map((file) => (
              <li key={file.path}>
                <button
                  type="button"
                  className="changed__item changed__item--file"
                  title={`${file.path} — open the diff`}
                  onClick={() => {
                    onOpenFile(file.path);
                  }}
                >
                  <span className={`badge badge--${file.status.toLowerCase()}`}>{file.status.toLowerCase()}</span>
                  <span className="changed__name">{fileName(file.path)}</span>
                  {file.category && <span className="badge badge--category">{file.category.toLowerCase()}</span>}
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
    </nav>
  );
}

/** Tests in the selection's run order; anything not in `ordered` follows in report order. */
export function orderedTests(report: AnalysisReport): TestRecommendation[] {
  const byId = indexReport(report).tests;
  const ordered = report.test_selection.ordered.flatMap((id) => {
    const test = byId.get(id);
    return test ? [test] : [];
  });
  const seen = new Set(ordered.map((t) => t.id));
  return [...ordered, ...report.tests.filter((t) => !seen.has(t.id))];
}

export function evidenceKind(test: TestRecommendation): string {
  if (test.reason === "CHANGED_TEST") return "changed test";
  return test.coverage_observed ? "measured coverage" : "static path";
}

export function TestsPanel({ report, selectedId, onSelectSymbol }: ListProps) {
  if (report.tests.length === 0) {
    return (
      <p className="muted">
        No test has a dependency path to this change. That is a gap in evidence, not a sign the change is safe.
      </p>
    );
  }
  return (
    <>
      <p className="note">
        Run order from the test selection. A static path shows a test <em>can</em> reach the change; measured coverage
        shows it executed it. Neither makes the change safe to merge.
      </p>
      <ol className="tests">
        {orderedTests(report).map((test, index) => (
          <li key={test.id} className={`tests__item ${selectedId === test.id ? "is-selected" : ""}`}>
            <div className="tests__header">
              <span className="tests__rank">{index + 1}</span>
              <button type="button" className="link tests__name" onClick={() => onSelectSymbol(test.id)} title={test.id}>
                {shortLabel(test.id)}
              </button>
              <TierBadge tier={test.tier} />
              <span className={`evkind evkind--${evidenceKind(test).replace(" ", "-")}`}>{evidenceKind(test)}</span>
              {test.reason === "STATIC_PATH" && <EvidenceBadge evidence={test.weakest_evidence} prefix="weakest: " />}
            </div>
            {test.reason === "STATIC_PATH" && (
              <EvidencePath root={test.root} hops={test.path} onSelectSymbol={onSelectSymbol} />
            )}
          </li>
        ))}
      </ol>
    </>
  );
}

export function ImpactTable({ report, selectedId, onSelectSymbol }: ListProps) {
  if (report.impacted_symbols.length === 0) {
    return <p className="muted">No symbol depends on the changed code through a propagating edge.</p>;
  }
  return (
    <div className="table-wrap">
      <table className="table">
        <caption className="sr-only">Impacted symbols with depth and weakest evidence on their explaining path</caption>
        <thead>
          <tr>
            <th scope="col">Symbol</th>
            <th scope="col" className="num">
              Depth
            </th>
            <th scope="col">Weakest evidence</th>
            <th scope="col">Via</th>
            <th scope="col">Coverage</th>
            <th scope="col">Location</th>
          </tr>
        </thead>
        <tbody>
          {report.impacted_symbols.map((symbol) => {
            const last = symbol.path[symbol.path.length - 1];
            return (
              <tr key={symbol.id} className={selectedId === symbol.id ? "is-selected" : ""}>
                <td>
                  <button type="button" className="link sym" onClick={() => onSelectSymbol(symbol.id)} title={symbol.id}>
                    {shortLabel(symbol.id)}
                  </button>
                  {symbol.is_test && <span className="badge badge--test">test</span>}
                  {symbol.graph === "base" && (
                    <span className="badge badge--neutral" title="Explained through the base revision's graph">
                      via base
                    </span>
                  )}
                </td>
                <td className="num">{symbol.depth}</td>
                <td>
                  <EvidenceBadge evidence={symbol.weakest_evidence} />
                </td>
                <td>{last ? (last.via_dispatch ? "dispatch" : last.edge.kind.toLowerCase()) : "—"}</td>
                <td>
                  <CoverageBadge status={symbol.coverage} />
                </td>
                <td>
                  <Location file={symbol.file} line={symbol.span.start_line} />
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

export function UncertaintyPanel({ report }: { report: AnalysisReport }) {
  if (report.uncertainty.length === 0) {
    return <p className="muted">No uncertainty recorded for this change.</p>;
  }
  return (
    <ul className="uncertainty">
      {report.uncertainty.map((item) => (
        <li key={`${item.kind}|${item.file ?? ""}|${item.line ?? 0}|${item.detail}`} className="uncertainty__item">
          <SeverityBadge severity={item.severity} />
          <span className="uncertainty__kind">{humanize(item.kind)}</span>
          {item.file && <Location file={item.file} line={item.line} />}
          <span>{item.detail}</span>
        </li>
      ))}
    </ul>
  );
}
