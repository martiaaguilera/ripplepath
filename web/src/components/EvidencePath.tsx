import type { Hop } from "../api/types";
import { EVIDENCE_LABEL, shortLabel } from "../graph/model";

interface Props {
  root: string;
  hops: Hop[];
  onSelectSymbol?: ((id: string) => void) | undefined;
  /** Accessible name; defaults to the impact wording. */
  label?: string;
}

/**
 * The explaining path as a vertical chain: changed symbol first, each step labelled with the
 * relation and the exact source location that justifies it. Measured coverage hops are drawn
 * differently from static ones: they come from a test run, not from reading the code.
 */
export function EvidencePath({ root, hops, onSelectSymbol, label }: Props) {
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
    <ol className="path" aria-label={label ?? "Explaining path from the changed symbol"}>
      <li className="path__step path__step--root">
        <span className="path__badge path__badge--changed">changed</span>
        {symbol(root)}
      </li>
      {hops.map((hop) => {
        const measured = hop.edge.evidence === "COVERAGE_OBSERVED";
        return (
          <li
            key={`${hop.edge.from}|${hop.edge.to}|${hop.edge.kind}`}
            className={`path__step ${measured ? "path__step--measured" : "path__step--static"}`}
          >
            <span className="path__relation">
              <span className={`path__kind path__kind--${hop.edge.kind.toLowerCase()}`}>
                {hop.via_dispatch ? "dispatch" : hop.edge.kind.toLowerCase()}
              </span>
              <span
                className={`evidence evidence--${hop.edge.evidence.toLowerCase()}`}
                title={EVIDENCE_LABEL[hop.edge.evidence]}
              >
                {measured ? "measured by coverage" : EVIDENCE_LABEL[hop.edge.evidence].toLowerCase()}
              </span>
              {measured ? (
                <code className="path__location" title="Coverage report this edge was recorded from">
                  {hop.edge.rule}
                </code>
              ) : (
                <code className="path__location">
                  {hop.edge.file}:{hop.edge.line}
                </code>
              )}
            </span>
            {symbol(hop.symbol)}
          </li>
        );
      })}
    </ol>
  );
}
