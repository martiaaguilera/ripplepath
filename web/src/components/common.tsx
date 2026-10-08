// Small presentational pieces shared by every view. They only map report values to labels and
// styles; none of them decides anything the report has not already decided.

import type { ReactNode } from "react";
import type {
  CoverageStatus,
  DeltaStatus,
  Evidence,
  EvidenceTier,
  GateResult,
  Reliability,
  RiskReport,
  Severity,
} from "../api/types";
import { EVIDENCE_LABEL, shortLabel } from "../graph/model";

/** `MIGRATION_CHANGED` → `Migration changed`. */
export function humanize(code: string): string {
  const words = code.replaceAll("_", " ").toLowerCase();
  return words.charAt(0).toUpperCase() + words.slice(1);
}

export function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(ms < 10_000 ? 2 : 1)} s`;
  const minutes = Math.floor(ms / 60_000);
  return `${minutes} min ${Math.round((ms % 60_000) / 1000)} s`;
}

export function EvidenceBadge({ evidence, prefix }: { evidence: Evidence; prefix?: string }) {
  return (
    <span className={`evidence evidence--${evidence.toLowerCase()}`} title={EVIDENCE_LABEL[evidence]}>
      {prefix}
      {EVIDENCE_LABEL[evidence].toLowerCase()}
    </span>
  );
}

const TIER_HINT: Record<EvidenceTier, string> = {
  STRONG: "The test changed, or coverage measured it executing the change's dependency path",
  MEDIUM: "A static path of exactly resolved edges (or coverage with an inferred static hop)",
  WEAK: "A static path that includes inferred edges",
};

export function TierBadge({ tier }: { tier: EvidenceTier }) {
  return (
    <span className={`tier tier--${tier.toLowerCase()}`} title={TIER_HINT[tier]}>
      {tier.toLowerCase()}
    </span>
  );
}

const COVERAGE_LABEL: Record<CoverageStatus, string> = {
  COVERED: "covered",
  NOT_COVERED: "not covered",
  NO_DATA: "no coverage data",
};

const COVERAGE_HINT: Record<CoverageStatus, string> = {
  COVERED: "Some ingested test run executed this symbol",
  NOT_COVERED: "Its file was measured, but no recorded run executed it",
  NO_DATA: "No ingested coverage report contains its file",
};

export function CoverageBadge({ status }: { status: CoverageStatus | null }) {
  if (status === null) return null;
  return (
    <span className={`cov cov--${status.toLowerCase()}`} title={COVERAGE_HINT[status]}>
      {COVERAGE_LABEL[status]}
    </span>
  );
}

const RELIABILITY_HINT: Record<Reliability, string> = {
  STABLE: "No disagreement between runs at the same commit, and the latest commit's runs passed",
  FLAKY: "Passed and failed at the same commit at least once",
  CONSISTENTLY_FAILING: "Every run at the most recent recorded commit failed",
  INSUFFICIENT_DATA: "Fewer than 3 recorded runs and no same-commit flip",
};

export function ReliabilityBadge({ reliability }: { reliability: Reliability }) {
  return (
    <span className={`rel rel--${reliability.toLowerCase()}`} title={RELIABILITY_HINT[reliability]}>
      {humanize(reliability).toLowerCase()}
    </span>
  );
}

export function SeverityBadge({ severity }: { severity: Severity }) {
  return <span className={`severity severity--${severity}`}>{severity}</span>;
}

const DELTA_LABEL: Record<DeltaStatus, string> = { NEW: "new", PRE_EXISTING: "pre-existing", REMOVED: "removed" };

export function DeltaBadge({ status }: { status: DeltaStatus }) {
  return <span className={`delta delta--${status.toLowerCase()}`}>{DELTA_LABEL[status]}</span>;
}

export function LevelBadge({ level }: { level: RiskReport["level"] }) {
  return <span className={`level level--${level.toLowerCase()}`}>{level.toLowerCase()}</span>;
}

export function StatusBadge({ status }: { status: GateResult["status"] | "PASS" | "WARN" | "FAIL" }) {
  return <span className={`gate gate--${status.toLowerCase()}`}>{humanize(status).toLowerCase()}</span>;
}

export function Location({ file, line }: { file: string; line?: number | null }) {
  return (
    <code className="loc" title={line ? `${file}:${line}` : file}>
      {file}
      {line ? `:${line}` : ""}
    </code>
  );
}

export function SymbolLink({ id, onSelect }: { id: string; onSelect?: ((id: string) => void) | undefined }) {
  if (!onSelect) {
    return (
      <span className="sym" title={id}>
        {shortLabel(id)}
      </span>
    );
  }
  return (
    <button type="button" className="link sym" title={id} onClick={() => onSelect(id)}>
      {shortLabel(id)}
    </button>
  );
}

export function Empty({ children }: { children: ReactNode }) {
  return <p className="empty">{children}</p>;
}

/** Splits `file:line` written by the engine into its parts. */
function fileAndLine(token: string): { file: string; line: number | null } {
  const match = /^(.*?):(\d+)$/.exec(token);
  if (match?.[1] && match[2]) return { file: match[1], line: Number(match[2]) };
  return { file: token, line: null };
}

/**
 * Evidence strings are prose written by the engine ("Removed java:a.B#c() (src/B.java:18)"). Tokens
 * that name a symbol or a changed file the report knows about become links; everything else stays
 * plain text. Nothing is inferred: unknown tokens are never turned into links.
 */
export function EvidenceText({
  text,
  symbols,
  files,
  onSymbol,
  onFile,
}: {
  text: string;
  symbols: ReadonlySet<string>;
  files: ReadonlySet<string>;
  onSymbol?: ((id: string) => void) | undefined;
  onFile?: ((file: string, line: number | null) => void) | undefined;
}) {
  const parts = text.split(/(\s+)/);
  return (
    <>
      {parts.map((part, index) => {
        const key = `${index}:${part}`;
        const bare = part.replace(/^[([]+|[)\],;]+$/g, "");
        const lead = part.slice(0, part.indexOf(bare));
        const trail = part.slice(part.indexOf(bare) + bare.length);
        if (bare && symbols.has(bare) && onSymbol) {
          return (
            <span key={key}>
              {lead}
              <button type="button" className="link sym" title={bare} onClick={() => onSymbol(bare)}>
                {shortLabel(bare)}
              </button>
              {trail}
            </span>
          );
        }
        const { file, line } = fileAndLine(bare);
        if (bare && files.has(file) && onFile) {
          return (
            <span key={key}>
              {lead}
              <button type="button" className="link loc" onClick={() => onFile(file, line)}>
                {bare}
              </button>
              {trail}
            </span>
          );
        }
        return <span key={key}>{part}</span>;
      })}
    </>
  );
}
