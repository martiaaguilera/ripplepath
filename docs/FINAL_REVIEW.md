# Final adversarial review

Date: 2026-10-08/09. Scope: the whole product as of the v0.1.0 candidate — CLI, index, analysis,
test selection, evidence ingestion, demo, local server, GitHub Action. Method: try to make each
component give a wrong answer or do something unsafe with hostile repositories, hostile inputs and
unusual histories, then reproduce every finding as a failing test before fixing it.

Severity is about consequences for a user who trusts the output: **high** = wrong answer that looks
right, or code/data from an untrusted checkout reaching a trusted context; **medium** = unbounded
resource use or a misleading status; **low** = cosmetic or documentation.

## Findings

| # | Severity | Finding | Fix | Regression test |
|---|---|---|---|---|
| 1 | high | **Uncertainty shrank the test selection to nothing.** A change touching only a source file in a language Ripplepath does not analyse (Kotlin, Vue, Python…) changed no symbol, so even conservative mode decided `SELECTED` with zero tests and no fallback reason. | Such files raise the fallback `UNSUPPORTED_FILE_CHANGED` (medium), which widens conservative and balanced selection; non-code files (docs, images) stay low. | `crates/engine/tests/selection_fallbacks.rs` |
| 2 | high | **Incremental ≠ clean after a timeout.** A parse timeout on a loaded machine was stored in the fact cache and reused by every later run, so the incremental index diverged from a clean re-index — the project's mandatory invariant. | Extraction failures (timeouts, grammar errors) are never persisted; failures stored by older versions are treated as cache misses. | `crates/engine/tests/incremental_adversarial.rs` (also covers case-only renames, file↔symlink mode flips, duplicate FQNs moving the canonical file, deletions, walking history backwards) |
| 3 | high | **Java overloads inherited from a supertype were ignored.** `c.put("x")` on a `Child` declaring `put(int)` produced a `RESOLVED_EXACT` edge to `Child#put(int)` and no edge to the inherited `Base#put(String)` it really calls — a confident, wrong edge. | Candidates are every same-name, compatible-arity method of the hierarchy (nearest declaration of each signature); more than one makes the call `STATIC_INFERRED` to each. | `crates/lang/tests/java_adversarial.rs` |
| 4 | high | **`ripplepath demo` trusted the current directory.** It looked for `fixtures/` in the working directory first, so running it inside an untrusted checkout built the demo from that checkout's files, followed symlinks while copying, and ran `git` with the user's global config (filters, fsmonitor). | Fixtures are embedded at build time; snapshot directories are read without following symlinks and every path is validated; the fixture builder's `git` runs with no global/system config, fsmonitor off and a non-existent hooks directory. | `crates/cli/tests/demo_cli.rs` |
| 5 | high | **`ripplepath serve` could serve an untrusted checkout's scripts as the UI.** The default `--web-dir` was `web/dist` relative to the current directory, so a repository with its own `web/dist` had its JavaScript served on the API's origin. | The default is the build's own `web/dist`, resolved at compile time; a different directory requires `--web-dir`. | none automated (default path is a compile-time constant; review) |
| 6 | medium | **Usage errors looked like a policy verdict.** clap exits 2 on a usage error — the status the Action reads as "merge policy FAIL". A misspelled `mode` input either failed the PR as if by policy or passed silently with `fail-on-policy: false`, without any analysis running. | Usage errors exit 1; the Action also refuses status 2 when no `analysis.json` was written. | `crates/cli/tests/exit_status_cli.rs` |
| 7 | medium | **Evidence reads were bounded by reported size only.** A FIFO or device reports size 0, so `evaluate` read its manifest without limit (`ingest` was already bounded). | The read itself is bounded (`take(limit + 1)`), independent of metadata. | `crates/engine/src/evaluation.rs` (review) |

All seven are fixed on `main`. No critical or high finding is open.

## Checked and found sound

- Path validation in trees (traversal, `.git` in any case, control characters, drive prefixes).
- XML ingestion: entity declarations refused, element count and depth bounded.
- Server: `Host` validation against DNS rebinding, CSP, bounded `/api/v1/file`, concurrency permit
  held inside the blocking task.
- MCP: no paths, commands or URLs accepted; oversize lines dropped without buffering.
- Determinism: report bytes identical across repeated runs and across incremental vs clean indexes
  (now including the adversarial histories above).

## Known limitations (documented, not defects)

- Name resolution is static and syntactic; reflection, DI containers and dynamic dispatch through
  untyped values produce `UNRESOLVED_REFERENCE` uncertainty rather than edges
  (docs/LANGUAGE_SUPPORT.md).
- The offline evaluation's 15/15 conservative recall comes from the project's own fixtures, which
  were written alongside the tool; it is a regression check, not evidence of recall on other code
  (docs/TEST_INTELLIGENCE.md).
- The risk score is a decomposition of signals whose weights are not calibrated against outcomes
  (docs/RISK_MODEL.md).
- The local server has no authentication; it binds to loopback by default.
