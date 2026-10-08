import { useEffect, useRef, useState, type MouseEvent } from "react";
import { fetchAnalysis, fetchHealth } from "./api/client";
import type { AnalysisReport } from "./api/types";
import { VIEWS, VIEW_LABEL, hrefFor, useUrlState, type UrlState, type View } from "./app/url";
import { ChangeHeader } from "./components/ChangeHeader";
import { ArchitectureView } from "./views/ArchitectureView";
import { DiffView } from "./views/DiffView";
import { OverviewView } from "./views/OverviewView";
import { OwnersView } from "./views/OwnersView";
import { RiskView } from "./views/RiskView";
import { TestsView } from "./views/TestsView";

type Settled = { status: "error"; message: string } | { status: "ready"; report: AnalysisReport };
type LoadState = { status: "idle" } | { status: "loading" } | Settled;

function revisionKey(revisions: { base: string; head: string }): string {
  return `${revisions.base} ${revisions.head}`;
}

function isTyping(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName))
  );
}

export function App() {
  const [url, navigate] = useUrlState();
  const [draft, setDraft] = useState({ base: url.base, head: url.head });
  // The result is tagged with the revisions it belongs to; "loading" is derived from a mismatch,
  // so no effect has to set a transient state synchronously.
  const [result, setResult] = useState<{ key: string; state: Settled } | null>(null);
  const [healthError, setHealthError] = useState<string | null>(null);
  const revisions = { base: url.base, head: url.head };
  const key = revisionKey(revisions);

  // Fill in the server's default revisions when the URL does not name any.
  useEffect(() => {
    if (url.base && url.head) return;
    const controller = new AbortController();
    fetchHealth(controller.signal)
      .then((health) => {
        const next = { base: url.base || health.default_base, head: url.head || health.default_head };
        navigate(next, { replace: true });
        setDraft(next);
      })
      .catch((error: unknown) => {
        if (!controller.signal.aborted) {
          setHealthError(`Cannot reach the Ripplepath server: ${String(error)}`);
        }
      });
    return () => {
      controller.abort();
    };
  }, [url.base, url.head, navigate]);

  useEffect(() => {
    if (!url.base || !url.head) return;
    const controller = new AbortController();
    const requested = revisionKey({ base: url.base, head: url.head });
    fetchAnalysis(url.base, url.head, controller.signal)
      .then((report) => {
        setResult({ key: requested, state: { status: "ready", report } });
      })
      .catch((error: unknown) => {
        if (!controller.signal.aborted) {
          setResult({
            key: requested,
            state: { status: "error", message: error instanceof Error ? error.message : String(error) },
          });
        }
      });
    return () => {
      controller.abort();
    };
  }, [url.base, url.head]);

  // Keyboard: 1–6 switch views, "/" finds a symbol in the graph. Ignored while typing.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.metaKey || event.ctrlKey || event.altKey || isTyping(event.target)) return;
      const index = Number(event.key) - 1;
      const view = VIEWS[index];
      if (view && /^[1-9]$/.test(event.key)) {
        event.preventDefault();
        navigate({ view });
        return;
      }
      if (event.key === "/") {
        const search = document.getElementById("graph-search");
        if (search) {
          event.preventDefault();
          search.focus();
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [navigate]);

  // Move focus to the new view's heading so keyboard and screen-reader users land in the content,
  // as they would after a page load. Skipped on first render: the page load already does that.
  const previousView = useRef<View | null>(null);
  useEffect(() => {
    if (previousView.current !== null && previousView.current !== url.view) {
      document.querySelector<HTMLElement>("main .view-title")?.focus();
    }
    previousView.current = url.view;
  }, [url.view, result]);

  const state: LoadState =
    !url.base || !url.head
      ? healthError
        ? { status: "error", message: healthError }
        : { status: "idle" }
      : result?.key === key
        ? result.state
        : { status: "loading" };
  const report = state.status === "ready" ? state.report : null;

  return (
    <div className="app">
      <a className="skip-link" href="#content">
        Skip to content
      </a>
      <header className="topbar">
        <div className="brand">
          <img src="/favicon.svg" alt="" width={20} height={20} />
          <span>Ripplepath</span>
        </div>
        <form
          className="revisions"
          aria-label="Revisions to compare"
          onSubmit={(event) => {
            event.preventDefault();
            const next = { base: draft.base.trim(), head: draft.head.trim() };
            navigate({ ...next, sel: null, file: null, test: null, line: null });
          }}
        >
          <label>
            base
            <input
              value={draft.base}
              onChange={(e) => {
                setDraft({ ...draft, base: e.target.value });
              }}
              spellCheck={false}
              aria-label="Base revision"
            />
          </label>
          <span aria-hidden="true">→</span>
          <label>
            head
            <input
              value={draft.head}
              onChange={(e) => {
                setDraft({ ...draft, head: e.target.value });
              }}
              spellCheck={false}
              aria-label="Head revision"
            />
          </label>
          <button type="submit" className="button">
            Analyze
          </button>
        </form>
        {report && (
          <span className="topbar__commits" title="Resolved commits">
            <code>{(report.base.commit ?? report.base.tree).slice(0, 10)}</code>
            <span aria-hidden="true"> → </span>
            <span className="sr-only"> to </span>
            <code>{(report.head.commit ?? report.head.tree).slice(0, 10)}</code>
            <span className="muted"> · v{report.tool_version}</span>
          </span>
        )}
      </header>

      {state.status === "loading" && (
        <p className="status" role="status">
          Analyzing {url.base} → {url.head}…
        </p>
      )}
      {state.status === "error" && (
        <p className="status status--error" role="alert">
          {state.message}
        </p>
      )}

      {report && (
        <>
          <ChangeHeader report={report} url={url} navigate={navigate} />
          <ViewNav url={url} navigate={navigate} report={report} />
          <main id="content" className={`content content--${url.view}`}>
            <CurrentView key={key} report={report} url={url} navigate={navigate} />
          </main>
        </>
      )}
    </div>
  );
}

function CurrentView({ report, url, navigate }: { report: AnalysisReport; url: UrlState; navigate: ReturnType<typeof useUrlState>[1] }) {
  switch (url.view) {
    case "overview":
      return <OverviewView report={report} url={url} navigate={navigate} />;
    case "diff":
      return <DiffView report={report} url={url} navigate={navigate} />;
    case "tests":
      return <TestsView report={report} url={url} navigate={navigate} />;
    case "risk":
      return <RiskView report={report} navigate={navigate} />;
    case "architecture":
      return <ArchitectureView report={report} navigate={navigate} />;
    case "owners":
      return <OwnersView report={report} navigate={navigate} />;
  }
}

function ViewNav({
  url,
  navigate,
  report,
}: {
  url: UrlState;
  navigate: ReturnType<typeof useUrlState>[1];
  report: AnalysisReport;
}) {
  const counts: Record<View, string | null> = {
    overview: String(report.summary.symbols_impacted),
    diff: String(report.files.length),
    tests: String(report.tests.length),
    risk: String(report.risk.score),
    architecture: report.architecture.configured
      ? String(report.architecture.summary.new_violations + report.architecture.summary.new_cycles)
      : null,
    owners: report.owners.source === null ? null : String(report.owners.owners.length),
  };
  return (
    <nav className="viewnav" aria-label="Views">
      <ul>
        {VIEWS.map((view, index) => {
          const onClick = (event: MouseEvent<HTMLAnchorElement>) => {
            if (event.metaKey || event.ctrlKey || event.shiftKey || event.button !== 0) return;
            event.preventDefault();
            navigate({ view });
          };
          return (
            <li key={view}>
              <a
                href={hrefFor(url, { view })}
                aria-current={url.view === view ? "page" : undefined}
                aria-keyshortcuts={String(index + 1)}
                title={`${VIEW_LABEL[view]} (press ${index + 1})`}
                className={`viewnav__link ${url.view === view ? "is-current" : ""}`}
                onClick={onClick}
              >
                {VIEW_LABEL[view]}
                {counts[view] !== null && <span className="viewnav__count">{counts[view]}</span>}
              </a>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
