# Portfolio material

Ready-to-paste text about Ripplepath for a CV, LinkedIn and GitHub. Every number is measured and
traceable to the linked document. Do not add adoption, users or stars: there are none to report.

## Project title

**Ripplepath: change intelligence for Git repositories**

## One-line GitHub description

> See what a code change can break before you merge it: symbol-level blast radius, evidence-backed
> test selection and architecture drift for Java and TypeScript. Local, deterministic, Rust + React.

## CV description (2–3 lines)

> **Ripplepath** (Rust, TypeScript/React). A local change-intelligence tool. It maps a Git diff to
> changed symbols, traverses a typed, evidence-carrying dependency graph to explain blast radius,
> recommends tests from static paths and ingested coverage/CI evidence, and reports architecture
> drift and a versioned risk decomposition. It ships as a CLI, a web UI, a GitHub Action (SARIF, PR
> comment) and an MCP server, and it never executes the analysed repository.

## Three factual bullets

- Built a Rust analysis engine (10 crates, tree-sitter Java and TypeScript frontends, `gix` object
  access instead of the git CLI) with **byte-identical deterministic output**. Its incremental
  SQLite index is property-tested to equal a clean re-index over random edit histories, and the
  test was mutation-checked.
- Tried it on my own Spring Boot/React project, QuantaRun. That cut unresolved references
  **from 2,529 to 120**: records, `var`, JDK collection generics and lambdas in Java, and
  object-literal members in TypeScript. It also removed spurious full-suite fallbacks caused by a
  tree-sitter grammar gap.
  ([DOGFOODING_QUANTARUN.md](DOGFOODING_QUANTARUN.md))
- Designed test selection that **widens when evidence is weak**, plus a leakage-controlled offline
  replay evaluation. On an 11-case history, the evaluation exposed a real gap: balanced mode caught
  10/15 failing tests, and every miss came from one runtime resource-file change. Reported with an
  explicit small-sample label. Backed by 220 Rust and 40 web unit tests plus a Playwright end-to-end
  smoke in CI. ([TEST_INTELLIGENCE.md](TEST_INTELLIGENCE.md))

Alternative bullet (security angle):

- Hardened it for untrusted repositories. Security reviews found stack overflows from crafted
  nesting, exponential `export *` resolution (old resolver over 120 s, new one 10 ms), DNS
  rebinding against the local server, and terminal/workflow-command injection. Each fix has a
  regression test that reproduced the bug first.

## LinkedIn project description

> **Ripplepath: see what a code change can break before you merge it**
>
> Ripplepath reads two Git revisions and works out which methods and classes changed. It then
> follows a typed dependency graph to everything that depends on them. Every step comes with a file,
> a line and an evidence class, so each claim can be checked. It recommends tests from static paths
> and from coverage and CI results your pipeline already produces, and it falls back to running
> more tests when the evidence is weak. It flags new architecture-rule violations and breaks review
> risk into named, versioned signals. The score is deliberately not presented as a probability.
>
> It is a Rust core (tree-sitter, gix, SQLite) with a React + TypeScript UI, a GitHub Action
> (step summary, SARIF, PR comment) and a read-only MCP server for coding agents. It runs locally
> with no API keys and never executes code from the repository it analyses.
>
> I tried it on my own project, QuantaRun, which reduced unresolved references from 2,529 to 120
> and documented where it is still wrong. An offline replay evaluation with leakage controls
> measures test-selection recall and labels small samples as such.
>
> Built with AI-assisted implementation. Design decisions, evaluation and dogfooding are documented
> in the repository's ADRs and engineering log.

## Technologies

Rust (2024 edition) · tree-sitter · gix · SQLite (rusqlite) · rayon · Axum/Tokio · BLAKE3 ·
React 19 · TypeScript (strict) · Vite · React Flow + ELK · Vitest · Playwright · GitHub Actions ·
SARIF · Model Context Protocol · JaCoCo / LCOV / JUnit XML

## Interview talking points

Details for each in [INTERVIEW_GUIDE.md](INTERVIEW_GUIDE.md).

1. **Why symbols, not files:** fingerprints over syntax tokens. Comment-only edits are not changes,
   and editing a method does not mark its class.
2. **Evidence on every edge** (`RESOLVED_EXACT` vs `STATIC_INFERRED` vs `COVERAGE_OBSERVED`), and
   why the tool widens instead of guessing.
3. **Security as a design input:** gix instead of the git CLI, configuration read from the base
   revision, DNS rebinding, a semaphore permit moved into the blocking task.
4. **Correctness invariants with teeth:** incremental == clean, the leakage test, mutation checks.
5. **Honest measurement:** dogfooding numbers, a small-sample evaluation with its caveat, and a risk
   score that is explicitly not a probability.

## The QuantaRun + Ripplepath story

The two projects cover different halves of backend and platform engineering, and one was used to
test the other.

| | QuantaRun | Ripplepath |
|---|---|---|
| Problem | Running distributed AI workloads reliably | Knowing what a code change can break |
| Core skills | Distributed coordination: leases, fencing, scheduling, failure recovery; observability with OpenTelemetry | Program analysis: parsing, name resolution, graph algorithms; incremental computation and caching |
| Stack | Java / Spring Boot, PostgreSQL, React | Rust, SQLite, React + TypeScript |
| Correctness focus | Behaviour under failures and concurrency | Determinism, incremental == clean, no execution of untrusted input |
| Delivery | Service with a web console | CLI, local UI, GitHub Action, MCP server |

**How to tell it:**

> "My first project, QuantaRun, is a distributed control plane in Java and Spring: leases, fencing
> and failure recovery. My second, Ripplepath, is developer tooling in Rust: parsing, dependency
> graphs and deterministic analysis. Then I pointed the second at the first. QuantaRun's real Spring
> and React code exposed more than 2,000 unresolved references that my own test fixtures had
> hidden. Fixing them made the tool far more accurate, and the cases I could not fix, like Spring
> HTTP routes, are documented as limitations. Together they show I can build the system *and* the
> tooling around it, and that I test my tools against real code."
