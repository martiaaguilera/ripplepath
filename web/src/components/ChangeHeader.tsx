// The verdicts of one analysis at a glance, each linking to the view that explains it. Every value
// is copied from the report; nothing here is computed beyond counting what the report lists.

import type { MouseEvent, ReactNode } from "react";
import type { AnalysisReport } from "../api/types";
import { VIEW_LABEL, hrefFor, type Navigate, type UrlState, type View } from "../app/url";
import { LevelBadge, StatusBadge, humanize } from "./common";

interface Props {
  report: AnalysisReport;
  url: UrlState;
  navigate: Navigate;
}

function Item({
  view,
  label,
  url,
  navigate,
  tone,
  children,
  detail,
}: {
  view: View;
  label: string;
  url: UrlState;
  navigate: Navigate;
  tone?: string | undefined;
  children: ReactNode;
  detail: ReactNode;
}) {
  const onClick = (event: MouseEvent<HTMLAnchorElement>) => {
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.button !== 0) return;
    event.preventDefault();
    navigate({ view });
  };
  return (
    <a
      className={`verdict ${tone ? `verdict--${tone}` : ""} ${url.view === view ? "is-current" : ""}`}
      href={hrefFor(url, { view })}
      onClick={onClick}
      title={`Open ${VIEW_LABEL[view]}`}
    >
      <span className="verdict__label">{label}</span>
      <span className="verdict__value">{children}</span>
      <span className="verdict__detail">{detail}</span>
    </a>
  );
}

export function ChangeHeader({ report, url, navigate }: Props) {
  const { summary, risk, test_selection: selection, architecture: arch, policy } = report;
  const highUncertainty = report.uncertainty.filter((u) => u.severity === "high").length;
  const archValue = !arch.configured
    ? "not configured"
    : arch.summary.new_violations + arch.summary.new_cycles === 0
      ? "no new findings"
      : [
          arch.summary.new_violations > 0 ? `${arch.summary.new_violations} new violations` : "",
          arch.summary.new_cycles > 0 ? `${arch.summary.new_cycles} new cycles` : "",
        ]
          .filter(Boolean)
          .join(" · ");
  return (
    <section className="verdicts" aria-label="Change summary">
      <Item
        view="risk"
        label="Risk"
        url={url}
        navigate={navigate}
        tone={risk.level.toLowerCase()}
        detail={
          <>
            model v{risk.model_version} ·{" "}
            <span className="footnote" aria-describedby="risk-footnote">
              not a probability<sup aria-hidden="true">*</sup>
            </span>
          </>
        }
      >
        <span className="verdict__score">{risk.score}</span>
        <LevelBadge level={risk.level} />
      </Item>
      <Item
        view="diff"
        label="Changed"
        url={url}
        navigate={navigate}
        detail={`${summary.files_changed} files${highUncertainty > 0 ? ` · ${highUncertainty} high uncertainty` : ""}`}
        tone={highUncertainty > 0 ? "warn" : undefined}
      >
        {summary.symbols_changed} <small>symbols</small>
      </Item>
      <Item
        view="overview"
        label="Blast radius"
        url={url}
        navigate={navigate}
        detail={`${summary.modules_impacted} modules · depth ≤ ${summary.max_depth}${summary.impact_truncated ? " · truncated" : ""}`}
      >
        {summary.symbols_impacted} <small>impacted</small>
      </Item>
      <Item
        view="tests"
        label="Tests"
        url={url}
        navigate={navigate}
        tone={selection.decision === "FULL_SUITE" ? "warn" : undefined}
        detail={`${humanize(selection.mode).toLowerCase()} mode · ${selection.fallback_reasons.length} fallback reasons`}
      >
        {selection.decision === "FULL_SUITE" ? (
          "Full suite"
        ) : (
          <>
            {selection.selected_units} <small>of {selection.total_units} selected</small>
          </>
        )}
      </Item>
      <Item
        view="architecture"
        label="Architecture"
        url={url}
        navigate={navigate}
        tone={arch.summary.new_violations + arch.summary.new_cycles > 0 ? "fail" : undefined}
        detail={
          arch.configured
            ? `${arch.summary.pre_existing_violations} pre-existing · ${arch.summary.removed_violations} removed`
            : "no layers in ripplepath.yml"
        }
      >
        <span className="verdict__text">{archValue}</span>
      </Item>
      <Item
        view="risk"
        label="Policy"
        url={url}
        navigate={navigate}
        tone={policy.result.toLowerCase()}
        detail={`${policy.gates.filter((g) => g.status === "FAIL").length} failing · ${policy.gates.filter((g) => g.status === "WARN").length} warning gates`}
      >
        <StatusBadge status={policy.result} />
      </Item>
      <p id="risk-footnote" className="verdicts__footnote">
        <sup aria-hidden="true">*</sup> The risk score is a sum of capped, versioned signal points for ordering review
        attention. It is not a probability of failure and is not calibrated against outcomes.
      </p>
    </section>
  );
}
