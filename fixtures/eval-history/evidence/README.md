# Real test evidence for `eval-history`

Every file here is unedited tool output, except for the machine-identifying values listed under
"Sanitisation". Nothing was written by hand: no outcome, duration or coverage line.

- Collected: 2026-10-08, on Windows 11 (Git Bash 5.3.15, curl 8.21.0).
- Command: `scripts/collect-eval-history.sh` (no arguments; it rewrites `v1/` … `v12/` here).
- Tools: OpenJDK 25.0.4.1 (`javac --release 21`), JUnit Platform Console Standalone 6.1.3, JaCoCo
  agent and CLI 0.8.15. Jars come from Maven Central and are checked against the SHA-1 digests
  pinned in the script (the same pins as `scripts/collect-evidence-java.sh`).

## What the history is

`fixtures/eval-history/v1` … `v12` are twelve snapshots of a small Java shop (`com.acme.shop`),
committed in order by `build_fixture_repo` (v1 = `main~11`, v12 = `main`). The history was
**authored for this fixture**: some steps are refactorings, some introduce a regression that a later
step fixes (rounding in `Money.percent`, the bulk-discount threshold, a malformed tax rate in
`tax-rates.properties`, `Inventory.reserve` bounds, the free-shipping threshold). It is not mined
from a real project. What is real is the outcome: every pass, failure and duration below came from
actually compiling and running the tests of that snapshot.

| Snapshot | Tests | Failures | Errors |
|---|---|---|---|
| v1 | 17 | 0 | 0 |
| v2 | 17 | 0 | 0 |
| v3 | 17 | 3 | 0 |
| v4 | 20 | 0 | 0 |
| v5 | 20 | 2 | 0 |
| v6 | 20 | 0 | 0 |
| v7 | 20 | 0 | 5 |
| v8 | 20 | 0 | 0 |
| v9 | 20 | 0 | 4 |
| v10 | 20 | 0 | 0 |
| v11 | 20 | 1 | 0 |
| v12 | 22 | 0 | 0 |

## Layout

| Path | Content |
|---|---|
| `vN/coverage/<test class FQN>.xml` | JaCoCo XML report for **one** test class run alone under the agent (testwise coverage), measured at snapshot N. A class with failing tests still executed code; its coverage is kept. Ingest with `--test <test class FQN>`. |
| `vN/junit/run-1.xml` | JUnit XML (`TEST-junit-jupiter.xml`) of one whole-suite run at snapshot N, without instrumentation. Failing runs are kept, never retried: they are the observed outcome `ripplepath evaluate` scores against. |

`../cases.json` lists the eleven consecutive changes `vN → vN+1`; each case's observed outcome is
`evidence/vN+1/junit/run-1.xml`.

Durations are milliseconds from a single run on a laptop and vary between collections; re-running
the script changes them (and the runtime figures derived from them) but, on the 2026-10-08
collection, no outcome: a re-collection was compared with an earlier one and differed only in
`time`/`timestamp` attributes and JaCoCo session start/dump times.

## Sanitisation

- JUnit: the `<properties>` block (every JVM system property: user name, home and working
  directories) was deleted and `hostname` set to `redacted`.
- JaCoCo: `<sessioninfo id>` (host name plus a random suffix) set to `redacted`.
