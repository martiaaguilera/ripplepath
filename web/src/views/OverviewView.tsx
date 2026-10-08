import { useEffect, useMemo, useState } from "react";
import type { AnalysisReport, Edge, EdgeKind, Evidence } from "../api/types";
import type { Navigate, UrlState } from "../app/url";
import { EVIDENCE_LABEL } from "../graph/model";
import { EdgeTable, NodeList } from "../components/GraphLists";
import { Inspector } from "../components/Inspector";
import { ChangedList, ImpactTable, TestsPanel, UncertaintyPanel } from "../components/Panels";
import { displayGraph } from "../graph/clusters";
import { ImpactGraph, type Selection } from "../graph/ImpactGraph";
import { EMPTY_HIGHLIGHT, EVIDENCE_CLASSES, IMPACT_EDGE_KINDS, buildView, edgeKey, highlightFor } from "../graph/model";

type Tab = "tests" | "impact" | "nodes" | "edges" | "uncertainty";

interface Props {
  report: AnalysisReport;
  url: UrlState;
  navigate: Navigate;
}

function toggle<T>(set: ReadonlySet<T>, value: T): Set<T> {
  const next = new Set(set);
  if (next.has(value)) next.delete(value);
  else next.add(value);
  return next;
}

export function OverviewView({ report, url, navigate }: Props) {
  const [edgeSelection, setEdgeSelection] = useState<Edge | null>(null);
  const [maxDepth, setMaxDepth] = useState(report.summary.max_depth);
  const [edgeKinds, setEdgeKinds] = useState<ReadonlySet<EdgeKind>>(() => new Set(IMPACT_EDGE_KINDS));
  const [evidence, setEvidence] = useState<ReadonlySet<Evidence>>(() => new Set(EVIDENCE_CLASSES));
  const [query, setQuery] = useState("");
  const [groupByModule, setGroupByModule] = useState(false);
  const [collapse, setCollapse] = useState(false);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set());
  const [hideIsolated, setHideIsolated] = useState(true);
  const [tab, setTab] = useState<Tab>("tests");

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setEdgeSelection(null);
      navigate({ sel: null }, { replace: true });
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [navigate]);

  const selection: Selection = edgeSelection
    ? { type: "edge", edge: edgeSelection }
    : url.sel
      ? { type: "node", id: url.sel }
      : null;

  const view = useMemo(
    () => buildView(report, { maxDepth, edgeKinds, evidence, query, hideIsolated }),
    [report, maxDepth, edgeKinds, evidence, query, hideIsolated],
  );
  const display = useMemo(() => displayGraph(view, collapse, expanded), [view, collapse, expanded]);
  const highlight = useMemo(() => {
    if (!selection) return EMPTY_HIGHLIGHT;
    if (selection.type === "node") return highlightFor(report, selection.id);
    return { nodes: new Set([selection.edge.from, selection.edge.to]), edges: new Set([edgeKey(selection.edge)]) };
  }, [report, selection]);

  const selectSymbol = (id: string) => {
    setEdgeSelection(null);
    navigate({ sel: id }, { replace: true });
  };
  const select = (next: Selection) => {
    if (next?.type === "edge") {
      setEdgeSelection(next.edge);
      return;
    }
    setEdgeSelection(null);
    navigate({ sel: next?.id ?? null }, { replace: true });
  };
  const openFile = (path: string) => {
    navigate({ view: "diff", file: path });
  };
  const expandModule = (module: string) => {
    setExpanded((current) => new Set(current).add(module));
  };

  const kindsPresent = IMPACT_EDGE_KINDS.filter((k) => report.graph.edges.some((e) => e.kind === k));
  const evidencePresent = EVIDENCE_CLASSES.filter((c) => report.graph.edges.some((e) => e.evidence === c));
  const selectedId = selection?.type === "node" ? selection.id : null;
  const tabs: [Tab, string][] = [
    ["tests", `Tests (${report.tests.length})`],
    ["impact", `Impacted (${report.impacted_symbols.length})`],
    ["nodes", `Nodes (${view.nodes.length})`],
    ["edges", `Edges (${view.edges.length})`],
    ["uncertainty", `Uncertainty (${report.uncertainty.length})`],
  ];

  return (
    <div className="workspace">
      <ChangedList report={report} selectedId={selectedId} onSelectSymbol={selectSymbol} onOpenFile={openFile} />
      <section className="center" aria-labelledby="overview-title">
        <h1 id="overview-title" className="view-title" tabIndex={-1}>
          Blast radius
        </h1>
        <div className="toolbar" role="toolbar" aria-label="Graph filters">
          <input
            id="graph-search"
            type="search"
            placeholder="Find symbol…  /"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
            }}
            aria-label="Find symbol in graph"
          />
          <label className="toolbar__depth">
            depth ≤ {maxDepth}
            <input
              type="range"
              min={1}
              max={Math.max(1, report.summary.max_depth)}
              value={maxDepth}
              onChange={(e) => {
                setMaxDepth(Number(e.target.value));
              }}
            />
          </label>
          <label className="toolbar__toggle">
            <input
              type="checkbox"
              checked={collapse}
              onChange={(e) => {
                setCollapse(e.target.checked);
                setExpanded(new Set());
              }}
            />
            collapse modules
          </label>
          {collapse && expanded.size > 0 && (
            <button
              type="button"
              className="button button--quiet"
              onClick={() => {
                setExpanded(new Set());
              }}
            >
              re-collapse {expanded.size}
            </button>
          )}
          <label className="toolbar__toggle">
            <input
              type="checkbox"
              checked={groupByModule}
              onChange={(e) => {
                setGroupByModule(e.target.checked);
              }}
            />
            group by module
          </label>
          <label className="toolbar__toggle">
            <input
              type="checkbox"
              checked={hideIsolated}
              onChange={(e) => {
                setHideIsolated(e.target.checked);
              }}
            />
            hide symbols without edges
            {view.hiddenIsolated > 0 && <span className="muted">({view.hiddenIsolated} hidden)</span>}
          </label>
        </div>
        <div className="toolbar toolbar--chips">
          <fieldset className="toolbar__kinds">
            <legend>Relation</legend>
            {kindsPresent.map((kind) => (
              <label key={kind} className={`chip chip--${kind.toLowerCase()}`}>
                <input
                  type="checkbox"
                  checked={edgeKinds.has(kind)}
                  onChange={() => {
                    setEdgeKinds((current) => toggle(current, kind));
                  }}
                />
                {kind.toLowerCase()}
              </label>
            ))}
          </fieldset>
          <fieldset className="toolbar__kinds">
            <legend>Evidence</legend>
            {evidencePresent.map((cls) => (
              <label key={cls} className={`chip chip--ev chip--ev-${cls.toLowerCase()}`}>
                <input
                  type="checkbox"
                  checked={evidence.has(cls)}
                  onChange={() => {
                    setEvidence((current) => toggle(current, cls));
                  }}
                />
                {EVIDENCE_LABEL[cls].toLowerCase()}
              </label>
            ))}
          </fieldset>
        </div>
        {report.graph.clamped && (
          <p className="note" role="status">
            Showing the {report.graph.node_cap} closest symbols of {report.summary.symbols_impacted} impacted; the
            Impacted table is complete.
          </p>
        )}
        <ImpactGraph
          graph={display}
          highlight={highlight}
          selection={selection}
          groupByModule={groupByModule}
          onSelect={select}
          onExpandModule={expandModule}
        />
        <div className="tabs" role="tablist" aria-label="Details">
          {tabs.map(([id, label]) => (
            <button
              key={id}
              type="button"
              role="tab"
              id={`tab-${id}`}
              aria-selected={tab === id}
              aria-controls={`panel-${id}`}
              tabIndex={tab === id ? 0 : -1}
              className={`tab ${tab === id ? "is-active" : ""}`}
              onClick={() => {
                setTab(id);
              }}
              onKeyDown={(event) => {
                if (event.key !== "ArrowRight" && event.key !== "ArrowLeft") return;
                const index = tabs.findIndex(([t]) => t === id);
                const next = tabs[(index + (event.key === "ArrowRight" ? 1 : tabs.length - 1)) % tabs.length];
                if (next) {
                  setTab(next[0]);
                  document.getElementById(`tab-${next[0]}`)?.focus();
                }
              }}
            >
              {label}
            </button>
          ))}
        </div>
        <div className="tabpanel" role="tabpanel" id={`panel-${tab}`} aria-labelledby={`tab-${tab}`}>
          {tab === "tests" && <TestsPanel report={report} selectedId={selectedId} onSelectSymbol={selectSymbol} />}
          {tab === "impact" && <ImpactTable report={report} selectedId={selectedId} onSelectSymbol={selectSymbol} />}
          {tab === "nodes" && <NodeList nodes={view.nodes} selectedId={selectedId} onSelect={selectSymbol} />}
          {tab === "edges" && (
            <EdgeTable
              edges={view.edges}
              selectedKey={edgeSelection ? edgeKey(edgeSelection) : null}
              onSelectEdge={setEdgeSelection}
              onSelectSymbol={selectSymbol}
            />
          )}
          {tab === "uncertainty" && <UncertaintyPanel report={report} />}
        </div>
      </section>
      <Inspector report={report} selection={selection} onSelectSymbol={selectSymbol} onOpenFile={openFile} />
    </div>
  );
}
