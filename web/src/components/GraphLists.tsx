// Text equivalents of the impact graph: every node and every edge it draws, in lists that work
// with a keyboard and a screen reader.

import { useId, useState, type KeyboardEvent } from "react";
import type { Edge } from "../api/types";
import { edgeKey, type ViewNode } from "../graph/model";
import { KIND_GLYPH, roleText } from "../graph/SymbolNode";
import { EvidenceBadge, Location, SymbolLink } from "./common";

interface NodeListProps {
  nodes: readonly ViewNode[];
  selectedId: string | null;
  onSelect: (id: string) => void;
}

/** Listbox with roving active option: arrows move, Enter or Space selects, Home/End jump. */
export function NodeList({ nodes, selectedId, onSelect }: NodeListProps) {
  const ordered = [...nodes].sort((a, b) => a.depth - b.depth || a.id.localeCompare(b.id));
  const baseId = useId();
  const selectedIndex = ordered.findIndex((n) => n.id === selectedId);
  const [active, setActive] = useState(0);
  // Keep the active option valid when the filters shrink the list.
  const activeIndex = Math.min(active, Math.max(ordered.length - 1, 0));

  if (ordered.length === 0) return <p className="muted">No symbols match the current filters.</p>;

  const move = (index: number) => {
    const next = Math.max(0, Math.min(ordered.length - 1, index));
    setActive(next);
    document.getElementById(`${baseId}-${next}`)?.scrollIntoView({ block: "nearest" });
  };

  const onKeyDown = (event: KeyboardEvent<HTMLUListElement>) => {
    const handlers: Record<string, () => void> = {
      ArrowDown: () => {
        move(activeIndex + 1);
      },
      ArrowUp: () => {
        move(activeIndex - 1);
      },
      Home: () => {
        move(0);
      },
      End: () => {
        move(ordered.length - 1);
      },
      PageDown: () => {
        move(activeIndex + 10);
      },
      PageUp: () => {
        move(activeIndex - 10);
      },
      Enter: () => {
        const node = ordered[activeIndex];
        if (node) onSelect(node.id);
      },
      " ": () => {
        const node = ordered[activeIndex];
        if (node) onSelect(node.id);
      },
    };
    const handler = handlers[event.key];
    if (handler) {
      event.preventDefault();
      handler();
    }
  };

  return (
    <>
      <p className="hint" id={`${baseId}-hint`}>
        {ordered.length} symbols in the graph. Use ↑ ↓ to move, Enter to inspect.
      </p>
      <ul
        className="nodelist"
        role="listbox"
        tabIndex={0}
        aria-label="Graph symbols"
        aria-describedby={`${baseId}-hint`}
        aria-activedescendant={`${baseId}-${activeIndex}`}
        onKeyDown={onKeyDown}
        onFocus={() => {
          if (selectedIndex >= 0 && active === 0) setActive(selectedIndex);
        }}
      >
        {ordered.map((node, index) => (
          <li
            key={node.id}
            id={`${baseId}-${index}`}
            role="option"
            aria-selected={node.id === selectedId}
            className={[
              "nodelist__item",
              index === activeIndex ? "is-active" : "",
              node.id === selectedId ? "is-selected" : "",
            ].join(" ")}
            onClick={() => {
              setActive(index);
              onSelect(node.id);
            }}
          >
            <span className="node__glyph" aria-hidden="true">
              {KIND_GLYPH[node.kind]}
            </span>
            <span className="nodelist__label" title={node.id}>
              {node.label}
            </span>
            <span className={`nodelist__role nodelist__role--${node.role}`}>{roleText(node)}</span>
            <span className="nodelist__module muted">{node.module}</span>
            <Location file={node.file} line={node.line} />
          </li>
        ))}
      </ul>
    </>
  );
}

interface EdgeTableProps {
  edges: readonly Edge[];
  selectedKey: string | null;
  onSelectEdge: (edge: Edge) => void;
  onSelectSymbol: (id: string) => void;
}

export function EdgeTable({ edges, selectedKey, onSelectEdge, onSelectSymbol }: EdgeTableProps) {
  if (edges.length === 0) return <p className="muted">No edges match the current filters.</p>;
  return (
    <div className="table-wrap">
      <table className="table">
        <caption className="sr-only">Graph edges, dependent to dependency, with their source evidence</caption>
        <thead>
          <tr>
            <th scope="col">Dependent</th>
            <th scope="col">Relation</th>
            <th scope="col">Dependency</th>
            <th scope="col">Evidence</th>
            <th scope="col">Source</th>
            <th scope="col">Rule</th>
            <th scope="col">
              <span className="sr-only">Inspect</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {edges.map((edge) => {
            const key = edgeKey(edge);
            return (
              <tr key={key} className={key === selectedKey ? "is-selected" : ""}>
                <td>
                  <SymbolLink id={edge.from} onSelect={onSelectSymbol} />
                </td>
                <td>
                  <span className={`path__kind path__kind--${edge.kind.toLowerCase()}`}>{edge.kind.toLowerCase()}</span>
                </td>
                <td>
                  <SymbolLink id={edge.to} onSelect={onSelectSymbol} />
                </td>
                <td>
                  <EvidenceBadge evidence={edge.evidence} />
                </td>
                <td>
                  <Location file={edge.file} line={edge.line} />
                </td>
                <td>
                  <code>{edge.rule}</code>
                </td>
                <td>
                  <button
                    type="button"
                    className="button button--quiet"
                    onClick={() => {
                      onSelectEdge(edge);
                    }}
                  >
                    Inspect
                  </button>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
