// Likely reviewers from CODEOWNERS in head. Review-routing metadata, never authorization.

import type { AnalysisReport } from "../api/types";
import type { Navigate } from "../app/url";
import { Empty } from "../components/common";

interface Props {
  report: AnalysisReport;
  navigate: Navigate;
}

export function OwnersView({ report, navigate }: Props) {
  const owners = report.owners;
  const changedPaths = new Set(report.files.map((f) => f.path));
  return (
    <div className="page">
      <header className="page__header">
        <h1 className="view-title" tabIndex={-1}>
          Owners
        </h1>
        <p className="page__lede">{owners.note}</p>
      </header>

      {owners.source === null ? (
        <Empty>
          No CODEOWNERS file was found in head (looked in the repository root, <code>.github/</code> and{" "}
          <code>docs/</code>), so no likely reviewers can be named.
        </Empty>
      ) : (
        <>
          <dl className="facts facts--inline">
            <dt>Source</dt>
            <dd>
              <code>{owners.source}</code>
              {owners.changed_in_head && <span className="badge badge--warn">changed in this change</span>}
            </dd>
            <dt>Owners</dt>
            <dd>{owners.owners.length}</dd>
            <dt>Changed files without owner</dt>
            <dd className={owners.unowned_changed_files > 0 ? "tone-warn" : ""}>{owners.unowned_changed_files}</dd>
          </dl>
          {owners.changed_in_head && (
            <p className="note note--warn">
              This change edits CODEOWNERS itself; the owners below are read from head and may not reflect the base
              branch's review rules.
            </p>
          )}

          <section aria-labelledby="owner-summary-title">
            <h2 id="owner-summary-title" className="section-title">
              Likely reviewers
            </h2>
            {owners.owners.length === 0 ? (
              <Empty>No CODEOWNERS rule matches a changed or impacted file.</Empty>
            ) : (
              <div className="table-wrap">
                <table className="table">
                  <caption className="sr-only">Owners with the number of changed and impacted files they own</caption>
                  <thead>
                    <tr>
                      <th scope="col">Owner</th>
                      <th scope="col" className="num">
                        Changed files
                      </th>
                      <th scope="col" className="num">
                        Impacted files
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {owners.owners.map((o) => (
                      <tr key={o.owner}>
                        <th scope="row">
                          <code>{o.owner}</code>
                        </th>
                        <td className="num">{o.changed_files}</td>
                        <td className="num">{o.impacted_files}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </section>
        </>
      )}

      <section aria-labelledby="owner-files-title">
        <h2 id="owner-files-title" className="section-title">
          Files <span className="count">{owners.files.length}</span>
        </h2>
        {owners.files.length === 0 ? (
          <Empty>No changed or impacted files.</Empty>
        ) : (
          <div className="table-wrap">
            <table className="table">
              <caption className="sr-only">Changed and impacted files with their CODEOWNERS owners</caption>
              <thead>
                <tr>
                  <th scope="col">File</th>
                  <th scope="col">Role</th>
                  <th scope="col">Owners</th>
                  <th scope="col" className="num">
                    Rule line
                  </th>
                </tr>
              </thead>
              <tbody>
                {owners.files.map((f) => (
                  <tr key={`${f.role}|${f.path}`}>
                    <td>
                      {changedPaths.has(f.path) ? (
                        <button
                          type="button"
                          className="link loc"
                          onClick={() => {
                            navigate({ view: "diff", file: f.path, line: null });
                          }}
                        >
                          {f.path}
                        </button>
                      ) : (
                        <code className="loc">{f.path}</code>
                      )}
                    </td>
                    <td>
                      <span className={`badge ${f.role === "changed" ? "badge--modified" : "badge--neutral"}`}>
                        {f.role}
                      </span>
                    </td>
                    <td>
                      {f.owners.length > 0 ? (
                        f.owners.map((o) => (
                          <code key={o} className="owner">
                            {o}
                          </code>
                        ))
                      ) : (
                        <span className="muted">no owner</span>
                      )}
                    </td>
                    <td className="num">{f.line ?? "—"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        {owners.files_truncated && <p className="muted">The file list is truncated.</p>}
      </section>

      {owners.errors.length > 0 && (
        <section aria-labelledby="owner-errors-title">
          <h2 id="owner-errors-title" className="section-title">
            CODEOWNERS problems
          </h2>
          <ul className="plain-list">
            {owners.errors.map((e) => (
              <li key={`${e.line}:${e.message}`} className="note note--warn">
                line {e.line}: {e.message}
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}
