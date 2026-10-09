# Dogfooding on QuantaRun

Ripplepath run on a real project by the same author, to find where the frontends fall short on
idiomatic code, fix the highest-value gaps, and record what is still wrong. Every number below
comes from a run described here. Nothing in CI depends on this repository being reachable: the
regression tests added as a result are minimal reproductions written for Ripplepath's own suite.

## Target

- Repository: <https://github.com/martiaaguilera/quantarun> (public; cloned read-only, never
  modified, nothing executed from it: no build, no test, no hook).
- HEAD at the time of the run: `ddb6b2f4dba5968013221f06142198ec325ad67e` (20 commits).
- Shape: Java 25 / Spring Boot 4.1 control plane and worker (219 `.java` files, 23,207 lines), a
  Vite 8 + React 19 + TypeScript 6 console (43 `.ts`/`.tsx` files and 1 `.js` file, 4,717 lines),
  Flyway SQL migrations, Docker/YAML config. 373 files at HEAD, 263 of them parsed as source.
- Idioms that matter for static analysis: Spring constructor injection into `private final` fields
  (Lombok appears only in 3 template files under `.claude/skills/`, which are indexed like any
  other Java file), records everywhere (DTOs, read models, sealed result types), `var` almost
  everywhere (1,261 `var x =` declarations), `java.util` collections and streams with
  lambdas, sealed interfaces with record patterns in `switch`, JUnit 5 + AssertJ, MockMvc/HTTP
  integration tests; on the console side an `api` object literal of arrow functions, TanStack Query
  hooks, Vitest + Testing Library tests named `*.test.ts(x)`, relative imports only (no `tsconfig`
  path aliases, so alias resolution was not needed here and was not implemented).

> On Windows the clone needs `git config core.longpaths true` before checkout: several Java paths
> exceed 260 characters. Ripplepath itself reads the object database and does not need a checkout.

## Reproducing the reference analysis

Ripplepath at commit `8d4e633` (the last code commit of this session), release build.

```sh
git clone https://github.com/martiaaguilera/quantarun quantarun
cargo build --release -p ripplepath-cli
B=cb13484adac69df8ab56934c12110a08c00b3edc   # "docs(release): final review ... (Phase 15)"
H=3b9ec7d129055a6ca5242df362b9389a4525a1df   # "fix(worker): release security review: memory OOM, HTTP client retries, proxy routing"
target/release/ripplepath index   --repo quantarun --rev $H --db /tmp/qr.db
target/release/ripplepath analyze --repo quantarun --base $B --head $H --db /tmp/qr.db
target/release/ripplepath analyze --repo quantarun --base $B --head $H --db /tmp/qr.db --format json -o analysis.json
```

The JSON report was byte-identical with and without `--db` (cold run), SHA-256
`d5b9ffff980eea807ec48aecbf14f7314821313c08b79da99389a471888afd93` (547,771 bytes). The hash pins
this Ripplepath version too: any change to extraction or report format changes it.

### Result

`Files changed 12 | symbols changed 24 | impacted 74 across 2 module(s) | tests 70 of 337`;
test selection decision `SELECTED` (Balanced mode), no fallback to the full suite.

**Changed symbols (24).** All in the worker:
- `AttemptExecutor#completed(WorkerProtocol.Assignment,Map,RuntimeException,boolean)` modified,
  `AttemptExecutor#cappedMillis(Duration)` added (the Retry-After cap now compared as a `Duration`);
- `HttpWorkload#execute(Payload,AttemptContext)` modified (automatic HTTP client retries off) and
  the `HttpWorkload.java` header (imports);
- `MemoryWorkload`: `execute` modified; fields `budgetMib`, `heldMib`, constructors `<init>()` and
  `<init>(long)`, methods `hold`, `tryReserve`, `heldMib()` added (the shared memory budget);
- tests: 3 new methods in `AttemptExecutorTest`/`HttpWorkloadTest`, the new `MemoryWorkloadTest`
  (class, 4 tests, nested `Probe` class with `main`).

The textual diff also touched six Markdown files, reported as `UNSUPPORTED_LANGUAGE` (low).

**Blast radius (74 impacted, all `RESOLVED_EXACT`).** By depth: 13 / 14 / 17 / 24 / 6 (depths 1–5).
Representative explaining paths (file:line is the edge's evidence):
- `MemoryWorkload#<init>()` ← `INSTANTIATES` `AttemptExecutor#<init>(…)` (AttemptExecutor.java:122)
  ← `INSTANTIATES` `AttemptExecutorTest#setUp()` (AttemptExecutorTest.java:59): the executor builds
  its workload table in its constructor, so every executor test is reached.
- `AttemptExecutor#completed(…)` ← `CALLS` `AttemptExecutor#execute(Running)` (AttemptExecutor.java:300)
  ← … ← `WorkerAgent#claimStep()` ← `WorkerLifecycleRunner#start()` (depth 5).
- `HttpWorkload#execute` ← `CALLS` `HttpWorkloadTest#assertBlocked(HttpWorkload,String)`
  (HttpWorkloadTest.java:242, rule `java.call.local`) ← `CALLS` `HttpWorkloadTest#loopbackLiteral_isBlocked()`.

**Recommended tests (70 units, 68 entries).** 8 `STRONG` (`CHANGED_TEST`: the new tests), 60
`MEDIUM` (`STATIC_PATH`), e.g.:
- `HttpWorkloadTest#slowTarget_timesOut()` — depth 1, `CALLS` HttpWorkloadTest.java:226 (`java.call.field`).
- `StagedWorkloadTest#memoryWorkload_holdsAndReleasesTheRequestedMemory()` — depth 1,
  `INSTANTIATES` StagedWorkloadTest.java:93 (`java.new`).
- `AttemptExecutorTest` (class, through `setUp`) and its 21 methods — depth 2–4.
- `WorkerAgentExecutionTest`, `WorkerAgentTest` (classes, via their `setUp`) — depth 2.

No coverage or CI history was ingested, so every recommendation is static evidence only
(`evidence: {coverage_reports: 0, test_runs: 0, …}`); runtimes are not estimated.

**Uncertainty (20 items).** 14 `UNRESOLVED_REFERENCE` (medium), 6 `UNSUPPORTED_LANGUAGE` (low):
- `failureClass/0` ×10 and `retryAfter/0` ×1: calls inside AssertJ lambdas,
  `isInstanceOfSatisfying(WorkloadFailure.class, failure -> failure.failureClass())`. The lambda
  parameter is untyped, and `WorkloadFailure#failureClass()` exists in the repository, so a real
  `CALLS` edge is missing here and is correctly surfaced instead of guessed.
- `add/2`, `close/0` ×2: `exchange.getResponseHeaders().add(…)` / `exchange.close()` on the
  parameter of a `com.sun.net.httpserver` handler lambda. External in reality; reported because
  repository types also declare `add`/`close`.

**Architecture and risk sections.** Not available yet in this Ripplepath version (they are being
built in parallel); this document will be extended when they exist.

### What a reviewer should make of it

True positives: everything at depth ≤ 2 above is a test that really exercises the changed code.
**False positives (17 of the 60 static-path tests):** `HttpWorkload#execute` is reached through
the forward `OVERRIDES` hop to `Workload#execute` (correct: callers of the interface may dispatch to
it), and from there the traversal follows the *other* implementations' `OVERRIDES` edges in reverse
(`CpuHashWorkload#execute`, `DelayWorkload#execute`, `FailWorkload#execute`,
`MockInferenceWorkload#execute`, `StagedWorkload#execute`), recommending `WorkloadTest.CpuHash`,
`WorkloadTest.Delay`, … which call those sibling implementations directly and never run
`HttpWorkload`. This is engine traversal semantics (docs/SPEC.md §5), not a frontend gap; see
"Open findings". **False negatives:** the AssertJ-lambda calls above (flagged as uncertainty).

## Before and after this session

`index` at HEAD with the binary built before this session (`d6264c9`) and after (`8d4e633`):

| Metric (HEAD `ddb6b2f`) | Before | After |
|---|---:|---:|
| Unresolved references, Java | 2,478 | 112 |
| Unresolved references, TypeScript | 51 | 8 |
| Files with syntax errors (Java; none in TypeScript) | 4 | 0 |
| Symbols | 3,939 | 4,061 |
| Edges | 11,554 | 12,940 |
| …of which `STATIC_INFERRED` | 13 | 360 |

Java unresolved count after each step (same HEAD): records + `var` + enums → 702; ignore untyped
calls to names no repository type declares → 461; JDK container element types and for-each → 243;
lambda parameters of container methods, and refusing to type ambiguous same-named locals → 153;
typing same-named locals when all declarations agree → 112. The 122 extra symbols are TypeScript
object-literal members (`api.jobs`, `keys.workers`, …).

Per commit range (`analyze --base X~1 --head X`, Balanced mode; before → after):

| Range (head) | Changed symbols | Impacted | Tests selected | Unresolved items | Decision |
|---|---|---|---|---|---|
| `3b9ec7d` worker security fixes | 24 → 24 | 74 → 74 | 70 → 70 of 337 | 136 → 14 | selected → selected |
| `cb13484` Phase 15 (Java, TS, migration) | 36 → 22 | 22 → 22 | 330 → 330 of 330 | 64 → 2 | full suite (migration) |
| `64520bb` Phase 14 security | 40 → 40 | 8 → 8 | 327 → 18 of 327 | 53 → 2 | full suite → selected |
| `951f6cd` Phase 13 performance | 112 → 98 | 170 → 169 | 320 → 320 | 200+ → 37 | full suite (migration) |
| `82dacd0` Phase 11 console | 681 → 796 | 0 → 0 | 300 → 30 of 300 | 200+ → 3 | full suite → selected |
| `2ac6925` Phase 12 reliability | 181 → 173 | 63 → 56 | 314 → 70 of 314 | 200+ → 14 | full suite → selected |
| `6136e61` Phase 6 jobs | 391 → 322 | 147 → 146 | 197 → 197 | 200+ → 38 | full suite (build file, migration) |

"200+" is the report's cap of 200 listed unresolved references plus a summary item. Three ranges
stopped falling back to the full suite only because the qualified-record-pattern syntax errors are
gone (`CHANGED_FILE_NOT_UNDERSTOOD`, high). The others fall back for reasons that are real: a Flyway
migration or a build file changed. Fewer changed symbols is the record-component fix: before it,
adding one method to a record marked every component modified (69 spurious changes in `6136e61`).
More changed symbols in `82dacd0` are the new object-literal members, each a symbol of its own.

Precision spot check: of the 347 `STATIC_INFERRED` `CALLS`/`REFERENCES` edges whose rule is not
`java.call.unqualified` (345 of them new this session), every 25th by file and line (14 edges) was
read against the source; all 14 named the right target (e.g. `claimed.payload()` in a
`stream().map(claimed -> …)` over `List<JobAttempts.Claimed>`, `capacity.slots()` where
`var capacity = worker.worker.capacity()` inside a for-each over `Map.values()`).

## Gaps found and what was done

| Gap | Effect on QuantaRun | Fix |
|---|---|---|
| Record accessors `job.id()` not modelled | largest unresolved group (`id/0` ×239 …) | accessor → `REFERENCES` to the component, typed chaining |
| Enum `name()`/`values()`/`valueOf()` unresolved | `values/0` ×26, `valueOf/1` ×16 | enums inherit external `java.lang.Enum` |
| `var x = call()` untyped | every `var` from a method call | typed from the resolved declaration's return type |
| Generics erased for `List`/`Map`/`Optional`/streams | `get/1`, `orElseThrow`, for-each and lambda receivers | JDK container table, `STATIC_INFERRED` |
| Flattened scopes: last same-named local wins | silent mistyping risk once lambdas are typed | type only when all declarations agree |
| Untyped calls to library-only names reported | `getString/1`, `getLong/1`, `formatted/1` … | reported only if a repository type declares the name |
| Record component fingerprint = whole record | spurious MODIFIED components, false impact | per-component fingerprint |
| Qualified record patterns are syntax errors (grammar) | 4 files flagged, full-suite fallbacks | length-preserving re-parse; components typed |
| TS object-literal members invisible | every page depended on all of `api`; `api.workers()` unresolved | properties are members of the variable |
| TS `Row[]`/`string` annotations untyped | `rows.push(...)` reported whenever a class has `push` | arrays/primitives are external; `any` stays untyped |

Regression tests: `crates/lang/tests/java_modern_idioms.rs`, `crates/lang/tests/ts_modern_idioms.rs`
(each fix was mutation-checked: reverting it makes its test fail).

## Open findings (not fixed here)

- **Sibling implementations through an interface** (above): 17 of 60 static-path tests in the
  reference range. A traversal that, after a forward dispatch hop to a declaration, follows only
  callers of that declaration and not other implementations' `OVERRIDES` edges would remove them.
  This is a change to the impact semantics in docs/SPEC.md §5 and to engine/graph code owned by
  parallel work, so it is recorded rather than changed here.
- **HTTP routes are invisible.** Spring controllers are called by the framework and tested through
  MockMvc/HTTP clients with URL strings. In `82dacd0`, `JobController#list` and `JobQueries#list`
  changed signature and `JobRepository#list(ListFilter)` changed, yet nothing was impacted: no
  static edge links `/api/v1/jobs` in a test to the `@GetMapping` that serves it. A route index
  (annotation paths ↔ string literals in tests) would be `NAMING_HEURISTIC` evidence at best.
- **Unresolved lambdas of non-container APIs** (AssertJ, JDBC row mappers, Spring callbacks, JDK
  `HttpServer` handlers) remain the largest unresolved group (112 left in Java).
- **Untyped values in Playwright e2e tests** (`response.status()`, `request.url()`): 6 of the 8
  remaining TypeScript unresolved references. The other 2 are a value typed by an indexed access
  type (`Lane['marks']`) and `(x ?? []).at(-1)`.
- Template code is analysed as product code: the Java files under `.claude/skills/` are indexed
  and can appear in blast radii. Path exclusion belongs in `ripplepath.yml` once it exists.
- Edge lines for a multi-line call chain point at the first line of the chain, not the line of the
  member called.

## Performance (CLI wall-clock, for the record; measured benchmarks are in [BENCHMARKS.md](BENCHMARKS.md))

Release build, Intel Core i7-6700HQ (4 cores / 8 threads, 2.6 GHz), 19.9 GB RAM, NVMe SSD,
Windows 11. Median of 5 wall-clock runs, measured with no other build running:

| Operation | Median |
|---|---:|
| `index` HEAD, cold (263 source files parsed) | 1,463 ms |
| `index` HEAD again, nothing changed | 702 ms |
| `analyze` `cb13484..3b9ec7d`, no database | 892 ms |
| `analyze` `cb13484..3b9ec7d`, with the index above | 935 ms |

While other agents were compiling on the same machine, the same cold index took 1.1–2.4 s; treat
these as orders of magnitude, not benchmarks.
