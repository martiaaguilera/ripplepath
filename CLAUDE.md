# Ripplepath — rules for future sessions

Change intelligence for Git repositories: symbol-level change → typed dependency graph → blast radius →
test evidence → architecture delta → deterministic risk decomposition. Owner: Martí Aguilera.

## Non-negotiables
- Deterministic core: same inputs ⇒ byte-identical output. Sort before serializing; never rely on
  HashMap iteration order in anything that reaches output.
- Zero paid dependency. Everything runs locally with no API keys.
- Never execute code from the analyzed repository by default (no builds, tests, scripts, git hooks,
  git filters). Git is read through `gix` object access, not the `git` CLI.
- Every edge carries evidence (kind, evidence class, file, span, rule). Heuristic ≠ certain.
- Uncertainty broadens recommendations; it is never hidden.
- Risk score is a decomposition of versioned signals, NOT a probability. No fake probabilities.
- No invented benchmark numbers, users, or results. Measured or absent.
- No AI-generated dependency facts. Optional AI may only summarize structured findings.
- Mandatory invariant: incremental index output == clean full re-index output.
- Unsupported constructs are documented in docs/LANGUAGE_SUPPORT.md, not silently skipped.

## Code
- Comments explain WHY, never WHAT. No boilerplate docs on trivial private fns.
- Typed errors (`thiserror`) per crate; no `unwrap()`/`expect()` on production paths without an
  invariant comment. No `unsafe`.
- Pure functions for graph/risk/ranking algorithms; thin adapters around I/O.
- Bounded everything: file size, depth, response size, concurrency.
- TypeScript stays `strict`. Frontend never re-implements business rules.

## Workflow
- Tests before declaring anything done: `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`,
  `cargo fmt --check`, and in `web/`: `npm run typecheck && npm run lint && npm test` (re-run them
  after adding *any* file under web/: config files are typechecked too).
- Update docs/SPEC.md, docs/ARCHITECTURE.md, docs/ENGINEERING_LOG.md when semantics/design change.
- Conventional Commits. Real coherent commits only.
- On Windows the cargo bin dir may not be on PATH in fresh shells: prepend `$env:USERPROFILE\.cargo\bin`.
