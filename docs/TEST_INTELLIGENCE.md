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
| `RESOURCE_CHANGED` | medium | a file under a `resources/` directory changed (properties, templates, data read at run time); no static edge reaches it |
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

## 6. Offline evaluation

`ripplepath evaluate --repo R --cases cases.json --db D [--format json|text] [-o FILE]` replays
historical changes and scores the selection each one *would* have received against the tests that
actually failed. Ripplepath still runs no test: the outcomes are JUnit files produced elsewhere.

```json
{"cases": [{"name": "v4..v5", "base": "main~8", "head": "main~7", "junit": ["evidence/v5/junit/run-1.xml"]}]}
```

`junit` lists the result files of the run **at head** (paths relative to the cases file; several
files are parts of one run, and a test listed twice failed if any entry failed). The database `D`
holds whatever coverage and CI history was ingested over the whole history.

### Leakage controls

For case `base → head`, the analysis may only use evidence that existed before the change:

- **Visible commits = base and its ancestors** (`git::Repo::ancestors`, gix object walk, refused
  rather than truncated above 1 000 000 commits). Passed to the analysis as
  `AnalyzeOptions::evidence_commits`.
- **Coverage** is filtered *before* choosing the latest report per test
  (`Store::latest_coverage_among`), so a newer report from head or later can neither be used nor
  hide an older visible one.
- **CI history** (flakiness, durations used for ordering) keeps only runs recorded at visible
  commits.
- A case whose head is base or one of its ancestors is refused (`HeadNotAfterBase`): its head
  evidence would otherwise count as prior evidence. A case without a JUnit file is refused
  (`NoObservedOutcome`).
- The head run is used only to **score** (which tests failed, and their durations for the runtime
  metrics), never as an input to the selection.

The leakage test (`crates/engine/tests/evaluation.rs`,
`evidence_recorded_at_head_or_later_cannot_affect_a_case`) evaluates `v4..v5` once with a database
holding evidence up to v4 and once with one that also holds v5 (head) and v6 (later); the reports
are byte-identical. Without the restriction the same database changes the decision
(`STALE_COVERAGE` → `FULL_SUITE` in conservative mode), so the test is not vacuous.

### Metrics

Per case and mode, over test **units** (the same counting as `summary.tests_recommended`; a listed
container contributes all its units, a `FULL_SUITE` decision runs every unit):

| Metric | Definition |
|---|---|
| `failing_test_recall` | caught failing tests / failing tests; absent when nothing failed (never "100 %" by default) |
| `missed_failures` | failing tests not in the selection, by id. A failure that maps to no test of head (`junit:<class>#<name>`) is missed unless the full suite runs |
| `selected_test_reduction` | `1 − selected / total` units |
| `runtime_reduction` | `1 − selected / full` recorded duration at head; present only when **every** counted unit has a recorded duration and the full duration is > 0 |
| `first_failure_position` | 1-based position of the first failing test in run order (the analysis's ranking, then the rest on a full-suite decision) |
| `time_to_first_failure_ms` | recorded durations summed up to and including that test; absent if one is unknown |
| `decision`, `fallback_reasons` | what the analysis decided, and why |

Aggregate per mode: failing tests and caught failures **pooled** over cases (recall = Σ caught / Σ
failed), missed failures, failing cases fully caught, `FULL_SUITE` fallback count, mean selected-test
and runtime reduction (with the number of cases that had one), mean first-failure position and time
(over the cases that caught at least one failure). Ratios are rounded to four decimals.

The report (`evaluation_schema_version: 1`) leads with the **sample**: cases, cases with a failing
test, failing tests. Below 30 failing cases it carries `"small sample — not statistically
meaningful"`. There are no confidence intervals or percentages of certainty: the numbers are counts
over the cases given, nothing more. Same inputs ⇒ byte-identical JSON.

### Results on `fixtures/eval-history`

Eleven consecutive changes of an authored twelve-snapshot Java history, with **real** testwise
JaCoCo coverage and JUnit results collected by running the tests at every snapshot
(`fixtures/eval-history/evidence/README.md`: tools, versions, command, date). Each case sees the
evidence of its base and every earlier snapshot.

**Sample: 11 cases, 5 with failing tests, 15 failing tests — small sample, not statistically
meaningful.**

| Mode | Recall (pooled) | Missed | Failing cases fully caught | `FULL_SUITE` | Mean tests saved | Mean recorded runtime saved | Mean first failure |
|---|---|---|---|---|---|---|---|
| conservative | 15/15 (1.0) | 0 | 5/5 | 2/11 | 0.4554 | 0.4528 (11 cases) | position 1.6, 12.0 ms (5 cases) |
| balanced | 10/15 (0.6667) | 5 | 4/5 | 0/11 | 0.6372 | 0.6346 (11 cases) | position 1.75, 14.0 ms (4 cases) |
| fast_feedback | 10/15 (0.6667) | 5 | 4/5 | 0/11 | 0.6372 | 0.6346 (11 cases) | position 1.75, 14.0 ms (4 cases) |

Only v6..v7 and v7..v8 raise a fallback reason (`RESOURCE_CHANGED`, medium). Conservative mode widens
to the full suite on any reason and so catches every failure, at the cost of running all 20 tests in
those two cases; balanced widens only on high-severity reasons. Balanced and fast feedback coincide
because no change had more than 10 recommended tests for fast feedback to cut.

| Case | What changed | Selected / total | Failed | Caught | First failure |
|---|---|---|---|---|---|
| v1..v2 | `Order.subtotal` rewritten with streams | 7 / 17 | 0 | – | – |
| v2..v3 | `Money.percent` loses half-up rounding | 12 / 17 | 3 | 3 | #3, 13 ms |
| v3..v4 | rounding fixed; `BulkDiscount` added | 15 / 20 | 0 | – | – |
| v4..v5 | `BulkDiscount` threshold `>=` → `>` | 8 / 20 | 2 | 2 | #1, 12 ms |
| v5..v6 | threshold fixed; `Inventory.reserve` refactored | 11 / 20 | 0 | – | – |
| v6..v7 | `tax-rates.properties` gains `PT=23%` | 0 / 20 | 5 | **0** | – |
| v7..v8 | `PT=23` | 0 / 20 | 0 | – | – |
| v8..v9 | `Inventory.reserve` bounds broken | 7 / 20 | 4 | 4 | #1, 6 ms |
| v9..v10 | bounds fixed; `CheckoutService.reserveStock` extracted | 7 / 20 | 0 | – | – |
| v10..v11 | free-shipping threshold `>=` → `>` | 4 / 20 | 1 | 1 | #2, 25 ms |
| v11..v12 | threshold fixed; `ShippingCalculatorTest` added | 6 / 22 | 0 | – | – |

**Every missed failure is from v6..v7.** The change touches only `src/main/resources/tax-rates.properties`;
`TaxCalculator.fromResource()` parses it at run time (`Integer.parseInt`), and the malformed `23%` makes
`TaxCalculatorTest#loadsRatesFromResource` and four `CheckoutServiceTest` tests error. Ripplepath has
no edge from Java code to a resource file it reads by name, the file matches no fallback pattern
(`CONFIG_CHANGED` covers `application*.yml`, `.env*`, `config/`), and an unsupported file change is
low severity and never widens the selection (§5). The first replay therefore selected nothing and decided
`SELECTED` with no reason — the clearest gap this evaluation found.

**What changed as a result, and what it does not prove.** Files under `resources/` directories are now
classified `RESOURCE` and raise `RESOURCE_CHANGED` (medium): a general rule for data code loads by name,
not a pattern matching this fixture. Balanced mode still runs nothing for v6..v7 (it widens only on
high-severity reasons) but now says why; conservative mode runs the full suite and catches all five.
Because the rule was added after seeing this miss, the conservative 15/15 is not an independent
measurement — a held-out history would be needed for that. Before the rule all three modes scored
10/15 with zero fallbacks.

Runtime figures are sums of single-run JUnit durations of a few milliseconds each, measured on one
laptop; they vary between collections and say nothing about CI time on a real suite.

### Limitations of the evaluation

- **Authored history.** The fixture's regressions were written to exercise the tool, by the same
  project; real histories have other failure modes (flaky tests, environment, data).
- **Evidence at every base.** Each case assumes coverage and CI results were recorded at its base
  commit; with sparser evidence, `STALE_COVERAGE` and other fallbacks would fire more often.
- **Ingestion order.** Among visible reports for one test, the most recently *ingested* wins
  (§7); ingest history oldest first.
- **Unit-level counting.** Metrics count test units (methods); a selected test class counts as all
  of its units.
- **Durations at head.** Runtime and time-to-first-failure use the durations of the observed run at
  head, as the best record of what running that selection would have cost.

## 7. Limitations

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
