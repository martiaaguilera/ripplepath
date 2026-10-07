import type { AnalysisReport } from "../api/types";
import type { Selection } from "../graph/ImpactGraph";
import { EVIDENCE_LABEL, edgeExplanation, explainingPath, shortLabel } from "../graph/model";
import { EvidencePath } from "./EvidencePath";

interface Props {
  report: AnalysisReport;
  selection: Selection;
  onSelectSymbol: (id: string) => void;
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <>
      <dt>{label}</dt>
      <dd>{children}</dd>
    </>
  );
}

export function Inspector({ report, selection, onSelectSymbol }: Props) {
  if (!selection) {
    return (
      <aside className="inspector" aria-label="Inspector">
        <h2 className="panel-title">Inspector</h2>
        <p className="muted">
          Select a symbol or an edge in the graph or the lists to see what connects it to the change, and the source
          evidence for every step.
        </p>
      </aside>
    );
  }

  if (selection.type === "edge") {
    const { edge } = selection;
    return (
      <aside className="inspector" aria-label="Inspector">
        <h2 className="panel-title">Edge</h2>
        <p className="inspector__headline">
          <button type="button" className="link" onClick={() => onSelectSymbol(edge.from)}>
            {shortLabel(edge.from)}
          </button>{" "}
          <span className={`path__kind path__kind--${edge.kind.toLowerCase()}`}>{edge.kind.toLowerCase()}</span>{" "}
          <button type="button" className="link" onClick={() => onSelectSymbol(edge.to)}>
            {shortLabel(edge.to)}
          </button>
        </p>
        <dl className="facts">
          <Field label="Evidence">
            <span className={`evidence evidence--${edge.evidence.toLowerCase()}`}>{EVIDENCE_LABEL[edge.evidence]}</span>
          </Field>
          <Field label="Source">
            <code>
              {edge.file}:{edge.line}
            </code>
          </Field>
          <Field label="Rule">
            <code>{edge.rule}</code>
          </Field>
        </dl>
        <h3 className="inspector__subtitle">Why it matters</h3>
        <p>{edgeExplanation(edge)}</p>
        {edge.evidence !== "RESOLVED_EXACT" && (
          <p className="note">
            Not resolved to a single declaration (e.g. an overload with the same arity or an inferred receiver). Treat it
            as plausible, not certain.
          </p>
        )}
      </aside>
    );
  }

  const id = selection.id;
  const node = report.graph.nodes.find((n) => n.id === id);
  const changed = report.changed_symbols.find((s) => s.id === id);
  const impacted = report.impacted_symbols.find((s) => s.id === id);
  const path = explainingPath(report, id);
  const dependents = report.graph.edges.filter((e) => e.to === id && e.kind !== "CONTAINS").length;
  const testsThrough = report.tests.filter((t) => t.path.some((hop) => hop.symbol === id) || t.root === id);
  const unresolved = report.uncertainty.filter((u) => u.symbol === id);
  const file = changed?.file ?? impacted?.file ?? node?.file;
  const line = changed?.span.start_line ?? impacted?.span.start_line ?? node?.line;

  return (
    <aside className="inspector" aria-label="Inspector">
      <h2 className="panel-title">Symbol</h2>
      <p className="inspector__headline" title={id}>
        {shortLabel(id)}
      </p>
      <dl className="facts">
        <Field label="Kind">{changed?.kind ?? impacted?.kind ?? node?.kind ?? "unknown"}</Field>
        <Field label="Status">
          {changed
            ? `changed — ${changed.change.replace("_", " ").toLowerCase()}`
            : impacted
              ? `impacted at depth ${impacted.depth}${impacted.graph === "base" ? " (via base revision)" : ""}`
              : "not impacted"}
        </Field>
        {changed?.previous_id && (
          <Field label="Previously">
            <code>{shortLabel(changed.previous_id)}</code>
          </Field>
        )}
        {changed?.probable_move && (
          <Field label="Probable move">
            <code>{shortLabel(changed.probable_move)}</code>
          </Field>
        )}
        <Field label="Module">{changed?.module ?? impacted?.module ?? node?.module}</Field>
        {changed && <Field label="Visibility">{changed.visibility}</Field>}
        <Field label="Source">
          <code>
            {file}:{line}
          </code>
        </Field>
        <Field label="Dependents shown">{dependents}</Field>
        <Field label="Id">
          <code className="break">{id}</code>
        </Field>
      </dl>

      {path && (
        <>
          <h3 className="inspector__subtitle">
            Why it is impacted{" "}
            {impacted && (
              <span className={`evidence evidence--${impacted.weakest_evidence.toLowerCase()}`}>
                weakest: {EVIDENCE_LABEL[impacted.weakest_evidence].toLowerCase()}
              </span>
            )}
          </h3>
          <EvidencePath root={path.root} hops={path.hops} onSelectSymbol={onSelectSymbol} />
        </>
      )}

      <h3 className="inspector__subtitle">Test evidence</h3>
      {testsThrough.length === 0 ? (
        <p className="muted">No recommended test reaches this symbol through a static path.</p>
      ) : (
        <ul className="plain-list">
          {testsThrough.map((test) => (
            <li key={test.id}>
              <button type="button" className="link" onClick={() => onSelectSymbol(test.id)}>
                {shortLabel(test.id)}
              </button>{" "}
              <span className="muted">static path, depth {test.depth}</span>
            </li>
          ))}
        </ul>
      )}

      {unresolved.length > 0 && (
        <>
          <h3 className="inspector__subtitle">Uncertainty</h3>
          <ul className="plain-list">
            {unresolved.map((u) => (
              <li key={`${u.file ?? ""}:${u.line ?? 0}:${u.detail}`}>
                <code>
                  {u.file}:{u.line}
                </code>{" "}
                {u.detail}
              </li>
            ))}
          </ul>
        </>
      )}
    </aside>
  );
}
