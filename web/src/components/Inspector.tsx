import type { ReactNode } from "react";
import type { AnalysisReport } from "../api/types";
import { dependentsInSlice, indexReport, signalsNaming, testsReaching } from "../app/reportIndex";
import type { Selection } from "../graph/ImpactGraph";
import { edgeExplanation, explainingPath, shortLabel } from "../graph/model";
import { CoverageBadge, EvidenceBadge, Location, TierBadge, humanize } from "./common";
import { EvidencePath } from "./EvidencePath";
import { evidenceKind } from "./Panels";

interface Props {
  report: AnalysisReport;
  selection: Selection;
  onSelectSymbol: (id: string) => void;
  onOpenFile: (path: string, line: number | null) => void;
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <dt>{label}</dt>
      <dd>{children}</dd>
    </>
  );
}

export function Inspector({ report, selection, onSelectSymbol, onOpenFile }: Props) {
  if (!selection) {
    return (
      <aside className="inspector" aria-label="Inspector">
        <h2 className="panel-title">Inspector</h2>
        <p className="muted">
          Select a symbol or an edge in the graph or the lists to see what connects it to the change, and the source
          evidence for every step.
        </p>
        <h3 className="inspector__subtitle">Keyboard</h3>
        <dl className="facts facts--keys">
          <Field label="1 – 6">switch view</Field>
          <Field label="/">find symbol in graph</Field>
          <Field label="Esc">clear selection</Field>
          <Field label="↑ ↓ Enter">move and inspect in the Nodes tab</Field>
        </dl>
      </aside>
    );
  }

  const index = indexReport(report);
  const changedPaths = index.filePaths;

  if (selection.type === "edge") {
    const { edge } = selection;
    const fromDeleted = index.changed.get(edge.from)?.change === "DELETED";
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
            <EvidenceBadge evidence={edge.evidence} />
          </Field>
          <Field label="Source">
            {changedPaths.has(edge.file) ? (
              <button
                type="button"
                className="link loc"
                title="Open the diff"
                onClick={() => {
                  // Diff anchors are head line numbers; a base-side edge's line would mislead.
                  onOpenFile(edge.file, fromDeleted ? null : edge.line);
                }}
              >
                {edge.file}:{edge.line}
              </button>
            ) : (
              <Location file={edge.file} line={edge.line} />
            )}
          </Field>
          <Field label="Rule">
            <code>{edge.rule}</code>
          </Field>
        </dl>
        <h3 className="inspector__subtitle">Why it matters</h3>
        <p>{edgeExplanation(edge)}</p>
        {edge.evidence !== "RESOLVED_EXACT" && edge.evidence !== "COVERAGE_OBSERVED" && (
          <p className="note">
            Not resolved to a single declaration (e.g. an overload with the same arity or an inferred receiver). Treat it
            as plausible, not certain.
          </p>
        )}
      </aside>
    );
  }

  const id = selection.id;
  const node = index.nodes.get(id);
  const changed = index.changed.get(id);
  const impacted = index.impacted.get(id);
  const test = index.tests.get(id);
  const path = explainingPath(report, id);
  const dependents = dependentsInSlice(report, id);
  const testsThrough = testsReaching(report, id).filter((t) => t.id !== id);
  const unresolved = report.uncertainty.filter((u) => u.symbol === id);
  const file = changed?.file ?? impacted?.file ?? test?.file ?? node?.file;
  const line = changed?.span.start_line ?? impacted?.span.start_line ?? node?.line;
  const owners = file ? report.owners.files.find((f) => f.path === file) : undefined;
  const signals = signalsNaming(report, [id, file ?? ""]);
  const coverage = changed?.coverage ?? impacted?.coverage ?? null;
  // Diff anchors are head line numbers: deleted symbols and base-graph dependents have none.
  const headSide = changed?.change !== "DELETED" && impacted?.graph !== "base";

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
              : test
                ? "recommended test"
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
        <Field label="Module">{changed?.module ?? impacted?.module ?? node?.module ?? "—"}</Field>
        {changed && <Field label="Visibility">{changed.visibility}</Field>}
        {coverage && (
          <Field label="Coverage">
            <CoverageBadge status={coverage} />
          </Field>
        )}
        {file && (
          <Field label="Source">
            {changedPaths.has(file) ? (
              <button
                type="button"
                className="link loc"
                title="Open the diff"
                onClick={() => {
                  onOpenFile(file, headSide ? (line ?? null) : null);
                }}
              >
                {file}:{line}
              </button>
            ) : (
              <Location file={file} line={line ?? null} />
            )}
          </Field>
        )}
        <Field label="Dependents shown">{dependents.length}</Field>
        {owners && owners.owners.length > 0 && <Field label="Owners">{owners.owners.join(", ")}</Field>}
        {signals.length > 0 && (
          <Field label="Risk signals">
            {signals.map((s) => (
              <span key={s} className="chip-static">
                {humanize(s)}
              </span>
            ))}
          </Field>
        )}
        <Field label="Id">
          <code className="break">{id}</code>
        </Field>
      </dl>

      {path && (
        <>
          <h3 className="inspector__subtitle">
            Why it is {test && !impacted ? "recommended" : "impacted"}{" "}
            {impacted && <EvidenceBadge evidence={impacted.weakest_evidence} prefix="weakest: " />}
          </h3>
          <EvidencePath root={path.root} hops={path.hops} onSelectSymbol={onSelectSymbol} />
        </>
      )}

      <h3 className="inspector__subtitle">Test evidence</h3>
      {test && (
        <p>
          Recommended test <TierBadge tier={test.tier} /> <span className="muted">{evidenceKind(test)}</span>
        </p>
      )}
      {testsThrough.length === 0 ? (
        test ? null : (
          <p className="muted">No recommended test reaches this symbol, statically or by measured coverage.</p>
        )
      ) : (
        <ul className="plain-list inspector__tests">
          {testsThrough.map((t) => (
            <li key={t.id}>
              <button type="button" className="link sym" onClick={() => onSelectSymbol(t.id)} title={t.id}>
                {shortLabel(t.id)}
              </button>{" "}
              <TierBadge tier={t.tier} /> <span className="muted">{evidenceKind(t)}</span>
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
                {u.file && <Location file={u.file} line={u.line} />} {u.detail}
              </li>
            ))}
          </ul>
        </>
      )}
    </aside>
  );
}
