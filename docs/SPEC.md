# Ripplepath — specification

Status markers: **[implemented]**, **[partial]**, **[planned]**. Only implemented behavior is claimed elsewhere.

## 1. Inputs
- A Git repository (read through the object database; the working tree is not traversed in revision mode).
- `base` and `head` revisions (any revspec gix can resolve: branch, tag, SHA, `HEAD~1`).
- Optional `ripplepath.yml` [planned].
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
Architecture rules and delta, risk model v1, GitHub Action, replay evaluation.
Each will be specified here as it is implemented.
