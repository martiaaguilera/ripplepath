# Research — positioning Ripplepath

Date: 2026-10-07. Scope: what already exists around "what does this change break / what should I test",
what is commoditized, and the exact gap Ripplepath targets. Concepts only; no code, UI or text was copied.

## 1. Name validation

The working name was **ImpactTrace**. It was rejected:

| Candidate | Finding | Verdict |
|---|---|---|
| ImpactTrace | Active commercial software product (impacttrace.io, ESG/climate data platform); several GitHub repos with the same name; Micro Focus COBOL Analyzer has an "Impact Trace" feature | Rejected: real, established software product with the exact name |
| Blastline | npm package `blastline` (MIT) is a *graph-backed test impact / blast radius for CI* tool — same category | Rejected: direct same-domain collision |
| **Ripplepath** | No crate, no npm package, no GitHub repository with this name; no software product found | **Chosen** |

"Ripple" + "path": impact ripples outward through dependency *paths*, and every claim Ripplepath makes
is a path with evidence.

## 2. Landscape (concepts, not marketing)

### A. Code-intelligence graphs for AI agents (heavily commoditized in 2025–2026)
- **Code System Graph** (Rust): cross-repository evidence-backed graph of APIs, events, schemas, packages,
  owners; MCP; a `changes` command recommends tests for staged work; explicitly reports unknowns.
- **Codeknow** (Python, NetworkX): tree-sitter knowledge graph for 25+ languages, blast radius, `test-impact`
  mapping changed *files* to tests, architecture "health grade", drift baselines, MCP.
- **code-review-graph** (Python, SQLite): blast radius for AI review context, "risk-scored" functions,
  sticky PR comment GitHub Action, MCP, 30+ languages.
- **CodeGraph**, **codebase-memory-mcp**: semantic graphs exposed via dozens of MCP tools, 38–66 languages.

Commoditized here: tree-sitter parsing of many languages, "blast radius" as reverse reachability,
MCP exposure, sticky PR comments, generic "risk" numbers, architecture "health scores".

### B. Test impact analysis / selective testing
- **Teamscale TIA**: testwise coverage (per-test coverage recorded by profilers) to select and prioritize.
- **Launchable / CloudBees Smart Tests**, **Gradle Develocity Predictive Test Selection**:
  ML models trained on historical results; SaaS.
- **Azure Pipelines TIA**: coverage-based for .NET.
- Research: Ekstazi (Gligoric et al., ISSTA 2015, file-level dynamic dependencies), STARTS (Legunsen et al.,
  static class-level RTS), Facebook Predictive Test Selection (Machalica et al., ICSE-SEIP 2019).

Commoditized: "run only affected tests" at file/module granularity; ML ranking as a hosted service.

### C. Flaky tests / CI observability
ReportPortal, Datadog CI Visibility, BuildPulse, Trunk: ingest JUnit, classify flakiness, dashboards.
Research consensus (Luo et al., FSE 2014; iDFlakies): the strong signal is *different outcomes for the same
code*, not "fails often".

### D. Build systems
Bazel/Buck2/Pants/Nx/Turborepo compute affected targets exactly from the declared build graph —
perfectly, but only at *target* granularity and only if you adopt the build system.

### E. GitHub-native output
SARIF 2.1.0 upload (code scanning, free for public repos, needs `security-events: write`),
job summaries via `GITHUB_STEP_SUMMARY`, workflow annotations (`::error file=..,line=..::`), artifacts.

## 3. What would make Ripplepath look like a clone
- Claiming many languages through shallow tree-sitter queries.
- A 0–100 "health" or "risk" number without a decomposition.
- "Chat with your repo" / embeddings / MCP as the core.
- File-level "changed file → tests that import it" selection.
- A force-directed hairball of the entire repository.

## 4. The exact gap

Existing tools each hold one piece:
graph tools know *structure* but treat test association as static file reachability;
TIA products know *coverage/history* but are SaaS, ML-opaque, and do not explain structural paths;
architecture checkers (ArchUnit, dependency-cruiser) validate *state*, not *delta*.

Ripplepath joins them **per change, at symbol granularity, with evidence on every edge**:

1. Git diff → *symbol-level* change set (fingerprint of normalized tokens, not "file touched"),
   including signature changes, deletions and body-identical moves.
2. Reverse reachability with **typed edges and evidence classes**
   (`RESOLVED_EXACT`, `STATIC_INFERRED`, `COVERAGE_OBSERVED`, `HISTORY_COCHANGE`, `NAMING_HEURISTIC`)
   and the *explaining path* for each impacted symbol.
3. Test recommendations that cite *which* evidence connects test → impacted code
   (static path vs. measured JaCoCo/LCOV coverage vs. history) and **broaden** when evidence is poor.
4. Architecture rules evaluated on **base and head**, reporting only the *delta* by default.
5. A risk *decomposition* (versioned, capped, weighted signals) that is explicitly not a probability.
6. An offline **replay evaluation** so any selection claim is measured, not asserted.

## 5. What can be proven without ML/LLMs
- Which symbols changed (deterministic fingerprints).
- Which symbols statically depend on them, by which path and which resolution rule.
- Which tests were *observed* (coverage) to execute impacted code.
- Which layer rules a change newly violates.
- Same-SHA outcome flips (flakiness) from recorded CI history.
- Measured recall / reduction of a selection policy on replayed history.

## 6. Deliberately excluded
- Executing the analyzed repository (builds, tests, scripts) by default.
- ML risk models or ML test ranking (no dataset to justify them; see §7).
- Chat / RAG / embeddings.
- More languages before Java and TypeScript are deep.
- Hosted service, accounts, telemetry.

## 7. Possible future AI features (optional, never source of truth)
- Read-only MCP server exposing the same deterministic queries to coding agents.
- Optional local (Ollama) natural-language *summary* of structured findings, visually labeled
  "AI summary", never allowed to add edges, tests, owners or signals.
- ML ranking only with a real temporal-split dataset and a deterministic baseline to beat.

## 8. Competitor matrix (concepts)

| Concept | Graph tools (CSG, Codeknow, crg) | TIA SaaS (Teamscale, Launchable) | Build systems (Bazel, Nx) | Arch checkers | **Ripplepath** |
|---|---|---|---|---|---|
| Change → symbol mapping | partial / file | file or method (coverage) | target | — | symbol fingerprint |
| Evidence class per edge | some (CSG) | — | exact (declared) | — | yes, 5 classes |
| Explaining path per impact | rare | — | — | — | yes |
| Measured coverage evidence | rare | yes | — | — | JaCoCo, LCOV |
| Fallback-to-broad on uncertainty | rare | partial | n/a | — | explicit rules |
| Architecture *delta* base→head | baseline drift (Codeknow) | — | — | state only | yes |
| Risk as decomposition, versioned | numeric scores | — | — | — | yes, not a probability |
| Offline replay evaluation | — | vendor claims | — | — | yes |
| Runs locally, no keys | yes | no | yes | yes | yes |

## Sources
- https://github.com/dertin/code-system-graph
- https://github.com/asalsali/codeknow
- https://github.com/tirth8205/code-review-graph
- https://github.com/codegraph-ai/CodeGraph
- https://arxiv.org/html/2603.27277v1 (Codebase-Memory)
- https://www.cloudbees.com/blog/predictive-test-selection-vs-test-impact-analysis
- https://teamscale.com (TIA thesis, Ben Slimane 2023)
- https://alternativeto.net/software/impacttrace/about
- https://registry.npmjs.org/blastline
