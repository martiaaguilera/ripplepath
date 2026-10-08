// Risk decomposition, merge policy and public API changes, exactly as the report states them. The
// UI adds no weighting, no thresholds and no interpretation beyond labels.

import type { AnalysisReport } from "../api/types";
import { indexReport } from "../app/reportIndex";
import type { Navigate } from "../app/url";
import { Empty, EvidenceText, LevelBadge, Location, StatusBadge, SymbolLink, humanize } from "../components/common";

interface Props {
  report: AnalysisReport;
  navigate: Navigate;
}

export function RiskView({ report, navigate }: Props) {
  const { risk, policy, api_surface: api } = report;
  const index = indexReport(report);
  const onSymbol = (id: string) => {
    navigate({ view: "overview", sel: id });
  };
  const onFile = (file: string) => {
    navigate({ view: "diff", file });
  };
  const scored = risk.signals.filter((s) => s.points > 0).length;
  const notEvaluated = risk.signals.filter((s) => !s.evaluated).length;

  return (
    <div className="page">
      <header className="page__header">
        <h1 className="view-title" tabIndex={-1}>
          Risk &amp; policy
        </h1>
        <p className="page__lede">
          A decomposition of review-relevant signals under risk model v{risk.model_version}. Every point is traceable to
          a signal, its measured value and its evidence.
        </p>
      </header>

      <section className="riskhead" aria-label="Risk score">
        <div className="riskhead__score">
          <span className="riskhead__number">{risk.score}</span>
          <span className="riskhead__max">/ 100</span>
          <LevelBadge level={risk.level} />
        </div>
        <dl className="facts facts--inline">
          <dt>Signals with points</dt>
          <dd>
            {scored} of {risk.signals.length}
          </dd>
          <dt>Sum before cap</dt>
          <dd>{risk.uncapped_total}</dd>
          <dt>Not evaluated</dt>
          <dd>{notEvaluated}</dd>
          <dt>Model</dt>
          <dd>v{risk.model_version}</dd>
        </dl>
        <p className="riskhead__disclaimer" role="note">
          <strong>The risk score is not a probability of failure.</strong> {risk.interpretation} No merge gate uses it.
        </p>
      </section>

      <section aria-labelledby="signals-title">
        <h2 id="signals-title" className="section-title">
          Decomposition
        </h2>
        <div className="table-wrap">
          <table className="table table--risk">
            <caption className="sr-only">
              Risk signals: points = min(units × weight, cap); score = min(sum of points, 100)
            </caption>
            <thead>
              <tr>
                <th scope="col">Signal</th>
                <th scope="col" className="num">
                  Value
                </th>
                <th scope="col" className="num">
                  Units
                </th>
                <th scope="col" className="num">
                  Weight
                </th>
                <th scope="col" className="num">
                  Cap
                </th>
                <th scope="col">Points</th>
                <th scope="col">Evidence</th>
              </tr>
            </thead>
            <tbody>
              {risk.signals.map((signal) => (
                <tr
                  key={signal.id}
                  className={[!signal.evaluated ? "is-muted" : "", signal.points > 0 ? "has-points" : ""].join(" ")}
                >
                  <th scope="row">
                    <span className="signal__name">{humanize(signal.id)}</span>
                    <code className="signal__id">{signal.id}</code>
                    <span className="signal__def">{signal.definition}</span>
                  </th>
                  <td className="num">{signal.evaluated ? signal.value : "—"}</td>
                  <td className="num">{signal.evaluated ? signal.units : "—"}</td>
                  <td className="num">×{signal.weight}</td>
                  <td className="num">{signal.cap}</td>
                  <td>
                    <span className="points">
                      <span className="points__value">{signal.points}</span>
                      <span
                        className="points__bar"
                        role="img"
                        aria-label={`${signal.points} of a maximum ${signal.cap} points`}
                      >
                        <span
                          className={`points__fill ${signal.points >= signal.cap && signal.cap > 0 ? "is-capped" : ""}`}
                          style={{ width: `${signal.cap > 0 ? (signal.points / signal.cap) * 100 : 0}%` }}
                        />
                      </span>
                    </span>
                  </td>
                  <td className="signal__evidence">
                    {!signal.evaluated && <span className="muted">Not evaluated. </span>}
                    {signal.note && <span className="muted">{signal.note} </span>}
                    {signal.evidence.length > 0 && (
                      <ul className="plain-list">
                        {signal.evidence.map((item) => (
                          <li key={item}>
                            <EvidenceText
                              text={item}
                              symbols={index.symbolIds}
                              files={index.filePaths}
                              onSymbol={onSymbol}
                              onFile={onFile}
                            />
                          </li>
                        ))}
                      </ul>
                    )}
                    {signal.evidence_truncated > 0 && (
                      <span className="muted">and {signal.evidence_truncated} more not listed</span>
                    )}
                    {signal.evaluated && signal.evidence.length === 0 && !signal.note && <span className="muted">—</span>}
                  </td>
                </tr>
              ))}
            </tbody>
            <tfoot>
              <tr>
                <th scope="row">Total</th>
                <td colSpan={4} className="muted">
                  min(Σ points, 100)
                </td>
                <td>
                  <span className="points__value">{risk.score}</span>{" "}
                  {risk.uncapped_total !== risk.score && (
                    <span className="muted">(Σ {risk.uncapped_total} before cap)</span>
                  )}
                </td>
                <td>
                  <LevelBadge level={risk.level} />
                </td>
              </tr>
            </tfoot>
          </table>
        </div>
      </section>

      <section aria-labelledby="policy-title">
        <h2 id="policy-title" className="section-title">
          Merge policy <StatusBadge status={policy.result} />
        </h2>
        <p className="muted">{policy.note}</p>
        <div className="table-wrap">
          <table className="table">
            <caption className="sr-only">Merge-policy gates, their configured level and result</caption>
            <thead>
              <tr>
                <th scope="col">Gate</th>
                <th scope="col">Configured</th>
                <th scope="col">Result</th>
                <th scope="col">Detail and evidence</th>
              </tr>
            </thead>
            <tbody>
              {policy.gates.map((gate) => (
                <tr key={gate.gate} className={gate.status === "OFF" ? "is-muted" : ""}>
                  <th scope="row">
                    <code>{gate.gate}</code>
                  </th>
                  <td>{gate.level.toLowerCase()}</td>
                  <td>
                    <StatusBadge status={gate.status} />
                  </td>
                  <td>
                    <span>{gate.detail}</span>
                    {gate.evidence.length > 0 && (
                      <ul className="plain-list gate__evidence">
                        {gate.evidence.map((item) => (
                          <li key={item}>
                            <EvidenceText
                              text={item}
                              symbols={index.symbolIds}
                              files={index.filePaths}
                              onSymbol={onSymbol}
                              onFile={onFile}
                            />
                          </li>
                        ))}
                      </ul>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>

      <section aria-labelledby="api-title">
        <h2 id="api-title" className="section-title">
          Public API surface{" "}
          <span className="count">
            {api.breaking} breaking · {api.added} added
          </span>
        </h2>
        {api.changes.length === 0 ? (
          <Empty>No public API symbol was removed, narrowed, re-signed or added.</Empty>
        ) : (
          <div className="table-wrap">
            <table className="table">
              <caption className="sr-only">Public API changes</caption>
              <thead>
                <tr>
                  <th scope="col">Change</th>
                  <th scope="col">Symbol</th>
                  <th scope="col">Previously</th>
                  <th scope="col">Location</th>
                </tr>
              </thead>
              <tbody>
                {api.changes.map((change) => (
                  <tr key={`${change.kind}|${change.id}`}>
                    <td>
                      <span className={`badge badge--api-${change.kind.toLowerCase()}`}>{humanize(change.kind)}</span>
                    </td>
                    <td>
                      <SymbolLink id={change.id} onSelect={index.symbolIds.has(change.id) ? onSymbol : undefined} />
                    </td>
                    <td>{change.previous_id ? <SymbolLink id={change.previous_id} /> : <span className="muted">—</span>}</td>
                    <td>
                      <Location file={change.file} line={change.line} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        {api.truncated && <p className="muted">The list is truncated; counts above are complete.</p>}
      </section>

      <section aria-labelledby="config-title">
        <h2 id="config-title" className="section-title">
          Configuration
        </h2>
        <dl className="facts">
          <dt>Rules from</dt>
          <dd>
            {report.config.source === "DEFAULTS"
              ? "defaults (no ripplepath.yml at base)"
              : report.config.source === "INVALID_USING_DEFAULTS"
                ? "defaults — ripplepath.yml at base is invalid"
                : `${report.config.path} at ${report.config.revision}`}
          </dd>
          <dt>Change in head</dt>
          <dd>{humanize(report.config.head_change).toLowerCase()}</dd>
          <dt>Critical paths</dt>
          <dd>{report.config.critical.length > 0 ? report.config.critical.map((c) => <code key={c}>{c} </code>) : "none"}</dd>
        </dl>
        {[...report.config.errors, ...report.config.head_errors].map((error) => (
          <p key={error} className="note note--warn">
            {error}
          </p>
        ))}
      </section>
    </div>
  );
}
