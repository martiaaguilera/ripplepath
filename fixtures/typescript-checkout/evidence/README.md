# Real test evidence for `typescript-checkout`

Every file here is unedited tool output, except for the machine-identifying values listed under
"Sanitisation". Nothing was written by hand.

- Collected: 2026-10-08, on Windows 11 (Git Bash).
- Command: `scripts/collect-evidence-ts.sh` (defaults: `RUNS_V1=8`, `RUNS_V2=4`).
- Tools: Node.js 24.21.0, npm 11.19.0, Vitest 5.0.3 with `@vitest/coverage-v8` 5.0.3 (V8 coverage,
  `lcovonly` reporter; JUnit reporter).

| Path | Content |
|---|---|
| `v1/`, `v2/` | Evidence measured at the matching snapshot (`main~1` and `main` of the fixture repository). |
| `*/coverage/<test file>.lcov` | LCOV tracefile of **one** test file run alone (testwise coverage). Ingest with `--test <test file>`. Untested source files are listed with zero hits (`coverage.include=src/**`). |
| `*/junit/run-N.xml` | JUnit XML of one whole-suite run. |

`src/clock.test.ts` is a deliberately nondeterministic flakiness fixture (it fails when a random
"latency" exceeds 65 of 100). Recorded outcome of its runs:

| Snapshot | Runs | Failed | Runs that failed |
|---|---|---|---|
| v1 | 8 | 4 | 1, 4, 5, 6 |
| v2 | 4 | 0 | — |

No other test failed in any run.

Vitest writes paths relative to the project root; on Windows the LCOV `SF:` records use
backslashes (`SF:src\cart.ts`), which Ripplepath normalises.

## Sanitisation

- JUnit: `hostname` set to `redacted`. Absolute paths of the temporary work directory in failure
  traces would be replaced with `<work>`; this run's traces contained only relative paths.
