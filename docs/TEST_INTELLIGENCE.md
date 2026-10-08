# Test intelligence

What Ripplepath does with test evidence today. Only implemented behaviour is described; anything
not listed here is not done.

## 1. Ripplepath never runs tests

Ripplepath reads evidence files that *your* CI already produced. It does not build, run or
instrument anything in an analysed repository. The collection scripts in `scripts/` run only
Ripplepath's own fixtures, to produce the committed real evidence the integration tests use
(`fixtures/*/evidence/README.md` records tools, versions and commands).

## 2. Evidence sources

| Source | Format | Command | What is stored |
|---|---|---|---|
| Static analysis | the code graph | always | `CALLS`, `REFERENCES`, `OVERRIDES`, … edges with evidence classes |
| Coverage | JaCoCo XML, LCOV | `ripplepath ingest coverage FILE --format jacoco\|lcov --rev R [--test T]` | per report: commit, test (or aggregate), measured files, covered symbol ids |
| CI results | JUnit XML | `ripplepath ingest junit FILE --rev R` | per run: commit; per test: outcome, duration, failed attempts, failure fingerprint |

Inputs are bounded (64 MiB per file); XML entity declarations are refused. Evidence lives in the
same SQLite database as the index (`--db`, by default in the user cache directory, never inside
the analysed repository).

### Evidence classes and tiers

Edges carry an ordinal evidence class (`RESOLVED_EXACT` > `COVERAGE_OBSERVED` > `STATIC_INFERRED`
> `HISTORY_COCHANGE` > `NAMING_HEURISTIC`). These are labels, not probabilities.

Each recommended test gets a **tier** from its explaining path:

| Tier | Rule |
|---|---|
| `STRONG` | the test itself changed; or the path contains a measured coverage hop and every static hop is `RESOLVED_EXACT` |
| `MEDIUM` | a coverage hop with an inferred static hop; or an all-`RESOLVED_EXACT` static path |
| `WEAK` | a static path with at least one inferred edge |

`coverage_observed` is true when a hop of the path is `TESTS` with `COVERAGE_OBSERVED`.

## 3. How coverage maps to symbols

Coverage is mapped onto the symbols **of the revision it was measured at** (`--rev`). For every
line a run executed, the innermost symbol containing that line is recorded as covered, by its
stable id (`java:com.acme.Foo#bar(String)`, `ts:src/cart.ts#Cart.total`). Ids contain no line
numbers, so coverage measured at base still names the same method at head when the method
survived, even if it moved.

Paths written by tools are mapped onto repository paths:

- absolute paths under the repository's work tree are made relative;
- anything else (JaCoCo's `com/acme/Foo.java`, another machine's absolute path) matches a
  repository file by suffix on a path-component boundary, **only if exactly one file matches**;
- files that do not map are counted (`unmapped`) and up to five examples are printed. They never
  produce coverage.

A file listed by the tool **without any executable line** (a Java interface, a type-only TS module)
is not counted as measured: nothing in it could have run.

### Testwise vs aggregate coverage

- **Testwise** (`--test T`, or LCOV `TN:` names): the report is attributed to one test, given as a
  symbol id, a test file path (TS) or a Java test class name. An unknown selector is an error
  (`UnknownTest`), never a guess. Each covered non-test symbol becomes a `TESTS` edge
  `T → symbol` with evidence `COVERAGE_OBSERVED` and rule `coverage.<format>@<commit>`; these edges
  join the head graph before impact traversal, so measured tests are reached from a change like any
  other dependent. A test container (Java class, TS file) with such a hop is recommended even when
  some of its units are also recommended on static evidence: testwise coverage is recorded per
  container, so dropping it would hide the measurement.
- **Aggregate** (no test): only feeds coverage status (below). It cannot say *which* test ran the
  code, so it adds no edges.

Per test (and once for aggregate), only the most recently ingested report is used.

### Coverage status of changed and impacted symbols

Reported only when some coverage was ingested, and only for kinds that hold executable code
(method, constructor, function, field, variable):

| Status | Meaning |
|---|---|
| `COVERED` | some used report executed it |
| `NOT_COVERED` | its file was measured (had executable lines) but no used report executed it |
| `NO_DATA` | no used report measured its file |

A **deleted** symbol only exists at base, so only reports measured at the base commit describe it;
otherwise it is `NO_DATA`. Reports from the head side would call it "not covered" merely because
the code is gone.

## 4. History and flakiness

Per test, from all ingested JUnit runs (skipped results ignored):

- `flaky_commits`: commits at which the test both passed and failed — two runs disagreeing, or one
  run that passed after failed attempts (Surefire `flakyFailure`/`flakyError`).
- Reliability:
  - `FLAKY` if `flaky_commits > 0` (one same-commit flip is enough, whatever the run count);
  - else `INSUFFICIENT_DATA` with fewer than `MIN_RUNS = 3` executed runs;
  - else `CONSISTENTLY_FAILING` if every run at the most recent commit failed;
  - else `STABLE`.
- Failing often is **not** flaky: a test that fails at some commits and passes at others, without
  disagreement at any single commit, is reacting to the code. A broken test is `CONSISTENTLY_FAILING`.
- `median_duration_ms`: median of executed runs.

Reliability and durations belong to test units (a Java method, a TS test case); containers carry
no history of their own.

Measured on the fixture: `src/clock.test.ts` (a deliberately random test) failed 4 of 8 runs at v1
and passed 4 of 4 at v2 → `FLAKY`, `flaky_commits = 1`; its neighbours are `STABLE`.

## 5. Selection modes

Recommended tests are ordered by (tier, reliable before flaky, path depth, known median duration,
id). The mode decides whether that list may replace the full suite:

| Mode | Decision |
|---|---|
| `CONSERVATIVE` | full suite if **any** fallback reason applies |
| `BALANCED` (default) | full suite if a **high**-severity reason applies |
| `FAST_FEEDBACK` | always the first 10 ranked tests, with a note that passing them is not complete validation (and, if reasons apply, to run the full suite after) |

A full-suite decision still lists the recommended tests as the best place to start.

### Fallback reasons

| Code | Severity | When |
|---|---|---|
| `MIGRATION_CHANGED` | high | a database migration changed (`db/migration`, `migrations/`, Liquibase, Flyway, `schema.prisma`) |
| `LOCKFILE_CHANGED` | high | a dependency lockfile changed |
| `BUILD_FILE_CHANGED` | high | a build file changed (`pom.xml`, `build.gradle*`, `package.json`, `tsconfig*.json`, bundler/test-runner config, …) |
| `CI_CHANGED` | medium | CI configuration changed |
| `CONTAINER_CHANGED` | medium | a Dockerfile or compose file changed |
| `CONFIG_CHANGED` | medium | application configuration changed (`application*.yml`, `.env*`, `config/` files) |
| `CHANGED_FILE_NOT_UNDERSTOOD` | high | a changed file has a high-severity syntax error, parse failure or exceeds the parse limit |
| `IMPACT_TRUNCATED` | high | impact traversal hit its node limit |
| `UNRESOLVED_REFERENCE` | medium | a changed or impacted symbol has a reference that could not be resolved |
| `NO_TEST_EVIDENCE` | high | non-test code changed and no test is recommended |
| `CHANGED_CODE_WITHOUT_COVERAGE` | medium | coverage is loaded and a modified symbol is `NOT_COVERED` or `NO_DATA` |
| `STALE_COVERAGE` | medium | a used coverage report was measured at a commit other than base or head |

Low-severity uncertainty (e.g. a changed file in an unsupported language) never widens the
selection, so the `UNSUPPORTED_FILE_CHANGED` code exists but is not currently emitted.

On the fixtures: the Java change adds `V2__account_frozen.sql`, so `BALANCED` and `CONSERVATIVE`
decide `FULL_SUITE` (`MIGRATION_CHANGED`); the TypeScript change only has medium reasons, so
`BALANCED` selects 5 of 6 units and `CONSERVATIVE` runs all 6.

### Runtime estimate

`selected_runtime_ms` and `full_runtime_ms` are sums of median recorded durations. Each is present
only when **every** unit it counts has history; otherwise it is absent and a note says why. A unit
selected twice (directly and through its container) counts once.

## 6. Limitations

- **Correlation, not causation.** Coverage says a test executed a line, not that it checks its
  behaviour. A covered change can still break untested behaviour.
- **Stale coverage.** Coverage measured at another commit is used (and flagged `STALE_COVERAGE`);
  code may have moved since. Only the latest report per test is used, by ingestion order, not
  commit order: ingest history oldest first.
- **Unmapped paths** produce no evidence; check the `unmapped` count after ingesting.
- **Line granularity.** A covered line is credited to the innermost symbol containing it; an
  abstract method in a file that also has executable code reads as `NOT_COVERED`.
- **Container granularity.** Testwise coverage is per test run, which the collection tooling makes
  per test class or test file; it cannot say which method of the class executed the code.
- **No execution by Ripplepath.** Evidence is only as good as the CI that produced it; Ripplepath
  cannot detect a report from a different build of the code beyond the commit it is told.
