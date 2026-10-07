# Ripplepath — specification

Status markers: **[implemented]**, **[partial]**, **[planned]**. Only implemented behavior is claimed elsewhere.

## 1. Inputs
- A Git repository (read through the object database; the working tree is not traversed in revision mode).
- `base` and `head` revisions (any revspec gix can resolve: branch, tag, SHA, `HEAD~1`).
- Optional `ripplepath.yml` [planned].
- Optional evidence files: JaCoCo XML, LCOV, JUnit XML [planned].

## 2. Symbols [implemented for Java]
A symbol is a named program element with a **stable identity**:

```
<lang>:<qualified-name>[#<member>[(<param types>)]]
java:com.acme.billing.BillingService#charge(Money,String)
java:com.acme.billing.BillingService
ts:src/checkout/cart.ts#Cart.total            [planned]
```

Identity is built from language, qualified name, kind-specific member name and — for Java methods and
constructors — the *textual* parameter types with generics erased. It does **not** contain line numbers,
so moving a method or editing an unrelated one does not change identity.

Consequences (documented limitations):
- Renaming a symbol or its package creates a new identity. Ripplepath reports a *probable move* when a
  deleted and an added symbol have identical fingerprints.
- Parameter types are textual: `List<String>` and `java.util.List<Integer>` erase to `List` and
  `java.util.List` respectively; a qualified vs. simple spelling change is reported as a signature change.

Kinds: `package`, `file`, `class`, `interface`, `enum`, `record`, `annotation`, `method`, `constructor`,
`field`, `test` (a method that is a recognized test) [implemented for Java].

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

## 4. Edges [implemented for Java]
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
| `TESTS` | test → symbol, evidence-qualified [planned beyond static] |

Evidence classes, strongest first: `RESOLVED_EXACT`, `COVERAGE_OBSERVED`, `STATIC_INFERRED`,
`HISTORY_COCHANGE`, `NAMING_HEURISTIC`. They are ordinal labels, **not probabilities**.

## 5. Impact propagation [implemented]
Starting from changed symbols, traverse dependents:
- every non-`CONTAINS` edge is followed in reverse (`x → changed` makes `x` impacted);
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

## 7. Planned sections
TypeScript indexer, incremental index invariants, test evidence and ranking, fallback rules,
architecture rules and delta, risk model v1, GitHub Action, history/flakiness, replay evaluation.
Each will be specified here as it is implemented.
