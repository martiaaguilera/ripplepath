import type { Hop } from "../api/types";
import { EVIDENCE_LABEL, shortLabel } from "../graph/model";

interface Props {
  root: string;
  hops: Hop[];
  onSelectSymbol?: (id: string) => void;
}

/**
 * The explaining path as a vertical chain: changed symbol first, each step labelled with the
 * relation and the exact source location that justifies it.
 */
export function EvidencePath({ root, hops, onSelectSymbol }: Props) {
  const symbol = (id: string) =>
    onSelectSymbol ? (
      <button type="button" className="path__symbol link" onClick={() => onSelectSymbol(id)} title={id}>
        {shortLabel(id)}
      </button>
    ) : (
      <span className="path__symbol" title={id}>
        {shortLabel(id)}
      </span>
    );

  return (
    <ol className="path" aria-label="Explaining path from the changed symbol">
      <li className="path__step path__step--root">
        <span className="path__badge path__badge--changed">changed</span>
        {symbol(root)}
      </li>
      {hops.map((hop) => (
        <li key={`${hop.edge.from}|${hop.edge.to}|${hop.edge.kind}`} className="path__step">
          <span className="path__relation">
            <span className={`path__kind path__kind--${hop.edge.kind.toLowerCase()}`}>
              {hop.via_dispatch ? "dispatch" : hop.edge.kind.toLowerCase()}
            </span>
            <span
              className={`evidence evidence--${hop.edge.evidence.toLowerCase()}`}
              title={EVIDENCE_LABEL[hop.edge.evidence]}
            >
              {hop.edge.evidence.replace("_", " ").toLowerCase()}
            </span>
            <code className="path__location">
              {hop.edge.file}:{hop.edge.line}
            </code>
          </span>
          {symbol(hop.symbol)}
        </li>
      ))}
    </ol>
  );
}
