// Visual vocabulary of the impact graph, in the same words the report and inspector use.

const NODE_ROLES: { className: string; label: string; hint: string }[] = [
  { className: "node--changed node--modified", label: "Changed", hint: "modified or re-signed in head" },
  { className: "node--changed node--added", label: "Added", hint: "exists only in head" },
  { className: "node--changed node--deleted", label: "Deleted", hint: "exists only in base" },
  { className: "node--direct", label: "Direct dependent", hint: "depth 1" },
  { className: "node--transitive", label: "Transitive", hint: "depth 2 or more" },
  { className: "node--test", label: "Test", hint: "test code reached by the change" },
  { className: "node--cluster", label: "Module cluster", hint: "collapsed module; click to expand" },
];

const EDGE_STYLES: { className: string; label: string; hint: string }[] = [
  { className: "swatch-edge--exact", label: "Resolved exactly", hint: "one declaration by scoping rules" },
  { className: "swatch-edge--inferred", label: "Inferred", hint: "plausible, not certain" },
  { className: "swatch-edge--measured", label: "Measured coverage", hint: "a test executed it" },
  { className: "swatch-edge--dispatch", label: "Overrides / dispatch", hint: "runtime dispatch" },
  { className: "swatch-edge--path", label: "Explaining path", hint: "of the selection" },
];

export function Legend() {
  return (
    <details className="legend">
      <summary>Legend</summary>
      <div className="legend__body">
        <p className="legend__heading">Nodes</p>
        <ul className="legend__list">
          {NODE_ROLES.map((role) => (
            <li key={role.label}>
              <span className={`swatch-node ${role.className}`} aria-hidden="true" />
              <span>
                {role.label} <span className="muted">— {role.hint}</span>
              </span>
            </li>
          ))}
        </ul>
        <p className="legend__heading">Edges (dependent → dependency)</p>
        <ul className="legend__list">
          {EDGE_STYLES.map((style) => (
            <li key={style.label}>
              <svg className={`swatch-edge ${style.className}`} width="28" height="8" aria-hidden="true">
                <line x1="1" y1="4" x2="27" y2="4" />
              </svg>
              <span>
                {style.label} <span className="muted">— {style.hint}</span>
              </span>
            </li>
          ))}
        </ul>
      </div>
    </details>
  );
}
