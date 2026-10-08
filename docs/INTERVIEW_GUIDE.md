# Interview guide

How to explain Ripplepath in an interview, grounded in this repository. Every claim here points to a
file, an ADR or an entry in [ENGINEERING_LOG.md](ENGINEERING_LOG.md). If an interviewer asks about
something not listed here, say it is not implemented. [README § Known limitations](../README.md#known-limitations)
lists the honest answers.

**About how it was built.** The commit history shows AI-assisted implementation (Claude
co-authorship). Say so plainly if asked. What makes the project yours to defend is the decisions,
the evidence and the corrections: the ADRs, the dogfooding on your own project, the mutation-checked
tests, and the bugs below. Make sure you can explain every one of them without notes.

---

## 1. The 30-second explanation

> "Ripplepath answers: *what can this code change break, and which tests should I run?* It reads
> two Git revisions, works out which methods and classes changed, not just which files, and follows
> a typed dependency graph outward to everything that depends on them. Every step in that graph has
> a file, a line and an evidence class, so each claim is a path you can check. On top of that it
> recommends tests and widens to the full suite when evidence is weak. It also flags new
> architecture-rule violations and breaks review risk into named signals, explicitly not a
> probability. It's a Rust core with a React UI, runs locally, and never executes code from the
> repository it analyses."

## 2. The two-minute architecture explanation

Draw this left to right:

1. **Git layer** (`crates/git`). Opens the repository with `gix` in isolated mode, resolves `base`
   and `head`, lists trees and reads blobs with a size check *before* inflating. It computes line
   diffs itself (imara-diff, histogram) and pairs renames: exact blob first, then ≥ 50 % line
   similarity, capped at 10,000 pairs. No `git` process.
2. **Language frontends** (`crates/lang`), in two phases:
   - `extract(path, source) → facts` is pure and per file. Its result is cached by
     `(blob id, path, extractor version)`.
   - `resolve(all facts) → symbols + edges` is pure and per snapshot. Imports, hierarchies and
     receiver types need the whole snapshot.
3. **Graph** (`crates/graph`). A `CodeGraph` with sorted edges and adjacency indices. `impact` is a
   level-synchronous reverse BFS with explaining paths. `dependency_path` finds the shortest
   evidence-ranked chain between two symbols.
4. **Engine** (`crates/engine`). Builds both snapshots, classifies symbol changes with fingerprints,
   runs impact (head graph, or base graph for deleted roots), and selects tests. Then `assess` adds
   the architecture delta, API surface, owners, risk signals and policy gates. Everything except two
   blob reads is a pure function.
5. **Storage** (`crates/storage`). SQLite holds the fact cache, the indexed graph (updated by
   difference) and test evidence.
6. **Adapters.** The CLI renders text, JSON, Markdown, SARIF and annotations. The Axum server feeds
   the React UI. The MCP server runs over stdio. All of them render one `AnalysisReport`
   (`analysis.json`, schema v1). None of them makes decisions.

Close with the three invariants: same inputs give byte-identical output; incremental index equals
clean index; nothing from the analysed repository is executed.

---

## 3. Deep dives

### 3.1 Tree-sitter, not regex

- Regex cannot tell a method call inside a comment or string from a real one. It cannot find the
  span of a method so that it can be fingerprinted, and it cannot handle nesting.
- Tree-sitter gives a concrete syntax tree. It **recovers from syntax errors**, so a half-broken
  file still yields symbols, and those errors are flagged as uncertainty. Its incremental, C-based
  parsers are fast.
- The cost is the grammar's own bugs. tree-sitter-java 0.23.5 rejects valid Java 21 record patterns
  with a qualified head (`case Outer.Rec(var x) ->`). On QuantaRun, four files had syntax errors, and
  each one forced the analysis to fall back to the full suite. The fix re-parses a copy in which
  those dots become `_`. Only bytes of **equal length** change, so every node offset is still valid
  in the original text, which is where names are read from. The re-parse is kept only if it has
  fewer error lines (`crates/lang/src/java`, docs/LANGUAGE_SUPPORT.md).

### 3.2 Why not LSP or the compiler first

ADR 0002. Exact binding needs javac/tsc or a language server, and those need a configured build:
classpath, `tsconfig`, dependencies. That means **running the repository's build tooling**, which
is executing untrusted code, plus minutes of setup per repository. Ripplepath instead runs a
documented static resolver per language. Every edge records `rule` and `evidence`, and anything it
cannot bind is reported, not guessed. The trade-off is lower precision for overloads (matched by
name and arity), generics and inference, and it is surfaced on every edge. The ADR keeps the door
open: a sandboxed, opt-in compiler frontend could later *upgrade* edges to exact evidence, but it
would not replace the static path.

### 3.3 Symbol identity and fingerprints

- **Identity** (docs/SPEC.md §2): `java:com.acme.bank.domain.Money#minus(Money)`,
  `ts:src/cart.ts#Cart.total`. Language, qualified name, member name, and for Java methods the
  *textual* parameter types with generics erased. **No line numbers**, so moving a method or editing
  a neighbour keeps its identity. Coverage measured at base still names the same method at head,
  and architecture violations keyed by `(from, to, kind)` do not flicker.
- **Fingerprint** (`crates/lang/src/syntax.rs::fingerprint`): BLAKE3 over tree-sitter leaf tokens,
  **excluding comments and the spans of nested members**. Comment-only and formatting-only edits are
  not changes, and editing a method does not mark its class as modified.
- **Why not line overlap** (Engineering log, session 1): mapping diff hunks to enclosing symbols
  marks a method as changed when only the comment above it moved. Hunks are still mapped, but only
  for display.
- **Classification:** `ADDED`, `DELETED`, `MODIFIED`, `SIGNATURE_CHANGED` (a deleted and an added
  method with the same name in the same owner) and `PROBABLE_MOVE` (identical fingerprint and kind,
  reported but not merged).
- **Bugs this design surfaced:**
  - A TypeScript file fingerprint included the `describe` wrapper and even the trailing `;`, so
    adding a test marked the whole file changed.
  - On QuantaRun, record components carried the whole record's fingerprint. Adding one method to
    `WorkerProperties` marked every component modified, which produced 69 spurious changes in one
    commit. Nobody noticed until accessor calls became edges.
  - "A comment is not a critical change": `Money.java`, which only gained a comment, was counted as
    a critical-path change. That was file-level detection leaking into a symbol-level tool.

### 3.4 Git diff to symbols, and why gix instead of the git CLI

ADR 0003. `git diff` on an untrusted repository can run programs configured *in that repository*:
`core.fsmonitor`, `diff.external`, textconv drivers, filters. Its output also depends on the user's
global config. Ripplepath opens repositories with `gix::open::Options::isolated()` and reads only
the object database. The price is owning rename detection and line diffs, which are documented,
deterministic and bounded. The same rule shaped the GitHub Action, which checks for `.git/shallow`
on disk instead of running `git rev-parse --is-shallow-repository`.

Flow: resolve both revisions to trees, then path diff, then rename pairing. Symbols of changed files
on each side are compared by id and fingerprint. Hunks are attached for the UI's diff view.

### 3.5 Reverse dependency graph and OVERRIDES dispatch

`crates/graph/src/impact.rs`. Edges point `from → to`, meaning "from depends on to". Impact follows
them **in reverse** from the changed symbols, except `CONTAINS` (structure) and `IMPORTS`.

- **Why IMPORTS does not propagate** (session 2): with TypeScript every importing file became
  "impacted". Real *use* of an import already yields `CALLS`/`REFERENCES`, so imports stay in the
  graph as evidence but do not spread impact. Architecture rules **do** count them: an import of
  another layer's type is a compile-time dependency.
- **Dispatch** (session 1): a change to `StandardFeePolicy.feeFor` had no reverse edges, because its
  callers bind to the interface `FeePolicy.feeFor`. Impact therefore also follows `OVERRIDES`
  **forward**, from the implementation to the declaration and then backwards to its callers. The
  hop is labelled `via_dispatch`. The same was needed in TypeScript (`Cart.total` calls
  `this.prices.quote()` on an injected `PriceSource`).
- **Known over-approximation** (dogfooding): after that forward hop, traversal also walks *sibling*
  implementations' `OVERRIDES` edges in reverse. On QuantaRun's reference range, 17 of 60
  static-path tests were false positives from this. The fix would change the semantics in SPEC §5,
  so it is recorded as an open finding rather than patched quietly.
- **Deleted symbols** are traversed on the **base** graph, because their callers no longer
  reference them in head.

### 3.6 Cycles and termination

Level-synchronous BFS with a `visited` map: each symbol is visited once, which guarantees
termination on cycles, and depth is bounded (default 6, `--max-depth` 1–32). The impacted set is
capped at 5,000, which raises `IMPACT_TRUNCATED` (a high-severity fallback). Among equally short
candidates at one level, the winner is chosen by (strongest weakest-edge evidence, parent id, edge).
So the explaining path is both shortest and most defensible, **independent of hash order**. A
property test checks that impact is independent of edge order. Layer cycles (architecture) use
Tarjan's SCC with nodes and neighbours visited in sorted order (`crates/engine/src/architecture.rs`).

### 3.7 Incremental indexing and the incremental == clean property

ADR 0004. Resolution depends on every file, so making it incremental would need cross-file
dependency tracking, which brings real complexity and a correctness risk. The honest incremental
unit is the per-file **facts**. Resolution is recomputed as a pure in-memory pass, and the stored
graph is **updated by difference** (`Store::apply_graph` in `crates/storage/src/graph.rs`). The
run reports the delta, for example `symbols 70 (+0 -0 ~1)` after a one-method edit.

- **The invariant:** after any sequence of index runs, the stored rows equal a clean index of the
  final revision.
- **The test:** `crates/engine/tests/incremental.rs`. One test covers the fixtures. A proptest
  generates random histories of `Set`, `Remove` and `Move` operations over 5 Java classes and 5 TS
  modules whose dependencies depend on a variant, so edits add and remove edges, symbols and
  unresolved references. A third test checks that warm-cache `analyze` equals cold `analyze`.
- **The mutation check:** disabling the deletion of stale edges in `apply_graph` makes both the
  fixture test and the property test fail. "A test that cannot fail proves nothing" recurs in this
  project. The first export-star DoS regression test also passed against the *old* code until it
  was widened.

### 3.8 Cache invalidation

- **Extractor versions.** The fact cache key includes `EXTRACTOR_VERSION` (`java` 4, `ts` 2 in
  `crates/lang/src/*/mod.rs`). Changing what a frontend extracts means bumping it, and
  `crates/engine/src/snapshot.rs` has a compile-time `assert!` pinning both values. A frontend change
  therefore cannot silently reuse stale facts without someone editing that line.
- **Schema.** SQLite migrations run via `PRAGMA user_version`. A database created by a *newer*
  Ripplepath is refused rather than guessed at.
- **Corrupt entries.** A fact-cache entry that fails to decode is treated as a cache miss
  (`FactCache::lookup`), because parsing again is always correct.
- **Configuration is a cache-like trust question** (ADR 0005). `ripplepath.yml` is read from the
  **base** revision's tree. If head's file were used, a PR could delete the rule it breaks. The test
  `configuration_comes_from_base_so_a_change_cannot_weaken_its_own_rules` fails if head's file is
  used.
- **Cache poisoning.** The default index lives in the user cache directory, keyed by canonical repo
  path, never in the working tree. A repository could otherwise ship a forged index.

### 3.9 Why SQLite

ADR 0004. A single file, bundled (`rusqlite` with `bundled`), no server, transactional, and it runs
in CI and on laptops. Queried data lives in structured tables (`symbols`, `edges`, `unresolved`,
`indexed_files`, plus evidence tables). Fact payloads are an opaque serialised cache. Rejected
alternatives: a **graph database**, because traversal runs in memory and the storage layer does no
traversal; and **per-revision graph copies**, because storage would grow with history for no query
that needs it.

### 3.10 Static vs dynamic test evidence

docs/TEST_INTELLIGENCE.md. Ripplepath **never runs tests**. It ingests JaCoCo, LCOV and JUnit that
CI produced.

- **Static path:** a test can reach the change through code edges.
- **Testwise coverage:** a `TESTS` edge `test → symbol` with `COVERAGE_OBSERVED`. It joins the head
  graph before traversal, so measured tests are reached like any dependent.
- **Aggregate coverage:** only feeds `COVERED` / `NOT_COVERED` / `NO_DATA`. It cannot say *which*
  test ran the code.
- Real collected output found **three wrong answers** that synthetic samples did not (session 4):
  - JaCoCo lists interfaces with zero lines, so every interface method was `NOT_COVERED`.
  - A deleted method got `NOT_COVERED` from head-side coverage, which is trivially true because the
    code is gone. Deleted symbols now use only base-commit reports.
  - Testwise coverage is per class or file, so the `TESTS` hop lands on the container. The container
    was then dropped as redundant, which hid the measurement.

### 3.11 Ranking tiers and fallback

- **Tiers** (`STRONG` / `MEDIUM` / `WEAK`) come from the explaining path:
  - `STRONG`: the test changed, or the path has a coverage hop and every static hop is
    `RESOLVED_EXACT`.
  - `MEDIUM`: an all-exact static path, or a coverage hop with an inferred static hop.
  - `WEAK`: any inferred edge.
- **Order:** tier, then reliable before flaky, then depth, then median duration, then id.
  Deterministic.
- **Fallback reasons** have a severity, for example: `MIGRATION_CHANGED`, `BUILD_FILE_CHANGED`,
  `LOCKFILE_CHANGED`, `CHANGED_FILE_NOT_UNDERSTOOD`, `IMPACT_TRUNCATED` and `NO_TEST_EVIDENCE` are
  high; `UNRESOLVED_REFERENCE`, `STALE_COVERAGE`, `RESOURCE_CHANGED` and `CONFIG_CHANGED` are medium.
- **Modes:**
  - `CONSERVATIVE`: full suite on any reason.
  - `BALANCED`: full suite on a high-severity reason.
  - `FAST_FEEDBACK`: always the top 10, with a note.
- The point to make: **uncertainty broadens the recommendation**. A full-suite decision still lists
  the ranked tests as where to start.

### 3.12 Flakiness

The research consensus (Luo et al.; iDFlakies, in docs/RESEARCH.md) is that the strong signal is
**different outcomes for the same code**. A test is `FLAKY` if at some commit it both passed and
failed: two runs disagree, or one run passed after failed attempts (Surefire `flakyFailure`). One
same-commit flip is enough. Failing often is **not** flaky. A test that fails at some commits and
passes at others, without disagreeing at any single commit, is reacting to the code, and a broken
test is `CONSISTENTLY_FAILING`. Fewer than 3 executed runs gives `INSUFFICIENT_DATA`. Real data: a
deliberately random fixture test failed 4 of 8 runs at v1 and 0 of 4 at v2, which classifies it as
`FLAKY` with `flaky_commits = 1`.

### 3.13 Offline evaluation and leakage

`ripplepath evaluate` replays `base → head` cases and scores the selection against the JUnit run at
head.

- **Leakage is a property of the evidence query, not of the database** (Engineering log). Each case
  sees only evidence at base or its ancestors (`AnalyzeOptions::evidence_commits`). The subtle bug:
  choosing the "latest report per test" *first* and filtering *second* let a newer head report hide
  an older visible one. The filter is now applied before the choice (`Store::latest_coverage_among`).
- **The leakage test is not vacuous.** It adds head and later evidence and expects byte-identical
  output. It also proves that the *unrestricted* query changes the decision.
- **Honest metrics:** recall is pooled over failing tests and *absent* when nothing failed (6 of 11
  cases), never a default 100 %. Below 30 failing cases the report is labelled
  `small sample — not statistically meaningful`.
- **Result:** balanced mode caught 10/15. All misses came from one `.properties` change with no
  edge and no fallback, so zero tests were selected and *no reason was given*. That led to
  `RESOURCE_CHANGED`. Conservative mode now catches 15/15, but that is **not independent**, because
  the rule was added after seeing the miss.

### 3.14 Architecture delta

docs/ARCHITECTURE_RULES.md.

- Layers are assigned by path glob (first match wins), and rules are `deny` or `allow`.
- Both graphs are evaluated with **base's** configuration, and violations are keyed by
  `(from, to, edge kind)`.
- Before comparing, base ids are mapped through `SIGNATURE_CHANGED` and probable moves. Otherwise a
  re-signed method would show its violation as both removed *and* new.
- Statuses are `NEW`, `PRE_EXISTING` and `REMOVED`. By default only new violations matter: the delta
  is the author's concern, not historical debt.
- A new violation resting only on inferred edges can warn but never fail.
- The demo shows that one import can produce two violations (the file's `IMPORTS` and the method's
  `CALLS`) and close a four-layer cycle.

### 3.15 Deterministic risk, not a probability

ADR 0006, docs/RISK_MODEL.md.

- There is no outcome dataset, so any probability would be fabricated precision.
- The score is 14 named signals, each `min(units × weight, cap)` in integer arithmetic, with a total
  cap of 100 and four bands, all versioned as `model_version` 1.
- Missing input gives `evaluated: false`, never a silent zero.
- **Signals are correlated.** The demo's one domain → api import fires both
  `new_architecture_violation` and `new_layer_cycle`. Caps bound that; the fixture still scores 99,
  reported as measured, not tuned down.
- **Never a gate.** A threshold on an uncalibrated sum would make an ordering aid an arbitrary
  blocker. Gates evaluate concrete findings.
- No per-repository weight overrides, so scores are comparable across repositories at one model
  version.

### 3.16 GitHub Action, SARIF and pinning

docs/GITHUB_INTEGRATION.md.

- The composite action runs `analyze --fail-on-policy` (exit 2 on `FAIL`) and publishes the step
  summary, annotations, artifact, optional SARIF and one PR comment. The comment is found by marker
  *and* author `github-actions[bot]`, so a comment by anyone else that contains the marker is
  ignored.
- **SARIF:** only located findings. `partialFingerprints["ripplepath/findingHash/v1"]` hashes the
  finding's identity, not its line, so alerts survive unrelated line moves.
- **Pinning:** with no release yet, pin a commit SHA and the action builds from that exact source
  with cargo. With a `v*` tag it downloads the archive and verifies its `.sha256`, failing on a
  mismatch without falling back to another source. Be precise: the checksum detects corrupted or
  swapped downloads, **not a compromised release**. Third-party actions in the workflows are pinned
  by SHA.
- **Injection:**
  - Inputs reach the scripts as environment variables, never as `${{ }}` spliced into shell.
  - Log output is wrapped in `::stop-commands::<random token>`, so a file named `::add-mask::` is
    not executed.
  - Markdown escapes punctuation and puts a WORD JOINER after `@`/`#`, so symbol names cannot mention
    users or reference issues.
- **Forks** get a read-only token: the comment and SARIF steps are skipped with a notice. The docs
  warn against `pull_request_target`.

### 3.17 Untrusted-repository security

docs/SECURITY_MODEL.md. Findings from the project's own security reviews, each with a regression
test that reproduced the bug first:

- **Stack overflow from a crafted type name:** a 100k-segment left-nested
  `scoped_type_identifier` overflowed the stack (`STATUS_STACK_OVERFLOW`). Recursion is now bounded.
  The same issue appeared in TS (`extends a.b.b…`).
- **Exponential `export *`:** with 6 re-exports per layer of barrels, the old resolver did not
  finish in 120 s. A per-query visited set plus memoisation brings it to 10 ms.
- **Quadratic scans:** fingerprint exclusion used `Vec::contains`. With 50k members it took 21 s,
  and 4 s after the fix (debug build).
- **DNS rebinding:** binding to 127.0.0.1 is not enough, because a malicious page can rebind a
  hostname to loopback. The server rejects any `Host` that is not loopback or `--allow-host`ed
  (`dns_rebinding_hosts_are_rejected`).
- **Cache poisoning:** covered in 3.8.
- **Terminal escapes and Trojan Source:**
  - `cli::text::neutralize_terminal_controls` escapes control characters, Unicode Cf format
    characters (bidi overrides) and line/paragraph separators.
  - Stored values in error messages are escaped and truncated.
  - The UI uses text nodes only and a CSP of `script-src 'self'`.
- **XML:** entity declarations are refused (billion laughs, XXE), and inputs are bounded in size,
  element count and depth.
- **Paths:** tree paths must be UTF-8 and relative, with no `..`, no `.git` component in any case,
  and no backslash or drive prefix. Symlinks and submodules are never followed.

### 3.18 Rust ownership and concurrency choices

- **Rayon only for parsing** (`crates/engine/src/snapshot.rs`). Parsing dominates and is independent
  per file. `extract` is pure: it borrows one file's text and returns owned facts, so no shared mutable
  state crosses threads. Results are inserted into the cache sequentially afterwards, and
  resolution is single-threaded and deterministic.
- **Shared, immutable facts** are held in `Arc<JavaFile>` / `Arc<TsFile>`, so a file unchanged
  between base and head is parsed once and shared by both snapshots without copying.
- **Server** (`crates/server/src/lib.rs`):
  - Analyses are CPU-bound and synchronous, so they run on Tokio's **blocking pool** via
    `spawn_blocking`, at most 2 at once (`Semaphore`). File reads have a separate limit of 8.
  - The bug found in review: the permit was held by the request future. If the client disconnected,
    the future was dropped and the permit released, while the blocking analysis kept running, so an
    attacker could start unbounded analyses by disconnecting. The permit is now **moved into the
    blocking task**, and its lifetime is the work's lifetime. That is ownership used as a resource
    guarantee.
- **MCP** is deliberately sequential and synchronous. There is no async runtime (one reason for not
  using the `rmcp` SDK, ADR 0007).
- **Determinism:** `HashMap` is used only for lookups that never reach output. Serialised
  collections are `BTreeMap` or sorted `Vec`. Edge deduplication keeps (strongest evidence, earliest
  line, rule), independent of visit order.
- `unsafe_code = "forbid"` workspace-wide. `unwrap`/`expect` are linted, and errors are typed with
  `thiserror` per crate.

### 3.19 Performance work

- The quadratic and exponential cases above came from **security review**, not profiling. On
  hostile input, a performance bug is a denial-of-service bug.
- **Receiver-typing budget:** once lambda parameters were typed, resolving every declaration of a
  same-named local branched. A regression test (8 declarations per name, 20 levels) took 23 s with a
  4,096-step budget in a debug build and passed only after the budget became **256** steps per
  reference.
- **CODEOWNERS** matching is one dynamic-programming pass over (pattern segment × path segment), so
  a pathological `**/a*` pattern stays polynomial.
- **Benchmarks:** the harness lives in `crates/bench`. Each timed run happens in a fresh process,
  so "cold" really is cold and peak memory belongs to that run. `scripts/bench.sh` runs it, and
  `.github/workflows/bench.yml` runs it weekly, never as a merge gate. Quote numbers only from
  [BENCHMARKS.md](BENCHMARKS.md). From dogfooding: a QuantaRun cold index took a median of 1.46 s,
  and a no-change re-index 0.70 s, on a 4-core laptop (orders of magnitude only).

### 3.20 The biggest real bugs (pick two or three)

1. **2,478 unresolved references on a real Spring service, after fixtures reported none.** The
   causes were record accessors (`job.id()`, 239 alone), `var` from calls, enum methods, and generic
   JDK containers. After modelling them in steps, the count went 2,478 → 702 → 461 → 243 → 153 → 112.
   TypeScript went from 51 to 8. The lesson: *fixtures written by the tool's author test what the
   author thought of.*
2. **The silent-mistyping trap.** Locals are flattened per method, and the last declaration with a
   given name won. That was harmless while lambdas were untyped, but once they were typed, `w` in
   two lambdas over different collections would have been mistyped silently. Now a local is typed
   only when every declaration agrees.
3. **The record-component fingerprint** (3.3): 69 spurious changes, invisible until an edge pointed
   at the wrong fingerprint.
4. **Evaluation leakage through "latest report"** (3.13).
5. **The semaphore permit released on disconnect** (3.18).
6. **Java method ids end in `)`.** The UI's evidence-prose linker stripped trailing `)`, so no
   method id in risk or policy evidence was ever a link.

---

## 4. Hard questions

**"What changes for a 50-million-line monorepo?"**

The current design's limits are explicit:

- A hard limit of 200,000 files per snapshot.
- Resolution recomputed over the whole snapshot on every run (only parsing is incremental).
- One SQLite file per repository.
- One machine's cores for parsing.

In order, I would:

1. Measure first with the bench harness's synthetic sizes, to find whether resolution or I/O
   dominates.
2. Make resolution incremental with per-file dependency tracking (which files' bindings depend on
   which exports), guarded by the same incremental == clean property test. ADR 0004 names this as
   the next step.
3. Have CI build and publish the index for `main`, so developers start warm.
4. Scope analysis by ownership or build-system targets (Bazel/Nx already know the coarse graph) and
   use symbols within them.
5. Raise or shard the bounds deliberately, keeping truncation explicit.

What stays the same: evidence on every edge, determinism, and no execution.

**"When would you need a distributed backend?"**

When one index has to serve many developers and CI jobs for repositories too large to index on a
laptop, or when you want cross-repository edges. At that point you need a shared, versioned index
service (indexed per commit by CI, queried by clients) instead of a local SQLite file. Until then a
distributed backend adds operational cost and a trust boundary for no gain: today everything runs
locally with no keys, which is a design goal. My other project, QuantaRun, is exactly that kind of
system (leases, fencing tokens, failure recovery), so I know what it costs.

**"When would you use a graph database?"**

When traversal has to happen at the storage layer. That means graphs too large for memory, or many
concurrent ad-hoc queries over a shared index. ADR 0004 rejected one because traversal runs in
memory over a `CodeGraph` built per snapshot and SQLite only stores rows updated by difference. A
graph DB would add a server and non-determinism risks (query planner order) without serving any
current query.

**"When would ML test ranking make sense?"**

When there is a real dataset: many changes, with test outcomes, split by time. And it must beat a
deterministic baseline on held-out history. Ripplepath has the evaluation harness for that
(leakage-controlled replay, pooled recall, small-sample labelling), but its only dataset is an
11-case authored fixture. ADR 0006 and RESEARCH §6 exclude ML until then. Even then, the ranking
would be one more evidence source with its own class. It would not replace explaining paths, and it
should never skip tests when the static evidence says they are reachable.

**"What could an LLM add, and why isn't it the source of truth?"**

An LLM could *summarise* structured findings in prose, or help an agent decide what to ask through
MCP. It must not create dependency facts. An edge needs a file, a line and a rule that can be
re-derived, and the output must be byte-identical for the same input. An LLM gives neither, and it
can be steered by text inside the analysed repository, which is untrusted. So the MCP server exposes
deterministic engine output only (`readOnlyHint`, evidence on every hop), and the project rule is
"optional AI may only summarise structured findings", labelled as such. That summariser is not
implemented.

**"What would you do differently?"**

- Dogfood on a real repository from day one. The fixtures hid more than 2,000 unresolved references.
- Write the evaluation harness before tuning the fallback rules, so that rules are tested on
  held-out data.

**"How do you know it's correct?"**

- 220 Rust tests (unit, integration, property tests) and 40 web unit tests, plus a Playwright smoke
  against the real server, all in CI.
- Mutation checks on the critical invariants: incremental == clean, the leakage test, and the DoS
  regressions.
- Golden decompositions such as `banking_risk_decomposition_is_exact`.
- Dogfooding precision: a spot check of 14 sampled `STATIC_INFERRED` edges on QuantaRun found all
  14 correct. And where it is wrong (sibling over-selection, HTTP routes), that is written down.
