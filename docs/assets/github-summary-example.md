## Ripplepath — Change Intelligence

**Policy: FAIL** · **Risk: CRITICAL (99/100)** — a decomposition of review signals (model v1), not a probability

Base `main~1` (`b0ec42b898`) → head `main` (`453551ec51`)

| Changed | Blast radius | Tests to run | Architecture | Coverage gaps |
|---|---|---|---|---|
| 15 symbol(s) in 10 file(s) | 4 symbol(s) in 4 module(s) | **FULL SUITE** (3 tests) | 2 new violation(s), 1 new cycle(s) | not measured (no coverage ingested) |

### Policy gates: FAIL

| Gate | Status | Detail |
|---|---|---|
| `new_architecture_violation` | **FAIL** | 2 new layer\-rule violation\(s\)\, 2 on exactly resolved edges |
| `architecture_violation` | off (not enforced) | 2 layer\-rule violation\(s\) in head \(2 new\, 0 pre\-existing\) |
| `new_cycle` | **FAIL** | 1 new layer cycle\(s\) |
| `parse_failure_in_changed_file` | pass | 0 changed file\(s\) not fully parsed |
| `removed_test_on_critical_path` | pass | 0 deleted test\(s\) on a critical path |
| `breaking_api_change` | **WARN** | 3 public API symbol\(s\) removed\, narrowed or with a changed signature |
| `config_changed` | pass | ripplepath\.yml is Unchanged in head\; this analysis used the base revision\'s rules |
| `config_invalid` | pass | configuration valid |

Gates evaluate concrete findings\; the risk score is never a gate\.

### Architecture: new violations

- `domain` → `api` breaks domain must not depend on api\, application\, persistence: `Account.java` —IMPORTS→ `api.ApiErrors` at `src/main/java/com/acme/bank/domain/Account.java:3` (RESOLVED_EXACT)
- `domain` → `api` breaks domain must not depend on api\, application\, persistence: `domain.Account#withdraw(Money)` —CALLS→ `api.ApiErrors#accountFrozen(String)` at `src/main/java/com/acme/bank/domain/Account.java:25` (RESOLVED_EXACT)
- new layer cycle: `api` ↔ `application` ↔ `domain` ↔ `persistence`

### Test selection (BALANCED mode): run the **FULL SUITE**

- **HIGH** `MIGRATION\_CHANGED`: src\/main\/resources\/db\/migration\/V2\_\_account\_frozen\.sql changed\; its effects are not traced through code

<details><summary>Recommended tests in run order (2)</summary>

- STRONG `application.TransferServiceTest#movesMoneyAndChargesFee()` — test changed
- MEDIUM `api.TransferControllerTest#returnsOkOnSuccessfulTransfer()` — depth 3, weakest evidence RESOLVED_EXACT

</details>

### Most important evidence path

Why `api.TransferControllerTest#returnsOkOnSuccessfulTransfer()` is selected: a chain of 3 dependency edge(s) from the change, weakest evidence RESOLVED_EXACT.

```text
changed       application.TransferService#load(String)
← CALLS       application.TransferService#transfer(String,String,Money)  (src/main/java/com/acme/bank/application/TransferService.java:18)
← CALLS       api.TransferController#transfer(String,String,String,String)  (src/main/java/com/acme/bank/api/TransferController.java:14)
← CALLS       api.TransferControllerTest#returnsOkOnSuccessfulTransfer()  (src/test/java/com/acme/bank/api/TransferControllerTest.java:21)
```

<details><summary>Changed symbols (15)</summary>

- modified `api.AccountController#balance(String)` (`src/main/java/com/acme/bank/api/AccountController.java:13`)
- deleted `api.AccountController#legacyBalance(String)` (`src/main/java/com/acme/bank/api/AccountController.java:18`)
- added `ApiErrors.java` (`src/main/java/com/acme/bank/api/ApiErrors.java:1`)
- added `api.ApiErrors` (`src/main/java/com/acme/bank/api/ApiErrors.java:3`)
- added `api.ApiErrors#<init>()` (`src/main/java/com/acme/bank/api/ApiErrors.java:4`)
- added `api.ApiErrors#accountFrozen(String)` (`src/main/java/com/acme/bank/api/ApiErrors.java:7`)
- modified `application.TransferService#load(String)` (`src/main/java/com/acme/bank/application/TransferService.java:27`)
- modified `Account.java` (`src/main/java/com/acme/bank/domain/Account.java:1`)
- added `domain.Account#frozen` (`src/main/java/com/acme/bank/domain/Account.java:8`)
- modified `domain.Account#withdraw(Money)` (`src/main/java/com/acme/bank/domain/Account.java:23`)
- added `domain.Account#freeze()` (`src/main/java/com/acme/bank/domain/Account.java:38`)
- modified `domain.StandardFeePolicy#feeFor(Money)` (`src/main/java/com/acme/bank/domain/StandardFeePolicy.java:4`)
- signature changed `persistence.AccountRepository#findById(String,boolean)` (`src/main/java/com/acme/bank/persistence/AccountRepository.java:7`)
- signature changed `persistence.InMemoryAccountRepository#findById(String,boolean)` (`src/main/java/com/acme/bank/persistence/InMemoryAccountRepository.java:11`)
- modified `application.TransferServiceTest#movesMoneyAndChargesFee()` (`src/test/java/com/acme/bank/application/TransferServiceTest.java:12`)

</details>

<details><summary>Impacted non-test symbols (3)</summary>

- depth 1 `application.TransferService#transfer(String,String,Money)` (weakest evidence RESOLVED_EXACT)
- depth 1 `domain.FeePolicy#feeFor(Money)` (weakest evidence RESOLVED_EXACT)
- depth 2 `api.TransferController#transfer(String,String,String,String)` (weakest evidence RESOLVED_EXACT)

</details>

<details><summary>Risk decomposition (99/100)</summary>

| Signal | Value | Points |
|---|---|---|
| `public_api_changed` | 3 | +24 |
| `migration_changed` | 1 | +15 |
| `blast_radius` | 3 | +5 |
| `changed_symbol_centrality` | 3 | +5 |
| `critical_path_changed` | 3 | +20 |
| `new_architecture_violation` | 2 | +20 |
| `new_layer_cycle` | 1 | +10 |

Not evaluated (input missing, scored 0): `changed_code_without_coverage`, `flaky_impacted_tests`

Risk score\: a sum of capped\, versioned signal points for prioritising review\. It is NOT a probability of failure and is not calibrated against outcomes\.

</details>

<details><summary>Uncertainty (1 item(s), 0 high)</summary>

- LOW `src/main/resources/db/migration/V2__account_frozen.sql` changed file is not in a supported language\; its effects are not traced

</details>

<sub>Ripplepath 0\.1\.0 · analysis schema 1 · decision support, not proof of safety · full report in <code>analysis.json</code></sub>
