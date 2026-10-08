# Architecture rules, drift and policy configuration

Everything here is implemented. `ripplepath.yml` lives at the repository root and is read from the
**base** revision of a change (ADR 0005). Without the file, defaults apply: no layers, no critical
paths, every configurable gate except `architecture_violation` at *warn*.

## Schema (version 1)

```yaml
version: 1                      # required; only 1 is understood
architecture:
  layers:                       # order matters: the first layer whose pattern matches a path owns it
    - name: api                 # 1-64 chars: letters, digits, '_' or '-'; unique
      match: ["src/main/java/com/acme/bank/api/**"]
    - name: domain
      match: ["src/main/java/com/acme/bank/domain/**"]
    - name: persistence
      match: ["src/main/java/com/acme/bank/persistence/**"]
  rules:
    - from: domain
      deny: [api, persistence]  # domain must not depend on these
    - from: api
      allow: [domain]           # api may depend only on domain (and itself)
  cycles: warn                  # off | warn (default) | forbid
critical:                       # paths whose change is review-critical
  - "src/main/java/com/acme/bank/domain/**"
  - "src/main/resources/db/migration/**"
generated:                      # code that takes part in no layer, module pair or coverage signal
  - "**/generated/**"
tests:
  mode: balanced                # conservative | balanced | fast_feedback; `--mode` overrides
policy:
  fail_on: [new_architecture_violation, new_cycle, parse_failure_in_changed_file]
  warn_on: [breaking_api_change, removed_test_on_critical_path, config_changed]
```

Every mapping rejects unknown keys (`deny_unknown_fields`): `fail_one:` or `critcal:` is an error
naming the key, never a silently ignored gate. Validation errors: unsupported `version`, duplicate
or malformed layer names, a layer without patterns, an empty or `/`-prefixed or malformed glob, a
rule naming an unknown layer, a rule with both or neither of `deny`/`allow`, an empty `deny`, a rule
listing its own layer (intra-layer dependencies are never checked), a gate in both policy lists,
and listing `config_invalid`. Bounds: 64 KiB file, 64 layers, 256 rules, 1 024 patterns; YAML
alias expansion is bounded by serde-saphyr's default budget.

### Globs
Matched against repository-relative paths with `/` separators (globset, `literal_separator`):
`*` and `?` never cross `/`, `**` spans any number of directories, `{a,b}` alternatives and
`[...]` classes are supported. Patterns are relative to the root and must not start with `/`.

## Layer assignment
A symbol belongs to the layer of its **file path**: the first layer, in declaration order, with a
matching pattern. Unmatched files and files matching `generated` belong to no layer. Layers are
assigned by globs, not by language package declarations, so the same rules work for Java and
TypeScript and for tests (include or exclude test directories through the patterns).

## What counts as a dependency
Every edge kind except:
- `CONTAINS` — structure inside one file;
- `TESTS` — evidence about test execution (partly measured at runtime), not code dependency.

`IMPORTS` **does** count, unlike in impact propagation. An import does not run code, which is why it
does not propagate impact; but an import of another layer's type is a compile-time dependency, and
it is exactly what reviewers and ArchUnit-style checks consider an architecture dependency. A
domain class that imports and calls an api helper therefore yields two violations (the `IMPORTS`
edge from the file and the `CALLS` edge from the method), each with its own file and line.

Edges between two symbols of the same layer, and edges with an endpoint outside every layer, are
never violations.

## Rules
- `deny: [X, ...]` — a dependency `from → X` is a violation.
- `allow: [X, ...]` — any dependency `from → Y` with `Y` not listed (and not `from`) is a violation;
  `allow: []` means "no other layer".
- Several rules may share a `from`; a dependency is reported once, against the first rule (in file
  order) it breaks.

Each violation carries the rule index and description, both layers, and the exact symbol-level
edge: `from`, `to`, kind, evidence class, file, line and extractor rule.

## Delta semantics (base → head)
Both revisions are evaluated with the **same configuration** (base's). Each violation is keyed by
`(from symbol id, to symbol id, edge kind)`. Symbol ids are line-free (docs/SPEC.md §2), so moving
code within a file or editing unrelated code does not create a "new" violation.

| Status | Meaning |
|---|---|
| `NEW` | key present in head, absent in base |
| `PRE_EXISTING` | present in both |
| `REMOVED` | present in base, absent in head |

Identity across revisions: before comparing, base ids are mapped through the change
classification's identities — a `SIGNATURE_CHANGED` method's previous id maps to its new id, and a
deleted symbol maps to the added symbol reported as its probable move (identical fingerprint). A
violation carried along by such a change stays `PRE_EXISTING`, with the original base edge in
`base_edge`. Renames without an identical body are not identified and appear as removed + new.

By default only the delta is the change author's concern: the default policy warns on
`new_architecture_violation` and does not evaluate pre-existing debt. List
`architecture_violation` in a policy to gate on every violation present in head.

A new violation whose edge is not `RESOLVED_EXACT` (an inferred call, for example) can *warn* but
never *fail*: when every new violation rests on inferred evidence, a failing gate is downgraded to a
warning and says why.

## Cycles
The layer graph has an edge `A → B` when any counted dependency goes from layer A to layer B.
Cycles are the strongly connected components with more than one layer (Tarjan; nodes and
neighbours visited in sorted order, so the output is deterministic). A cycle is identified by its
sorted set of layers; one present in head but not base is `NEW`. Each cycle lists its layer edges
with the number of symbol-level dependencies behind each and the first one as an example.

- `cycles: off` — not computed; cycle gates and signals are "not evaluated".
- `cycles: warn` — computed and reported; `new_cycle` follows the policy lists.
- `cycles: forbid` — as warn, and `new_cycle` always fails.

## Direction and coupling
`layer_dependencies` lists every layer pair with dependencies in either revision and the counts in
each; `direction_reversed` marks a pair `A → B` that is new in head while base only had `B → A`.
`module_coupling` lists module pairs (Java package, TS directory) whose cross-module dependency
count changed, largest change first (at most 100); it is computed even without layers.

## Single-revision check
`ripplepath architecture check --repo . --rev HEAD [--format json]` evaluates one revision against
**that revision's** `ripplepath.yml` (an invalid file is an error, exit 1). Findings describe the
state; in JSON they carry the status `PRE_EXISTING`, because the state is compared with itself.

## Merge policy gates
| Gate | Triggered when |
|---|---|
| `new_architecture_violation` | a `NEW` violation exists (fails only on `RESOLVED_EXACT` evidence) |
| `architecture_violation` | any violation exists in head |
| `new_cycle` | a `NEW` layer cycle exists |
| `parse_failure_in_changed_file` | a changed file has a high-severity syntax error, parse failure or exceeds the size limit |
| `removed_test_on_critical_path` | a deleted test unit lies in a critical path, or in base reached a critical-path symbol along dependencies (depth ≤ `--max-depth`) |
| `breaking_api_change` | public API removed, narrowed or re-signed (docs/RISK_MODEL.md) |
| `config_changed` | head adds, modifies or removes `ripplepath.yml` |
| `config_invalid` | base or head `ripplepath.yml` is invalid — always fails, not configurable |

Status per gate: `PASS`, `WARN`, `FAIL`, `OFF` (triggered but not enforced) or `NOT_EVALUATED`
(no layers / no critical paths configured). The result is `FAIL` if any gate fails, else `WARN` if
any warns, else `PASS`. `ripplepath analyze --fail-on-policy` exits with status 2 on `FAIL`
(0 success, 1 error). The risk score is never a gate (docs/RISK_MODEL.md).

## CODEOWNERS
Read from the **head** tree: the first of `.github/CODEOWNERS`, `CODEOWNERS`, `docs/CODEOWNERS`
(GitHub's order). Files above 3 MB are ignored, as GitHub does. Semantics:
- `#` starts a comment (also after the owners); blank lines are ignored;
- the **last** matching line wins; a line with a pattern and no owners makes matching files unowned;
- a pattern with a leading or middle `/` is anchored to the root; otherwise it matches at any depth;
- a trailing `/` matches only the contents of matching directories; a pattern naming a directory
  without a trailing slash also owns its contents;
- `*` and `?` stay within one path segment, `**` spans segments, `dir/*` owns direct children only
  (GitHub documents this difference from gitignore);
- `!` negation and `[ ]` ranges are unsupported by GitHub; such lines, and lines with malformed
  owners, are reported as errors and skipped; matching is case-sensitive.

The report lists likely owners for changed files and for files of impacted symbols, a per-owner
summary, unowned changed files, and whether the change edits CODEOWNERS itself. **Ownership is
review-routing metadata, not authorization**: Ripplepath never decides who may approve anything,
and a change can edit the CODEOWNERS file it is reported against (`changed_in_head`).

## Limitations
- Layers are path-based; a type whose package does not match its directory is assigned by path.
- Only dependencies the frontends resolve are seen (docs/LANGUAGE_SUPPORT.md); reflection,
  dependency injection by name, and cross-language calls produce no edges, hence no violations.
- A dependency on an unresolved or external symbol is never a violation.
- Cycle identity is the set of layers: a cycle that gains a layer is reported as one removed and one
  new cycle.
