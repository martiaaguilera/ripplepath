# Real test evidence for `java-banking`

Every file here is unedited tool output, except for the machine-identifying values listed under
"Sanitisation". Nothing was written by hand.

- Collected: 2026-10-08, on Windows 11 (Git Bash).
- Command: `scripts/collect-evidence-java.sh` (defaults: `RUNS=3`).
- Tools: Eclipse Temurin JDK 25.0.4.1 (`javac --release 21`), JUnit Platform Console Standalone
  6.1.3, JaCoCo agent and CLI 0.8.15. Jars come from Maven Central and are checked against the
  SHA-1 digests pinned in the script.

| Path | Content |
|---|---|
| `v1/`, `v2/` | Evidence measured at the matching snapshot (`main~1` and `main` of the fixture repository). |
| `*/coverage/<test class FQN>.xml` | JaCoCo XML report for **one** test class run alone under the agent (testwise coverage). Ingest with `--test <test class FQN>`. |
| `*/junit/run-N.xml` | JUnit XML (`TEST-junit-jupiter.xml`) of one whole-suite run. |

All 18 recorded test executions passed (3 runs × 3 tests × 2 snapshots); these tests are
deterministic.

JaCoCo names files relative to the package root (`com/acme/bank/domain/Money.java`), so the reports
contain no absolute path; Ripplepath maps them by unique path suffix.

## Sanitisation

- JUnit: the `<properties>` block (every JVM system property: user name, home and working
  directories) was deleted and `hostname` set to `redacted`.
- JaCoCo: `<sessioninfo id>` (host name plus a random suffix) set to `redacted`.
