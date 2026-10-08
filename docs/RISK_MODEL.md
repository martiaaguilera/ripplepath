# Risk model

Current version: **1** (`risk.model_version`). Implemented in `crates/engine/src/risk.rs`, measured in
`crates/engine/src/assess.rs`.

**The risk score is not a probability of failure.** It is the sum of capped integer points from
named signals, meant to order review attention. None of its weights has been calibrated against
outcomes; there is no dataset behind them. The report says so in `risk.interpretation`, and every
point is traceable to a signal, its measured value and its evidence.

## Computation
For each signal: `units = scale(value)`, `points = min(units × weight, cap)`.
`score = min(Σ points, 100)`; `uncapped_total` keeps the sum before the cap.
A signal whose input is absent (no coverage ingested, no layers configured…) is reported with
`evaluated: false`, scores 0 and carries a note — missing evidence is shown, never hidden.

Scales: *linear* (one unit per counted item) or *banded* (units = number of thresholds reached).
Bands grow roughly geometrically (×3–×5 per band), a coarse logarithm: a large change scores more
than a small one, but size alone cannot dominate the total.

## Signals (model v1)

| id | value | scale | weight | cap | why |
|---|---|---|---|---|---|
| `public_api_changed` | breaking API changes (below) | linear | 8 | 24 | callers outside the repository can break without any trace in this graph |
| `migration_changed` | changed files classified MIGRATION | linear | 15 | 15 | schema changes are hard to roll back and invisible to code analysis |
| `build_or_dependency_changed` | changed BUILD, LOCKFILE, CI, CONTAINER or CONFIG files | linear | 5 | 10 | effects are not traced through code |
| `blast_radius` | impacted non-test symbols | bands 1/6/21/101 | 5 | 20 | more dependents, more ways to break |
| `changed_symbol_centrality` | most distinct direct dependents of one changed non-test symbol (impact-propagating edges, excluding TESTS; head graph, base for deleted, max of both for re-signed) | bands 3/10/30 | 5 | 15 | changing a hub is riskier than changing a leaf, even at equal blast radius |
| `critical_path_changed` | changed files matching `critical` that contain a changed symbol, or are not parsed source (SQL, config…) | linear | 10 | 20 | the team declared these paths critical; comment-only edits do not count |
| `new_architecture_violation` | `NEW` layer-rule violations | linear | 10 | 20 | drift is cheapest to stop when introduced |
| `new_layer_cycle` | `NEW` layer cycles | linear | 10 | 10 | cycles erase layering |
| `changed_code_without_coverage` | added/modified/re-signed non-test, non-generated symbols whose coverage status is NOT_COVERED or NO_DATA; evaluated only when coverage was ingested | linear | 3 | 15 | changed code no test was measured to execute |
| `flaky_impacted_tests` | recommended tests classified FLAKY; evaluated only when CI history exists for them | linear | 3 | 9 | their verdict on this change is weak |
| `deleted_tests` | deleted test units | linear | 5 | 15 | less protection after the change |
| `parser_uncertainty` | changed files with high-severity syntax error, parse failure or size limit | linear | 5 | 15 | the analysis may be missing changed symbols |
| `unresolved_references_in_changed_code` | listed unresolved references whose source is a changed symbol | linear | 1 | 5 | an impact edge may be missing |
| `config_changed` | 1 when head adds, modifies or removes `ripplepath.yml` | linear | 5 | 5 | rules for future changes are being edited |

Maximum before the total cap: 198. Signals are not independent — one `import` from domain into api
can produce a new violation *and* close a new layer cycle — and the per-signal caps are what keep
one finding from dominating.

### Public API surface (§24)
A symbol is API when it is not test code (no `is_test` symbol in its chain, not in a test file), and
it and every enclosing declaration up to the file are `public` or `protected`, with the outermost
`public`. Java: public/protected members of public (or public/protected nested) types; interface
members are implicitly public. TypeScript/JavaScript: exported module-level declarations (the
frontend records "exported" as `public`) and their non-private class/interface members.

Checks, per changed symbol:
- `REMOVED` — deleted, API in base; a removed member of a removed API type is not listed separately;
- `SIGNATURE_CHANGED` — classified `SIGNATURE_CHANGED` and the base symbol was API;
- `NARROWED` — modified, API in base and not API in head (visibility reduced, export removed);
- `ADDED` — added and API in head (top-most only); listed, never counted as breaking.

Not detected: return-type or thrown-exception changes (reported as `MODIFIED`), changes to generic
bounds, re-exports through barrels, semantic changes behind an unchanged signature.

## Levels
| Level | Score |
|---|---|
| LOW | 0–14 |
| MEDIUM | 15–39 |
| HIGH | 40–69 |
| CRITICAL | 70–100 |

## The score is not a gate
Merge policy gates evaluate concrete findings (docs/ARCHITECTURE_RULES.md). A threshold on an
uncalibrated sum would make an ordering aid an arbitrary blocker, so no gate uses the score.

## Versioning policy
Signal ids, definitions, scales, weights, caps and level bands belong to the model version. Any
change to them — including "small" weight tweaks — ships as a new `model_version` with an entry in
this document and in docs/ENGINEERING_LOG.md. No silent changes: two reports with the same
`model_version` are scored by the same rules.

## Worked example: `fixtures/java-banking` v1 → v2 (measured)
| Signal | Value | Points |
|---|---|---|
| public_api_changed | 3 (`legacyBalance` removed; `AccountRepository.findById`, `InMemoryAccountRepository.findById` re-signed) | 24 |
| migration_changed | 1 (`V2__account_frozen.sql`) | 15 |
| blast_radius | 3 impacted non-test symbols → band 1 | 5 |
| changed_symbol_centrality | 3 dependents of `AccountRepository#findById` → band 1 | 5 |
| critical_path_changed | 3 (`Account.java`, `StandardFeePolicy.java`, the migration; `Money.java` only gained a comment) | 20 (cap) |
| new_architecture_violation | 2 (`Account.java` imports `ApiErrors`; `Account#withdraw` calls `ApiErrors.accountFrozen`) | 20 (cap) |
| new_layer_cycle | 1 (api ↔ application ↔ domain ↔ persistence) | 10 |
| **score** | | **99, CRITICAL** |

`changed_code_without_coverage` and `flaky_impacted_tests` are not evaluated without ingested
evidence. Test: `banking_risk_decomposition_is_exact` in `crates/engine/tests/governance_analysis.rs`.
