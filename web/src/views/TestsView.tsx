// Test intelligence: the selection decision with every reason behind it, the ranked tests with the
// strength and kind of their evidence, recorded reliability and duration, and the evidence path of
// the selected test. Ordering, tiers and the decision come from the report; the UI only renders.

import type { AnalysisReport, SelectionMode, TestRecommendation } from "../api/types";
import type { Navigate, UrlState } from "../app/url";
import {
  Empty,
  EvidenceBadge,
  Location,
  ReliabilityBadge,
  SeverityBadge,
  SymbolLink,
  TierBadge,
  formatDuration,
  humanize,
} from "../components/common";
import { EvidencePath } from "../components/EvidencePath";
import { evidenceKind, orderedTests } from "../components/Panels";
import { useWindowedRows } from "../components/useWindowedRows";
import { shortLabel } from "../graph/model";

const MODE_HINT: Record<SelectionMode, string> = {
  CONSERVATIVE: "falls back to the full suite on any fallback reason",
  BALANCED: "falls back to the full suite on a high-severity reason",
  FAST_FEEDBACK: "always the first ranked tests; never a claim of complete validation",
};

const ROW_HEIGHT = 37;

interface Props {
  report: AnalysisReport;
  url: UrlState;
  navigate: Navigate;
}

export function TestsView({ report, url, navigate }: Props) {
  const selection = report.test_selection;
  const tests = orderedTests(report);
  const selected = tests.find((t) => t.id === url.test) ?? tests[0] ?? null;
  const rows = useWindowedRows(tests.length, ROW_HEIGHT);
  const fullSuite = selection.decision === "FULL_SUITE";
  const evidence = report.evidence;
  const onSymbol = (id: string) => {
    navigate({ view: "overview", sel: id });
  };

  return (
    <div className="page page--split">
      <div className="page__main">
        <header className="page__header">
          <h1 className="view-title" tabIndex={-1}>
            Test intelligence
          </h1>
          <p className="page__lede">
            Ripplepath never runs tests. It ranks the tests your CI already has by the evidence that they exercise this
            change, and says when that evidence is too weak to run less than everything.
          </p>
        </header>

        <section
          className={`decision ${fullSuite ? "decision--full" : "decision--selected"}`}
          aria-labelledby="decision-title"
        >
          <div className="decision__head">
            <span className="decision__label">Decision</span>
            <h2 id="decision-title" className="decision__title">
              {fullSuite
                ? "Run the full suite"
                : `Run ${selection.selected_units} of ${selection.total_units} test units`}
            </h2>
            <span className="decision__mode" title={MODE_HINT[selection.mode]}>
              {humanize(selection.mode).toLowerCase()} mode — {MODE_HINT[selection.mode]}
            </span>
          </div>
          <dl className="facts facts--inline">
            <dt>Recommended units</dt>
            <dd>
              {selection.selected_units} of {selection.total_units}
            </dd>
            <dt>Runtime, recommended</dt>
            <dd>
              {selection.selected_runtime_ms !== null ? formatDuration(selection.selected_runtime_ms) : "not estimated"}
            </dd>
            <dt>Runtime, full suite</dt>
            <dd>{selection.full_runtime_ms !== null ? formatDuration(selection.full_runtime_ms) : "not estimated"}</dd>
          </dl>
          {selection.notes.length > 0 && (
            <ul className="decision__notes">
              {selection.notes.map((note) => (
                <li key={note}>{note}</li>
              ))}
            </ul>
          )}
        </section>

        <section aria-labelledby="fallback-title">
          <h2 id="fallback-title" className="section-title">
            Fallback reasons <span className="count">{selection.fallback_reasons.length}</span>
          </h2>
          {selection.fallback_reasons.length === 0 ? (
            <Empty>No reason to widen the selection was found.</Empty>
          ) : (
            <ul className="reasons">
              {selection.fallback_reasons.map((reason) => (
                <li key={`${reason.code}|${reason.detail}`} className="reasons__item">
                  <SeverityBadge severity={reason.severity} />
                  <code className="reasons__code">{reason.code}</code>
                  <span>{reason.detail}</span>
                </li>
              ))}
            </ul>
          )}
        </section>

        <section aria-labelledby="ranked-title">
          <h2 id="ranked-title" className="section-title">
            Ranked tests <span className="count">{tests.length}</span>
          </h2>
          {tests.length === 0 ? (
            <Empty>No test has a dependency path or measured coverage reaching this change.</Empty>
          ) : (
            <div
              ref={rows.ref}
              className={`table-wrap ${rows.windowed ? "table-wrap--windowed" : ""}`}
              {...(rows.windowed ? { tabIndex: 0, "aria-label": "Ranked tests, scrollable" } : {})}
            >
              <table className="table table--tests">
                <caption className="sr-only">
                  Recommended tests in run order with evidence tier, kind of evidence and recorded history
                </caption>
                <thead>
                  <tr>
                    <th scope="col" className="num">
                      #
                    </th>
                    <th scope="col">Test</th>
                    <th scope="col">Tier</th>
                    <th scope="col">Evidence</th>
                    <th scope="col" className="num">
                      Depth
                    </th>
                    <th scope="col">Reliability</th>
                    <th scope="col" className="num">
                      Median
                    </th>
                    <th scope="col">Last outcome</th>
                  </tr>
                </thead>
                <tbody>
                  {rows.padTop > 0 && <tr aria-hidden="true" style={{ height: rows.padTop }} />}
                  {tests.slice(rows.start, rows.end).map((test, offset) => (
                    <TestRow
                      key={test.id}
                      rank={rows.start + offset + 1}
                      test={test}
                      selected={selected?.id === test.id}
                      onSelect={() => {
                        navigate({ test: test.id }, { replace: true });
                      }}
                    />
                  ))}
                  {rows.padBottom > 0 && <tr aria-hidden="true" style={{ height: rows.padBottom }} />}
                </tbody>
              </table>
            </div>
          )}
        </section>

        <section aria-labelledby="evidence-title">
          <h2 id="evidence-title" className="section-title">
            Evidence used
          </h2>
          <dl className="facts facts--inline">
            <dt>Coverage reports</dt>
            <dd>
              {evidence.coverage_reports}
              {evidence.coverage_reports_other_commits > 0 &&
                ` (${evidence.coverage_reports_other_commits} measured at another commit)`}
            </dd>
            <dt>Measured TESTS edges</dt>
            <dd>{evidence.coverage_edges}</dd>
            <dt>CI runs</dt>
            <dd>{evidence.test_runs}</dd>
            <dt>Tests with history</dt>
            <dd>{evidence.tests_with_history}</dd>
          </dl>
          {evidence.coverage_reports === 0 && evidence.test_runs === 0 && (
            <p className="note">
              No coverage or CI results were ingested, so every recommendation rests on static structure alone and no
              reliability or runtime is known. See <code>ripplepath ingest</code>.
            </p>
          )}
        </section>
      </div>

      <aside className="inspector" aria-label="Test evidence">
        {selected ? <TestDetail test={selected} onSymbol={onSymbol} /> : <p className="muted">No test selected.</p>}
      </aside>
    </div>
  );
}

function TestRow({
  rank,
  test,
  selected,
  onSelect,
}: {
  rank: number;
  test: TestRecommendation;
  selected: boolean;
  onSelect: () => void;
}) {
  const history = test.history;
  const kind = evidenceKind(test);
  return (
    <tr className={selected ? "is-selected" : ""} aria-selected={selected}>
      <td className="num muted">{rank}</td>
      <td>
        <button type="button" className="link sym" onClick={onSelect} title={test.id} aria-pressed={selected}>
          {shortLabel(test.id)}
        </button>
      </td>
      <td>
        <TierBadge tier={test.tier} />
      </td>
      <td>
        <span className={`evkind evkind--${kind.replace(" ", "-")}`}>{kind}</span>
      </td>
      <td className="num">{test.reason === "CHANGED_TEST" ? "—" : test.depth}</td>
      <td>
        {history ? (
          <ReliabilityBadge reliability={history.reliability} />
        ) : (
          <span className="muted" title="No recorded CI results for this test (containers carry no history)">
            no history
          </span>
        )}
      </td>
      <td className="num">
        {history && history.median_duration_ms !== null ? formatDuration(history.median_duration_ms) : <span className="muted">—</span>}
      </td>
      <td>
        {history?.last_outcome ? (
          <span className={`outcome outcome--${history.last_outcome.toLowerCase()}`}>
            {history.last_outcome.toLowerCase()}
          </span>
        ) : (
          <span className="muted">—</span>
        )}
      </td>
    </tr>
  );
}

function TestDetail({ test, onSymbol }: { test: TestRecommendation; onSymbol: (id: string) => void }) {
  const history = test.history;
  return (
    <>
      <h2 className="panel-title">Selected test</h2>
      <p className="inspector__headline" title={test.id}>
        {shortLabel(test.id)}
      </p>
      <dl className="facts">
        <dt>Tier</dt>
        <dd>
          <TierBadge tier={test.tier} />
        </dd>
        <dt>Evidence</dt>
        <dd>{evidenceKind(test)}</dd>
        {test.reason === "STATIC_PATH" && (
          <>
            <dt>Weakest hop</dt>
            <dd>
              <EvidenceBadge evidence={test.weakest_evidence} />
            </dd>
          </>
        )}
        <dt>File</dt>
        <dd>
          <Location file={test.file} />
        </dd>
        <dt>Starts from</dt>
        <dd>
          <SymbolLink id={test.root} onSelect={onSymbol} />
        </dd>
      </dl>

      <h3 className="inspector__subtitle">Evidence path</h3>
      {test.reason === "CHANGED_TEST" ? (
        <p className="muted">The test itself, or shared code in its test class or file, changed.</p>
      ) : (
        <>
          <EvidencePath
            root={test.root}
            hops={test.path}
            onSelectSymbol={onSymbol}
            label="Evidence path from the changed symbol to the test"
          />
          <p className="hint">
            <span className="hint__swatch hint__swatch--static" aria-hidden="true" /> static hop, read from code{" "}
            <span className="hint__swatch hint__swatch--measured" aria-hidden="true" /> measured hop, recorded by
            coverage
          </p>
        </>
      )}

      <h3 className="inspector__subtitle">Recorded history</h3>
      {history ? (
        <dl className="facts">
          <dt>Reliability</dt>
          <dd>
            <ReliabilityBadge reliability={history.reliability} />
          </dd>
          <dt>Runs</dt>
          <dd>{history.runs}</dd>
          <dt>Failures</dt>
          <dd>{history.failures}</dd>
          <dt>Flaky commits</dt>
          <dd>{history.flaky_commits}</dd>
          <dt>Median duration</dt>
          <dd>{history.median_duration_ms !== null ? formatDuration(history.median_duration_ms) : "unknown"}</dd>
          <dt>Last outcome</dt>
          <dd>{history.last_outcome?.toLowerCase() ?? "unknown"}</dd>
        </dl>
      ) : (
        <p className="muted">
          No CI results recorded for this test. Reliability and duration belong to test units; containers carry none.
        </p>
      )}
    </>
  );
}
