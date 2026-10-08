// Architecture drift, base → head: the layered picture, the exact symbol edge behind every
// violation, cycles and module coupling. Statuses (new / pre-existing / removed) are the report's.

import { useState, type KeyboardEvent } from "react";
import type { AnalysisReport, DeltaStatus, Violation } from "../api/types";
import { indexReport } from "../app/reportIndex";
import type { Navigate } from "../app/url";
import { DeltaBadge, Empty, EvidenceBadge, Location, SymbolLink } from "../components/common";
import { BOX_HEIGHT, BOX_WIDTH, buildDiagram, type DiagramMode, type LayerArc } from "./architectureDiagram";

interface Props {
  report: AnalysisReport;
  navigate: Navigate;
}

const MODES: [DiagramMode, string][] = [
  ["head", "Head"],
  ["base", "Base"],
  ["delta", "Base → head"],
];

const STATUSES: DeltaStatus[] = ["NEW", "PRE_EXISTING", "REMOVED"];

function violationKey(v: Violation): string {
  return `${v.status}|${v.edge.from}|${v.edge.to}|${v.edge.kind}|${v.edge.line}`;
}

function countLabel(arc: Pick<LayerArc, "base" | "head">, mode: DiagramMode): string {
  if (mode === "head") return String(arc.head);
  if (mode === "base") return String(arc.base);
  return arc.base === arc.head ? String(arc.head) : `${arc.base}→${arc.head}`;
}

export function ArchitectureView({ report, navigate }: Props) {
  const arch = report.architecture;
  const [mode, setMode] = useState<DiagramMode>("head");
  const [pair, setPair] = useState<string | null>(null);
  const [statuses, setStatuses] = useState<ReadonlySet<DeltaStatus>>(() => new Set(STATUSES));
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const index = indexReport(report);

  if (!arch.configured) {
    return (
      <div className="page">
        <header className="page__header">
          <h1 className="view-title" tabIndex={-1}>
            Architecture
          </h1>
        </header>
        <Empty>
          No layers are configured. Add an <code>architecture.layers</code> section to <code>ripplepath.yml</code> on
          the base branch to check layer rules and see drift here (docs/ARCHITECTURE_RULES.md).
        </Empty>
        <ModuleCoupling report={report} />
      </div>
    );
  }

  const diagram = buildDiagram(arch, mode);
  const visibleViolations = arch.violations.filter(
    (v) => statuses.has(v.status) && (pair === null || `${v.from_layer}\u0000${v.to_layer}` === pair),
  );
  const selected =
    arch.violations.find((v) => violationKey(v) === selectedKey) ?? visibleViolations[0] ?? null;
  const cycleLayers = new Set(arch.cycles.filter((c) => c.status !== "REMOVED").flatMap((c) => c.layers));
  const openFile = (file: string, line: number | null) => {
    navigate({ view: "diff", file, line });
  };
  const showInGraph = (id: string) => {
    navigate({ view: "overview", sel: id });
  };
  const choosePair = (key: string) => {
    setPair((current) => (current === key ? null : key));
  };

  return (
    <div className="page page--split">
      <div className="page__main">
        <header className="page__header">
          <h1 className="view-title" tabIndex={-1}>
            Architecture
          </h1>
          <p className="page__lede">
            Layer rules from <code>{report.config.path}</code> at <code>{report.config.revision}</code>, checked on
            base and head. Cycles: <code>{arch.cycles_mode}</code>.
          </p>
        </header>

        <dl className="facts facts--inline arch-summary">
          <dt>New violations</dt>
          <dd className={arch.summary.new_violations > 0 ? "tone-fail" : ""}>
            {arch.summary.new_violations}
            {arch.summary.new_violations > 0 && (
              <span className="muted"> ({arch.summary.new_violations_exact} on exact edges)</span>
            )}
          </dd>
          <dt>Pre-existing</dt>
          <dd>{arch.summary.pre_existing_violations}</dd>
          <dt>Removed</dt>
          <dd>{arch.summary.removed_violations}</dd>
          <dt>New cycles</dt>
          <dd className={arch.summary.new_cycles > 0 ? "tone-fail" : ""}>{arch.summary.new_cycles}</dd>
          <dt>Pre-existing cycles</dt>
          <dd>{arch.summary.pre_existing_cycles}</dd>
        </dl>

        <section className="arch-diagram" aria-labelledby="layers-title">
          <div className="section-head">
            <h2 id="layers-title" className="section-title">
              Layers
            </h2>
            <div className="segmented" role="radiogroup" aria-label="Revision shown in the diagram">
              {MODES.map(([value, label]) => (
                <label key={value} className={`segmented__item ${mode === value ? "is-active" : ""}`}>
                  <input
                    type="radio"
                    name="arch-mode"
                    value={value}
                    checked={mode === value}
                    onChange={() => {
                      setMode(value);
                    }}
                  />
                  {label}
                </label>
              ))}
            </div>
          </div>
          <svg
            className="layers"
            viewBox={`0 0 ${diagram.width} ${diagram.height}`}
            width={diagram.width}
            height={diagram.height}
            role="group"
            aria-label="Layer diagram; the Layer dependencies table lists the same edges"
          >
            <defs>
              {["plain", "new", "pre_existing", "removed", "active"].map((tone) => (
                <marker
                  key={tone}
                  id={`arrow-${tone}`}
                  viewBox="0 0 10 10"
                  refX="9"
                  refY="5"
                  markerWidth="7"
                  markerHeight="7"
                  orient="auto-start-reverse"
                >
                  <path d="M0,0 L10,5 L0,10 z" className={`arrowhead arrowhead--${tone}`} />
                </marker>
              ))}
            </defs>
            {diagram.boxes.map((box) => (
              <g key={box.name} className={`layer ${cycleLayers.has(box.name) ? "layer--cycle" : ""}`}>
                <rect x={box.x} y={box.y} width={BOX_WIDTH} height={BOX_HEIGHT} rx={6} className="layer__box" />
                <text x={box.x + 16} y={box.y + 25} className="layer__name">
                  {box.name}
                </text>
                <text x={box.x + 16} y={box.y + 44} className="layer__meta">
                  {mode === "head"
                    ? `${box.head} symbols`
                    : mode === "base"
                      ? `${box.base} symbols`
                      : `${box.base} → ${box.head} symbols`}
                  {cycleLayers.has(box.name) ? " · in a cycle" : ""}
                </text>
              </g>
            ))}
            {diagram.arcs.map((arc) => {
              const removedInMode = mode !== "base" && arc.head === 0;
              const tone = arc.status ? arc.status.toLowerCase() : removedInMode ? "removed" : "plain";
              const active = pair === arc.key;
              const label = `${arc.from} → ${arc.to}: ${countLabel(arc, mode)} edges${
                arc.status ? `, ${arc.violations} ${arc.status.toLowerCase().replace("_", "-")} violations` : ""
              }`;
              return (
                <g
                  key={arc.key}
                  className={`arc arc--${tone} ${active ? "is-active" : ""} ${removedInMode ? "arc--gone" : ""}`}
                  role="button"
                  tabIndex={0}
                  aria-pressed={active}
                  aria-label={label}
                  onClick={() => {
                    choosePair(arc.key);
                  }}
                  onKeyDown={(event: KeyboardEvent<SVGGElement>) => {
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      choosePair(arc.key);
                    }
                  }}
                >
                  <title>{label}</title>
                  <polyline
                    className="arc__hit"
                    points={arc.points.map(([x, y]) => `${x},${y}`).join(" ")}
                  />
                  <polyline
                    className="arc__line"
                    points={arc.points.map(([x, y]) => `${x},${y}`).join(" ")}
                    markerEnd={`url(#arrow-${active ? "active" : tone})`}
                  />
                  <rect
                    x={arc.labelX - 18}
                    y={arc.labelY - 9}
                    width={36}
                    height={18}
                    rx={9}
                    className="arc__pill"
                  />
                  <text x={arc.labelX} y={arc.labelY + 4} className="arc__count" textAnchor="middle">
                    {countLabel(arc, mode)}
                  </text>
                </g>
              );
            })}
          </svg>
          <ul className="arch-legend" aria-label="Diagram legend">
            <li>
              <span className="arch-swatch arch-swatch--plain" /> allowed dependency (edge count)
            </li>
            <li>
              <span className="arch-swatch arch-swatch--new" /> new violation
            </li>
            <li>
              <span className="arch-swatch arch-swatch--pre_existing" /> pre-existing violation
            </li>
            <li>
              <span className="arch-swatch arch-swatch--removed" /> removed in head
            </li>
            <li>Right: downward dependencies · left: upward. Select an arc to filter the violations.</li>
          </ul>
        </section>

        <section aria-labelledby="violations-title">
          <div className="section-head">
            <h2 id="violations-title" className="section-title">
              Violations <span className="count">{visibleViolations.length}</span>
            </h2>
            <fieldset className="toolbar__kinds">
              <legend className="sr-only">Status</legend>
              {STATUSES.map((status) => (
                <label key={status} className="chip">
                  <input
                    type="checkbox"
                    checked={statuses.has(status)}
                    onChange={() => {
                      setStatuses((current) => {
                        const next = new Set(current);
                        if (next.has(status)) next.delete(status);
                        else next.add(status);
                        return next;
                      });
                    }}
                  />
                  <DeltaBadge status={status} />
                </label>
              ))}
            </fieldset>
            {pair !== null && (
              <button
                type="button"
                className="button button--quiet"
                onClick={() => {
                  setPair(null);
                }}
              >
                {pair.replace("\u0000", " → ")} ✕
              </button>
            )}
          </div>
          {visibleViolations.length === 0 ? (
            <Empty>No layer-rule violation matches the current filters.</Empty>
          ) : (
            <div className="table-wrap">
              <table className="table">
                <caption className="sr-only">Layer-rule violations with the exact symbol edge</caption>
                <thead>
                  <tr>
                    <th scope="col">Status</th>
                    <th scope="col">Layers</th>
                    <th scope="col">Dependency</th>
                    <th scope="col">Relation</th>
                    <th scope="col">Location</th>
                  </tr>
                </thead>
                <tbody>
                  {visibleViolations.map((v) => {
                    const key = violationKey(v);
                    const isSelected = selected !== null && violationKey(selected) === key;
                    return (
                      <tr key={key} className={isSelected ? "is-selected" : ""}>
                        <td>
                          <DeltaBadge status={v.status} />
                        </td>
                        <td>
                          <span className="layer-pair">
                            {v.from_layer} → {v.to_layer}
                          </span>
                        </td>
                        <td>
                          <button
                            type="button"
                            className="link sym"
                            aria-pressed={isSelected}
                            onClick={() => {
                              setSelectedKey(key);
                            }}
                            title={`${v.edge.from} → ${v.edge.to}`}
                          >
                            <SymbolLink id={v.edge.from} /> → <SymbolLink id={v.edge.to} />
                          </button>
                        </td>
                        <td>
                          <span className={`path__kind path__kind--${v.edge.kind.toLowerCase()}`}>
                            {v.edge.kind.toLowerCase()}
                          </span>
                        </td>
                        <td>
                          <Location file={v.edge.file} line={v.edge.line} />
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}
          {arch.violations_truncated && <p className="muted">The violation list is truncated; counts are complete.</p>}
        </section>

        <section aria-labelledby="cycles-title">
          <h2 id="cycles-title" className="section-title">
            Layer cycles <span className="count">{arch.cycles.length}</span>
          </h2>
          {arch.cycles.length === 0 ? (
            <Empty>{arch.cycles_mode === "off" ? "Cycle detection is off." : "No layer cycle in base or head."}</Empty>
          ) : (
            <ul className="cycles">
              {arch.cycles.map((cycle) => (
                <li key={`${cycle.status}|${cycle.layers.join(">")}`} className="cycles__item">
                  <div className="cycles__head">
                    <DeltaBadge status={cycle.status} />
                    <span className="layer-pair">{cycle.layers.join(" ↔ ")}</span>
                  </div>
                  <table className="table table--compact">
                    <caption className="sr-only">Layer edges closing this cycle, with one example each</caption>
                    <thead>
                      <tr>
                        <th scope="col">Layers</th>
                        <th scope="col" className="num">
                          Edges
                        </th>
                        <th scope="col">Example</th>
                        <th scope="col">Location</th>
                      </tr>
                    </thead>
                    <tbody>
                      {cycle.edges.map((e) => (
                        <tr key={`${e.from}>${e.to}`}>
                          <td className="layer-pair">
                            {e.from} → {e.to}
                          </td>
                          <td className="num">{e.edges}</td>
                          <td>
                            <SymbolLink id={e.example.from} /> → <SymbolLink id={e.example.to} />
                          </td>
                          <td>
                            <Location file={e.example.file} line={e.example.line} />
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </li>
              ))}
            </ul>
          )}
        </section>

        <section aria-labelledby="deps-title">
          <h2 id="deps-title" className="section-title">
            Layer dependencies
          </h2>
          <div className="table-wrap">
            <table className="table">
              <caption className="sr-only">Edges between layers in base and head (the diagram as a table)</caption>
              <thead>
                <tr>
                  <th scope="col">From</th>
                  <th scope="col">To</th>
                  <th scope="col" className="num">
                    Base
                  </th>
                  <th scope="col" className="num">
                    Head
                  </th>
                  <th scope="col">Note</th>
                </tr>
              </thead>
              <tbody>
                {arch.layer_dependencies.map((d) => (
                  <tr key={`${d.from}>${d.to}`}>
                    <td>{d.from}</td>
                    <td>{d.to}</td>
                    <td className="num">{d.base_edges}</td>
                    <td className="num">{d.head_edges}</td>
                    <td>
                      {d.direction_reversed && (
                        <span className="badge badge--warn" title="Head depends in the opposite direction of base">
                          direction reversed
                        </span>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>

        <ModuleCoupling report={report} />

        <section aria-labelledby="rules-title">
          <h2 id="rules-title" className="section-title">
            Rules
          </h2>
          <ol className="rules" start={0}>
            {arch.rules.map((rule) => (
              <li key={rule.index}>
                <code>#{rule.index}</code> {rule.description}{" "}
                <span className="muted">({rule.kind.toLowerCase()})</span>
              </li>
            ))}
          </ol>
        </section>
      </div>

      <aside className="inspector" aria-label="Violation detail">
        {selected ? (
          <>
            <h2 className="panel-title">Violation</h2>
            <p className="inspector__headline">
              {selected.from_layer} → {selected.to_layer} <DeltaBadge status={selected.status} />
            </p>
            <dl className="facts">
              <dt>Rule broken</dt>
              <dd>
                <code>#{selected.rule}</code> {selected.description}
              </dd>
              <dt>From</dt>
              <dd>
                <SymbolLink id={selected.edge.from} onSelect={index.symbolIds.has(selected.edge.from) ? showInGraph : undefined} />
                <code className="break muted small">{selected.edge.from}</code>
              </dd>
              <dt>To</dt>
              <dd>
                <SymbolLink id={selected.edge.to} onSelect={index.symbolIds.has(selected.edge.to) ? showInGraph : undefined} />
                <code className="break muted small">{selected.edge.to}</code>
              </dd>
              <dt>Relation</dt>
              <dd>{selected.edge.kind.toLowerCase()}</dd>
              <dt>Evidence</dt>
              <dd>
                <EvidenceBadge evidence={selected.edge.evidence} />
              </dd>
              <dt>Source</dt>
              <dd>
                {index.filePaths.has(selected.edge.file) ? (
                  <button
                    type="button"
                    className="link loc"
                    onClick={() => {
                      openFile(selected.edge.file, selected.edge.line);
                    }}
                  >
                    {selected.edge.file}:{selected.edge.line}
                  </button>
                ) : (
                  <Location file={selected.edge.file} line={selected.edge.line} />
                )}
              </dd>
              <dt>Extractor rule</dt>
              <dd>
                <code>{selected.edge.rule}</code>
              </dd>
              {selected.base_edge && (
                <>
                  <dt>In base at</dt>
                  <dd>
                    <Location file={selected.base_edge.file} line={selected.base_edge.line} />
                  </dd>
                </>
              )}
            </dl>
            {selected.status === "NEW" && (
              <p className="note note--fail">This dependency does not exist in base: the change introduces it.</p>
            )}
            {selected.status === "REMOVED" && <p className="note">This violation exists in base and not in head.</p>}
          </>
        ) : (
          <p className="muted">Select a violation to see its exact edge.</p>
        )}
      </aside>
    </div>
  );
}

function ModuleCoupling({ report }: { report: AnalysisReport }) {
  const coupling = report.architecture.module_coupling;
  return (
    <section aria-labelledby="coupling-title">
      <h2 id="coupling-title" className="section-title">
        Module coupling changes <span className="count">{coupling.length}</span>
      </h2>
      {coupling.length === 0 ? (
        <Empty>No module pair changed its number of dependencies.</Empty>
      ) : (
        <div className="table-wrap">
          <table className="table">
            <caption className="sr-only">Module pairs whose dependency count changed</caption>
            <thead>
              <tr>
                <th scope="col">From module</th>
                <th scope="col">To module</th>
                <th scope="col" className="num">
                  Base
                </th>
                <th scope="col" className="num">
                  Head
                </th>
              </tr>
            </thead>
            <tbody>
              {coupling.map((c) => (
                <tr key={`${c.from}>${c.to}`}>
                  <td>
                    <code>{c.from}</code>
                  </td>
                  <td>
                    <code>{c.to}</code>
                  </td>
                  <td className="num">{c.base_edges}</td>
                  <td className="num">{c.head_edges}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {report.architecture.module_coupling_truncated && <p className="muted">The list is truncated.</p>}
    </section>
  );
}
