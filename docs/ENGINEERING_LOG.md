# Engineering log

Meaningful discoveries only, newest first.

## 2026-10-08 — Offline replay evaluation of test selection

**Leakage is a property of the evidence query, not of the database.** The evaluation reuses one
database holding the whole history's coverage and CI runs, and restricts each case to evidence
recorded at base or its ancestors (`AnalyzeOptions::evidence_commits`). The subtle part was
coverage: "latest report per test" chosen first and filtered second lets a newer report from head
hide the older visible one, so the restriction is applied before the choice
(`Store::latest_coverage_among`). The leakage test proves a database with head and later evidence
gives byte-identical results, and that the same database *without* the restriction changes the
decision — otherwise the test could pass vacuously.

**Recall without failures is undefined, not 100 %.** Six of the eleven fixture cases had no failing
test; scoring them as perfect recall would inflate the headline. Recall is pooled over failing tests
across cases and absent when nothing failed; the sample size leads the report and anything under
30 failing cases is labelled "small sample — not statistically meaningful".

**The first real measurement found a real gap.** On `fixtures/eval-history` (11 cases, 5 failing,
15 failing tests, evidence from actually running the tests at every snapshot), recall is 10/15 in
every mode. All five misses are one change that only edits `tax-rates.properties`: the Java code
reads it by name at run time, Ripplepath has no edge to it, it matches no fallback pattern, and an
unsupported-file change is low severity — so zero tests were selected with no reason given. That is
left visible rather than fixed with a rule tuned to this fixture; a resource-file fallback (or
resource-name edges) is the follow-up the number points to.

**Re-collection as provenance check.** The salvaged evidence had no provenance record, so it was
collected again with `scripts/collect-eval-history.sh`; the two collections differ only in
durations, timestamps and JaCoCo session times. Runtime figures are millisecond-level single-run
durations and are documented as such, not as CI savings.

## 2026-10-08 — Read-only MCP server for coding agents

**The protocol moved under us.** The current MCP revision (`2026-07-28`) is stateless: no
`initialize`, per-request `_meta` with version and client capabilities, mandatory `server/discover`,
`resultType` on every result. Deployed clients still send `initialize`. The adapter is dual-era,
selected per request by the presence of `_meta["io.modelcontextprotocol/protocolVersion"]`.

**Hand-written over `rmcp`.** The official SDK is maintained and Apache-2.0, but a sequential
stdio adapter needs about a dozen messages, and the SDK's async/macro layers and release churn
across the protocol transition cost more than they save. Owning the line reader is also what lets
the 1 MiB message bound apply *before* buffering. ADR 0007.

**`what_if` reuses the analysis instead of approximating it.** A hypothetical edit is a
`ChangedSymbol` with `MODIFIED` fed to the same impact traversal and `recommend_tests`; on the
java-banking fixture `what_if Money#minus(Money)` reaches `Account#withdraw` (Account.java:27,
`CALLS`, `RESOLVED_EXACT`) and `TransferService#transfer` at depth 2, and recommends one test each
from `MoneyTest`, `TransferServiceTest` and `TransferControllerTest`, each with its path.
What it cannot know (new calls, signature changes) is stated in its note.

**Dependency paths need direction and a ranking.** `dependency_path` follows stored edges (depends-on)
with the impact traversal's ranking, skips `CONTAINS` (it would join any two members of a class), and
falls back to the reverse chain so "how are A and B related" gets an answer either way. A miss is
reported as "not proof of independence", with `search_truncated` when the cap made it inconclusive.

**Agents need ids before they can ask.** Every symbol tool takes exact ids; `find_symbols` and the
"similar ids" list on an unknown id (whole id → last segment → owner's members) make that cheap
without guessing on the agent's behalf.

## 2026-10-08 — Dogfooding on QuantaRun

Full write-up and reproducible commands: docs/DOGFOODING_QUANTARUN.md.

**The fixture said "no unresolved references"; a real Spring service had 2,478.** Almost all came
from idioms the hand-written fixtures did not use: record accessors (`job.id()`: 239 alone),
`var` initialised from a call, enum methods inherited from `java.lang.Enum`, and generic JDK
containers (`map.get(id).cancel()`, `for (var j : jobs)`, `xs.stream().filter(j -> j.live())`).
After modelling them: 112. Fixtures written by the tool's author test what the author thought of.

**A wrong fingerprint was harmless until an edge pointed at it.** Record components carried the
fingerprint of the whole record, so adding a method to a record marked every component modified.
Nothing referenced components, so nobody noticed; once accessor calls became edges, one new method
on `WorkerProperties` impacted every caller of every accessor. 69 spurious changes in one commit.

**The grammar, not the code, was broken.** Four valid Java 21 files had syntax errors:
tree-sitter-java 0.23.5 accepts only a simple name at the head of a record pattern
(`case Outer.Rec(var x) ->`). Each such file made the analysis fall back to the full suite. The fix
re-parses a copy where those dots become `_`; because only bytes of equal length change, every
node offset is valid in the original text, which is where names are read from.

**Flattened scopes were safe only while lambdas were untyped.** Locals are flattened per method
and the last same-named declaration won. With lambda parameters typed, `w` in two lambdas over
different collections would have been silently mistyped. A local is now typed only when all its
declarations agree. Resolving every declaration branches, so receiver typing got a per-reference
step budget; the regression test (8 declarations per name, 20 levels) took 23 s with a 4,096-step
budget in a debug build and passed only after the budget became 256.

**"Unresolved" needs a reason to suspect a missing edge.** `rs.getString(1)` in a JDBC lambda was
reported even though no repository type declares `getString`: no repository edge can be missing
there. Java now uses the same rule as the TypeScript frontend (report only names some repository
type declares). That rule is sound, not a heuristic: it is about the target set, not the receiver.

**`any` is not a primitive.** Treating predefined TypeScript types as external also swallowed the
fixture's deliberately untyped `const legacy: any`, which an engine test caught; `any`, `unknown`
and `object` stay untyped.

**Open, not fixed:** after the forward `OVERRIDES` hop from a changed implementation to its
interface method, traversal also reaches *sibling* implementations, recommending their tests
(17 of 60 static-path tests in the reference range). Spring HTTP routes have no static edges, so a
changed controller signature impacted nothing in `82dacd0`.

## 2026-10-08 — Session 4: real test evidence for the fixtures

**Evidence is collected, not written.** `scripts/collect-evidence-{java,ts}.sh` compile and run
only Ripplepath's own fixtures (JUnit Platform 6.1.3 + JaCoCo 0.8.15 without Maven; Vitest 5.0.3
with V8 coverage), one test class/file per run for testwise coverage. The only edits are removing
machine-identifying values (JVM system properties, host names). A deliberately random TypeScript
test recorded a real same-commit flip: 4 failures in 8 runs at v1, 0 in 4 at v2 → `FLAKY`.

**Real output found three wrong answers that synthetic samples did not.**
(1) JaCoCo lists interfaces with zero lines; counting them as "measured" made every interface
method `NOT_COVERED`. Files without executable lines no longer count as measured.
(2) A deleted method got `NOT_COVERED` from head-side coverage — trivially true, since the code is
gone. Deleted symbols now use only reports measured at the base commit.
(3) Testwise coverage is per test class/file, so its `TESTS` hop lands on the container; the
container was then dropped because a unit was already listed on static evidence, and the
TypeScript report showed no measured evidence at all. Containers with a coverage hop are kept.

**Formats matched the mapping.** Vitest's JUnit `classname` is the test file and `name` the
`describe > title` chain, exactly the TS test id; JUnit Platform's `classname`/`method()` match the
Java id. Every recorded result maps (0 unmapped). Vitest's LCOV on Windows uses backslashes.

## 2026-10-08 — Configuration, architecture delta, owners, risk model v1, policy

**Whose rules apply is a security question.** If a pull request's own `ripplepath.yml` were used,
the change could delete the rule it breaks. The configuration is read from the base revision's tree
(object access only) and applied to both graphs; head's edits are validated and flagged. The test
`configuration_comes_from_base_so_a_change_cannot_weaken_its_own_rules` fails if head's file is used.
ADR 0005.

**The same edge kind means different things to impact and to architecture.** IMPORTS does not
propagate impact (session 2), but an import of another layer's type *is* an architecture
dependency. The java-banking change shows both: `Account.java` imports `ApiErrors` (one violation on
the file symbol) and `Account#withdraw` calls it (a second violation on the method).

**One import closed a four-layer cycle.** The domain → api dependency is the only new edge, yet it
creates a new cycle api → application → domain → api (persistence included). Two signals fire for
one root cause; per-signal caps bound the effect and ADR 0006 records that signals are correlated.
The fixture scores 99 (CRITICAL) — reported as measured, not tuned down.

**A comment is not a critical change.** The first decomposition counted `Money.java` under
`critical_path_changed` although its only edit was a comment: file-level change detection leaked
into a symbol-level tool. Parsed source now counts only with a changed symbol; unparsed files
(SQL migrations) always count.

**Signature changes would have made violations flicker.** Violation identity is (from, to, kind),
and a re-signed method has a new id; without mapping `previous_id → id` (and probable moves) a
violation through it would be reported removed *and* new. The mapping reuses the change
classification's identities rather than inventing similarity.

**Config spelling vs. report spelling.** The report serialises `SelectionMode` as
`FAST_FEEDBACK`; the config file says `fast_feedback`. Deriving the config parser from the report
enum would have accepted only the shouting form; the config has its own enum.

**CODEOWNERS is not gitignore.** GitHub documents that `docs/*` owns direct children only, while a
gitignore `docs/*` would also cover nested files through the matched directory. The matcher is a
single dynamic-programming pass over (pattern segment, path segment) that answers full-path and
directory-prefix matches together, so matching costs O(pattern segments × path segments) even for a pathological `**/a*` pattern.

## 2026-10-07 — Session 3: persistent incremental index

**Incremental facts, recomputed resolution.** Resolution depends on every file's imports, so the
honest incremental unit is per-file *facts*; the stored graph is updated by difference. ADR 0004.

**A test that cannot fail proves nothing, again.** The incremental-equals-clean property test was
mutation-checked: disabling deletion of stale edges in `apply_graph` makes both the fixture test and
the random-history property test fail.

## 2026-10-07 — Session 2: TypeScript frontend

**First CI run failed on a local blind spot.** `playwright.config.ts` was added after the last local
typecheck and needs Node types. CLAUDE.md now says to re-run web checks after adding any file.

**`beforeEach` is not a test, but it is test code.** Suite-level hooks run for every test in the
file. Their references are attributed to the file symbol, and test files are marked `is_test`, so a
change can reach "run this whole file". The same gap existed in Java: a modified `@BeforeEach
setUp()` had no dependents and recommended nothing. Test *containers* are now recommended when shared
code inside them changes or is impacted (`changed_lifecycle_method_recommends_its_whole_test_class`).

**Adding a `describe` marked the whole test file changed.** The suite wrapper, and even the
statement's trailing `;`, leaked into the file fingerprint. It now covers only module-level code
plus suite-level statements.

**IMPORTS should not propagate impact.** With TypeScript every importing file became "impacted",
flooding the list. Real use of an import already yields CALLS/REFERENCES edges, so IMPORTS (like
CONTAINS) no longer propagates. Imports remain in the graph as evidence.

**Untyped receivers need a noise filter, not silence.** Reporting every call on an untyped value
(`items.reduce`, `res.json`) as unresolved would bury real gaps. A call is reported only when its
member name is declared somewhere in the repository, the case where an edge may truly be missing.

**Security review: three denial-of-service paths in the TS frontend.** (1) `extends a.b.b…` and
`new a.b.b…()` recursed per segment and overflowed the stack. (2) `export *` resolution explored
every path through layered barrels: with 6 re-exports per layer the old resolver did not finish in
120 s; a per-query visited set plus memoised results makes it 10 ms. The first version of that test
used 2 branches per layer and passed against the *old* code too — a test that cannot fail proves
nothing, so it was widened until it reproduced. (3) Several scans made big files quadratic; the
worst, shared with Java, was fingerprint exclusion via `Vec::contains` (50k members: 21 s → 4 s in a
debug build). Diamond re-exports were also wrongly marked ambiguous; providers are now deduplicated.

**Interface dispatch matters in TS too.** `Cart.total` calls `this.prices.quote()` on a `PriceSource`
injected through a constructor parameter property; the change in `PricingService.quote` reaches it
only through the forward `OVERRIDES` hop to the interface member.

## 2026-10-07 — Session 1

**Name collision.** "ImpactTrace" is an active software product (impacttrace.io); "Blastline" is an
npm package in exactly this domain. Renamed to Ripplepath before any public artifact existed.

**Git CLI is an execution vector.** Running `git diff`/`git status` on an untrusted repository can
execute programs configured in that repository (`core.fsmonitor`, `diff.external`, textconv). Chose
`gix` with isolated permissions and computed line diffs ourselves (imara-diff, histogram). Cost: we
own rename detection — implemented as exact-blob then line-Dice ≥ 50% pairing, bounded at 10k pairs.

**Line overlap is the wrong definition of "symbol changed".** Mapping hunks to enclosing symbols
marks a method as changed when only a comment above it moved. Fingerprints over tree-sitter leaf
tokens, excluding comments and nested member spans, give the intended semantics: comment/format-only
edits are not changes, and editing a method does not mark its class. Hunks are still mapped to
innermost symbols, for display only. Pure insertions map to the symbol spanning the gap so code
added after a closing brace is attributed to the class, not the previous method.

**Dispatch needs a forward edge.** Reverse traversal from a changed implementation
(`StandardFeePolicy.feeFor`) finds nothing: callers are bound to the interface method. Following
`OVERRIDES` *forward* during impact (impl → declaration, then reverse from the declaration) recovers
`TransferService.transfer → TransferController.transfer → test`, labelled "dispatch" in paths.

**Deleted symbols live in the base graph.** The caller of a deleted method no longer references it in
head (or does so in broken code). Deleted roots are traversed on the base graph; results are
labelled `graph: base`.

**Stack overflow from a crafted type name (security review finding).** Type erasure recursed through
left-nested `scoped_type_identifier`s; a 100k-segment name overflowed the stack
(`STATUS_STACK_OVERFLOW`, reproduced by the regression test against the old code). Fixed by bounding
recursion; deep receiver chains and type nesting were already capped.

**Two server findings from review.** (1) Binding to 127.0.0.1 does not stop DNS rebinding — added
`Host` validation. (2) A semaphore permit held in the request future is released when the client
disconnects, while the blocking analysis continues; moved the permit into the blocking task.

**Graph UI: isolated changes dominated the canvas.** Changed symbols without dependents (new helper
classes, private fields) formed many tiny components laid out in one strip, shrinking the meaningful
path to unreadable size. Fixed with component packing (ELK aspect ratio) and hiding isolated nodes by
default with a visible count; they remain in the left list.

**Windows line endings.** A global `core.autocrlf=true` would have given fixtures CRLF on checkout and
changed blob ids. `.gitattributes` pins LF; the fixture builder also normalises.
