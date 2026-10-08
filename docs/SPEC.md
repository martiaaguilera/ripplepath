# Ripplepath — specification

Status markers: **[implemented]**, **[partial]**, **[planned]**. Only implemented behavior is claimed elsewhere.

## 1. Inputs
- A Git repository (read through the object database; the working tree is not traversed in revision mode).
- `base` and `head` revisions (any revspec gix can resolve: branch, tag, SHA, `HEAD~1`).
- Optional `ripplepath.yml`, read from the **base** revision [implemented] (§10, ADR 0005).
- Optional evidence files: JaCoCo XML, LCOV, JUnit XML [implemented; see docs/TEST_INTELLIGENCE.md].

## 2. Symbols [implemented for Java]
A symbol is a named program element with a **stable identity**:

```
<lang>:<qualified-name>[#<member>[(<param types>)]]
java:com.acme.billing.BillingService#charge(Money,String)
java:com.acme.billing.BillingService
ts:src/checkout/cart.ts#Cart.total
ts:src/cart.test.ts#test:Cart > totals line items
```

Identity is built from language, qualified name, kind-specific member name and — for Java methods and
constructors — the *textual* parameter types with generics erased. It does **not** contain line numbers,
so moving a method or editing an unrelated one does not change identity.

Consequences (documented limitations):
- Renaming a symbol or its package creates a new identity. Ripplepath reports a *probable move* when a
  deleted and an added symbol have identical fingerprints.
- Parameter types are textual: `List<String>` and `java.util.List<Integer>` erase to `List` and
  `java.util.List` respectively; a qualified vs. simple spelling change is reported as a signature change.

Kinds: `file`, `class`, `interface`, `enum`, `record`, `annotation`, `method`, `constructor`, `field`,
`function`, `variable`, `type_alias`, `test_case` [implemented]. A Java test is a `method` with
`is_test`; a TypeScript test is a `test_case`. Test *containers* are a Java test class or a TS test
file (`is_test` on a non-unit symbol). Enum values in this contract may gain variants without a
schema version bump; consumers must tolerate unknown values.

## 3. Fingerprints and change classification [implemented for Java]
Each symbol has a fingerprint: BLAKE3 over its tree-sitter leaf tokens, **excluding comments and
excluding the spans of nested member symbols**. Hence:
- whitespace/formatting/comment-only edits do not mark a symbol modified;
- editing one method does not mark its enclosing class modified;
- changing a class header (extends, annotations, modifiers) marks the class modified.

Classification of `base → head`:

| Class | Rule |
|---|---|
| `ADDED` | id only in head |
| `DELETED` | id only in base |
| `MODIFIED` | id in both, fingerprint differs |
| `SIGNATURE_CHANGED` | a deleted and an added method/constructor in the same owner with the same name |
| `PROBABLE_MOVE` | a deleted and an added symbol with equal fingerprint and kind (reported, not merged) |

Textual diff hunks (imara-diff, Histogram) are computed per changed file and attached to the change set
for display and for mapping lines → enclosing symbol; they are **not** the source of truth for
"modified".

## 4. Edges [implemented for Java and TypeScript]
Edges are directed `from → to` meaning *from depends on to*. Each edge carries
`kind`, `evidence`, `file`, `span`, `rule`.

| Kind | Meaning |
|---|---|
| `CONTAINS` | structural parent → child (not traversed for impact) |
| `IMPORTS` | file imports a type |
| `EXTENDS` / `IMPLEMENTS` | type → supertype |
| `OVERRIDES` | method → method it overrides/implements |
| `CALLS` | method/ctor → method/ctor it invokes |
| `INSTANTIATES` | method/ctor → constructor or type created with `new` |
| `REFERENCES` | member → type it mentions (field type, parameter, return, local variable, cast…) |
| `TESTS` | test → symbol it executed, from testwise coverage (`COVERAGE_OBSERVED`) [implemented] |

Evidence classes, strongest first: `RESOLVED_EXACT`, `COVERAGE_OBSERVED`, `STATIC_INFERRED`,
`HISTORY_COCHANGE`, `NAMING_HEURISTIC`. They are ordinal labels, **not probabilities**.

An edge is `RESOLVED_EXACT` only when every step that found its target follows the language's own
rules over declarations in the repository. A step that relies on anything else (several overloads
of one arity, ambiguous `export *`, or element types of JDK containers taken from Ripplepath's fixed
table of `java.util` signatures) makes the edge `STATIC_INFERRED`. See docs/LANGUAGE_SUPPORT.md for
the per-language rules.

## 5. Impact propagation [implemented]
Starting from changed symbols, traverse dependents:
- every edge except `CONTAINS` and `IMPORTS` is followed in reverse (`x → changed` makes `x`
  impacted). An import alone does not run or name code; its use produces `CALLS`/`REFERENCES`;
- `OVERRIDES` is *also* followed forward from a changed implementation to the declaration it overrides,
  because callers bound to the declaration may dispatch to the implementation;
- traversal is breadth-first, depth-bounded (default 6), visits every symbol once, terminates on cycles;
- for every impacted symbol the result holds one *explaining path* back to a changed symbol, chosen by
  (depth, weakest-edge evidence strength, symbol id) — deterministic;
- `DELETED` symbols are traversed on the **base** graph (their dependents may now fail to compile);
  everything else on the **head** graph.

## 6. Determinism [implemented]
Collections that are serialized are `BTreeMap`/sorted `Vec`. Same repo + revisions + config + tool
version ⇒ byte-identical JSON. `analysis.json` carries `schema_version`.

## 7. Test recommendations [implemented]
- A test unit is recommended when it changed (`CHANGED_TEST`) or is impacted (`STATIC_PATH`).
- A test container is recommended when its own code changed, when a non-test symbol inside it
  (lifecycle hook, fixture, helper) changed or is impacted, when it is impacted through a measured
  coverage hop (testwise coverage is recorded per container), or when it is impacted and none of
  its units is listed.
- With an evidence database, coverage adds `TESTS`/`COVERAGE_OBSERVED` edges, coverage status,
  history/flakiness, evidence tiers, selection modes and fallback reasons: docs/TEST_INTELLIGENCE.md.
- `summary.tests_recommended` counts test *units* the selection runs: listed units plus all units
  inside a listed container, each once — or every unit on a `FULL_SUITE` decision;
  `summary.tests_total` counts all test units in head.

## 8. Persistent incremental index [implemented]
- `ripplepath index --rev R` stores the graph of R in SQLite, by default in the user cache directory
  (never inside the analysed repository, which could otherwise ship a forged index).
- Facts are cached per (blob id, path, extractor version); only changed files are parsed.
- Resolution runs over the whole snapshot; stored rows are updated by difference and the run
  reports the delta (symbols/edges added, removed, updated).
- **Invariant**: after any sequence of index runs, stored rows equal a clean index of the final
  revision. `analyze --db` with a warm cache produces byte-identical output to a cold run.

## 9. Planned sections
Replay evaluation. Each will be specified here as it is implemented.

## 10. Configuration [implemented]
`ripplepath.yml` at the repository root; full schema in docs/ARCHITECTURE_RULES.md. Strict parsing
(unknown keys are errors), bounded input (64 KiB, 64 layers, 256 rules, 1 024 patterns).
- Read from the base revision's tree through object access and applied to both graphs; head's file
  is validated and reported (`config.head_change`), never applied (ADR 0005).
- Invalid base file: defaults are used, errors reported, gate `config_invalid` fails.
- `tests.mode` sets the selection mode unless `--mode` is given.
- Report section `config`: source (`DEFAULTS` | `BASE_REVISION` | `INVALID_USING_DEFAULTS`), revision,
  blob, head change, errors of each side, critical/generated patterns, mode, policy lists.

## 11. Architecture rules and delta [implemented]
Layers by path glob (first match wins); every edge kind except `CONTAINS` and `TESTS` is a
dependency (`IMPORTS` included); `deny`/`allow` rules; violations keyed by (from, to, kind) and
classified `NEW` / `PRE_EXISTING` / `REMOVED` after mapping signature changes and probable moves;
layer cycles by Tarjan SCC; layer direction reversals; module coupling deltas. Report section
`architecture`. Details: docs/ARCHITECTURE_RULES.md.

## 12. Owners [implemented]
CODEOWNERS from head (`.github/`, root, `docs/`, first found), GitHub semantics, last match wins.
Report section `owners`: owners of changed files and of files of impacted symbols. Metadata, not
authorization.

## 13. Public API surface and risk model v1 [implemented]
Report section `api_surface` (removed, signature changed, narrowed, added) and `risk`
(`model_version` 1, 14 signals, integer points, bands). **The risk score is not a probability.**
Details: docs/RISK_MODEL.md, ADR 0006.

## 14. Merge policy [implemented]
Report section `policy`: per gate `level` (`OFF` | `WARN` | `FAIL`) from the base configuration and
`status` (`PASS` | `WARN` | `FAIL` | `OFF` | `NOT_EVALUATED`); result `FAIL` if any gate fails, else
`WARN` if any warns, else `PASS`. A failing gate whose findings all rest on inferred edges warns
instead. The risk score is never a gate. CLI exit codes: 0 success, 1 error, 2 policy `FAIL` with
`analyze --fail-on-policy`.

The report additions in §10–§14 are additive fields; `schema_version` stays 1.

## 15. Single-revision queries [implemented]
Used by the MCP adapter (docs/MCP.md); computed on one revision's graph, with ingested coverage
joined as in §7 when a database is given.
- **What if** `symbol`: §5 impact from that one root, treated as `MODIFIED` (same id), then §7 test
  recommendation. Equals what `analyze` reports for a committed body-only edit of the symbol.
  Uncertainty: unresolved references in the root and impacted symbols, syntax errors in the root's
  file, unindexed files and rejected paths of the revision, truncation, coverage from other commits.
- **Dependency path** `from → to`: shortest chain of edges in their stored direction (`from` depends
  on … depends on `to`), every kind except `CONTAINS`, breadth-first and depth-bounded (default 8);
  among equally short chains the strongest weakest-edge evidence wins, then parent id, then edge.
  If the visited-symbol cap is hit without finding `to`, the miss is reported as inconclusive.
- **Symbol info**: the symbol, its layer (none for generated paths), its incoming and outgoing edges,
  rule-breaking dependencies it takes part in (the revision's own `ripplepath.yml`, as
  `architecture check`), and the tests the what-if recommendation lists.

## 16. CI outputs and GitHub Action [implemented]
`analyze --format text|json|markdown|sarif|github-annotations`; `--output-dir D` additionally writes
`analysis.json`, `summary.md`, `ripplepath.sarif`, `annotations.txt`; `--path-prefix P` prefixes file
paths in SARIF and annotations. All are pure renderings of the report and byte-identical for the
same report.
- Located findings (SARIF, annotations): `NEW` architecture violations, breaking API changes,
  high-severity parse failures of changed files, configuration errors (file-level, line 1 in
  SARIF). Level from the corresponding policy gate (`FAIL` → error, `WARN` → warning, else note).
  Rule ids `ripplepath/<gate-like-name>`; `partialFingerprints["ripplepath/findingHash/v1"]` is
  line-independent.
- Markdown: at most 60 000 bytes; repository text only inside code spans or punctuation-escaped.
- The composite action (`action.yml`) runs `analyze --fail-on-policy`, publishes summary,
  annotations, artifact, optional SARIF and one PR comment (marker `<!-- ripplepath -->`), then
  exits 2 on policy `FAIL`. Details: docs/GITHUB_INTEGRATION.md.
