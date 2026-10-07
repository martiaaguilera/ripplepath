import type { AnalysisReport, ChangeKind, Severity } from "../api/types";
import { EVIDENCE_LABEL, shortLabel } from "../graph/model";
import { EvidencePath } from "./EvidencePath";

const CHANGE_LABEL: Record<ChangeKind, string> = {
  ADDED: "added",
  DELETED: "deleted",
  MODIFIED: "modified",
  SIGNATURE_CHANGED: "signature",
};

export function SummaryBar({ report }: { report: AnalysisReport }) {
  const s = report.summary;
  const bySeverity = (severity: Severity) => report.uncertainty.filter((u) => u.severity === severity).length;
  const high = bySeverity("high");
  const items: { label: string; value: string; detail?: string; tone?: string }[] = [
    { label: "Changed symbols", value: String(s.symbols_changed), detail: `${s.files_changed} files` },
    {
      label: "Blast radius",
      value: String(s.symbols_impacted),
      detail: `${s.modules_impacted} modules · depth ≤ ${s.max_depth}${s.impact_truncated ? " · truncated" : ""}`,
    },
    {
      label: "Recommended tests",
      value: `${s.tests_recommended} / ${s.tests_total}`,
      detail: "static evidence",
    },
    {
      label: "Uncertainty",
      value: String(s.uncertainty_items),
      detail: `${high} high · ${bySeverity("medium")} medium · ${bySeverity("low")} low`,
      ...(high > 0 ? { tone: "warn" } : {}),
    },
  ];
  return (
    <section className="summary" aria-label="Summary">
      {items.map((item) => (
        <div key={item.label} className={`summary__item ${item.tone ? `summary__item--${item.tone}` : ""}`}>
          <span className="summary__label">{item.label}</span>
          <span className="summary__value">{item.value}</span>
          {item.detail && <span className="summary__detail">{item.detail}</span>}
        </div>
      ))}
    </section>
  );
}

interface ListProps {
  report: AnalysisReport;
  selectedId: string | null;
  onSelectSymbol: (id: string) => void;
}

export function ChangedList({ report, selectedId, onSelectSymbol }: ListProps) {
  const byFile = new Map<string, AnalysisReport["changed_symbols"]>();
  for (const symbol of report.changed_symbols) {
    if (symbol.kind === "file") continue;
    const list = byFile.get(symbol.file) ?? [];
    list.push(symbol);
    byFile.set(symbol.file, list);
  }
  const unanalysed = report.files.filter((f) => f.language === null);
  return (
    <nav className="changed" aria-label="Changed symbols">
      <h2 className="panel-title">Changed</h2>
      {[...byFile.entries()].map(([file, symbols]) => (
        <section key={file} className="changed__file">
          <h3 className="changed__path" title={file}>
            {file.split("/").pop()}
          </h3>
          <ul className="plain-list">
            {symbols.map((symbol) => (
              <li key={symbol.id}>
                <button
                  type="button"
                  className={`changed__item ${selectedId === symbol.id ? "is-selected" : ""}`}
                  onClick={() => onSelectSymbol(symbol.id)}
                  aria-pressed={selectedId === symbol.id}
                  title={symbol.id}
                >
                  <span className={`badge badge--${symbol.change.toLowerCase()}`}>{CHANGE_LABEL[symbol.change]}</span>
                  <span className="changed__name">{shortLabel(symbol.id)}</span>
                  {symbol.is_test && <span className="badge badge--test">test</span>}
                </button>
              </li>
            ))}
          </ul>
        </section>
      ))}
      {unanalysed.length > 0 && (
        <section className="changed__file">
          <h3 className="changed__path">Not analysed</h3>
          <ul className="plain-list">
            {unanalysed.map((file) => (
              <li key={file.path} className="changed__unanalysed" title={file.path}>
                <span className={`badge badge--${file.status.toLowerCase()}`}>{file.status.toLowerCase()}</span>
                <span className="changed__name">{file.path.split("/").pop()}</span>
              </li>
            ))}
          </ul>
        </section>
      )}
    </nav>
  );
}

export function TestsPanel({ report, selectedId, onSelectSymbol }: ListProps) {
  if (report.tests.length === 0) {
    return (
      <p className="muted">
        No test has a static dependency path to this change. That is a gap in evidence, not a sign the change is safe.
      </p>
    );
  }
  return (
    <>
      <p className="note">
        Recommended first tests, ordered by evidence. Static paths show that a test <em>can</em> reach the change; they
        do not prove it does, and passing them does not make the change safe to merge.
      </p>
      <ol className="tests">
        {report.tests.map((test, index) => (
          <li key={test.id} className={`tests__item ${selectedId === test.id ? "is-selected" : ""}`}>
            <div className="tests__header">
              <span className="tests__rank">{index + 1}</span>
              <button type="button" className="link tests__name" onClick={() => onSelectSymbol(test.id)} title={test.id}>
                {shortLabel(test.id)}
              </button>
              {test.reason === "CHANGED_TEST" ? (
                <span className="badge badge--modified">test changed</span>
              ) : (
                <>
                  <span className="badge badge--neutral">depth {test.depth}</span>
                  <span className={`evidence evidence--${test.weakest_evidence.toLowerCase()}`}>
                    {EVIDENCE_LABEL[test.weakest_evidence].toLowerCase()}
                  </span>
                </>
              )}
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
  return (
    <table className="table">
      <caption className="sr-only">Impacted symbols with depth and weakest evidence on their explaining path</caption>
      <thead>
        <tr>
          <th scope="col">Symbol</th>
          <th scope="col">Depth</th>
          <th scope="col">Weakest evidence</th>
          <th scope="col">Via</th>
          <th scope="col">Location</th>
        </tr>
      </thead>
      <tbody>
        {report.impacted_symbols.map((symbol) => (
          <tr key={symbol.id} className={selectedId === symbol.id ? "is-selected" : ""}>
            <td>
              <button type="button" className="link" onClick={() => onSelectSymbol(symbol.id)} title={symbol.id}>
                {shortLabel(symbol.id)}
              </button>
              {symbol.is_test && <span className="badge badge--test">test</span>}
            </td>
            <td className="num">{symbol.depth}</td>
            <td>
              <span className={`evidence evidence--${symbol.weakest_evidence.toLowerCase()}`}>
                {EVIDENCE_LABEL[symbol.weakest_evidence].toLowerCase()}
              </span>
            </td>
            <td>{symbol.path[symbol.path.length - 1]?.via_dispatch ? "dispatch" : symbol.path[symbol.path.length - 1]?.edge.kind.toLowerCase()}</td>
            <td>
              <code>
                {symbol.file}:{symbol.span.start_line}
              </code>
            </td>
          </tr>
        ))}
      </tbody>
    </table>
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
          <span className={`severity severity--${item.severity}`}>{item.severity}</span>
          <span className="uncertainty__kind">{item.kind.replaceAll("_", " ").toLowerCase()}</span>
          {item.file && (
            <code>
              {item.file}
              {item.line !== null ? `:${item.line}` : ""}
            </code>
          )}
          <span>{item.detail}</span>
        </li>
      ))}
    </ul>
  );
}
