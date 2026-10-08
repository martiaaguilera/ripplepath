# Demo scripts

Two scripts: a five-minute technical walkthrough and a 60-second version for a non-specialist
audience. Every command below was run on 2026-10-08 (Windows 11, Git Bash, release build of this
repository) and every quoted output is from that run, abridged where marked `…`. Timings vary
between runs and machines; they are not benchmarks (see [BENCHMARKS.md](BENCHMARKS.md)).

## Preparation (before the audience arrives)

From the repository root:

```bash
cargo build --release
(cd web && npm ci && npm run build)
export PATH="$PWD/target/release:$PATH"          # so `ripplepath` is the binary just built
ripplepath demo > /dev/null                        # creates ./ripplepath-demo (java-banking fixture)
ripplepath index --repo ripplepath-demo --rev main~1 --db demo.db
ripplepath index --repo ripplepath-demo --rev main --db demo.db
```

`ripplepath demo` builds a Git repository from `fixtures/java-banking` with fixed author and dates,
so `main~1` is always `b0ec42b898` and `main` is `453551ec51`. The fixture is a small banking service
in four layers (`api`, `application`, `domain`, `persistence`) with a `ripplepath.yml`. Its one change
adds a frozen-account feature that cuts across those layers.

Keep a second terminal open for the server:

```bash
ripplepath serve --repo ripplepath-demo --db demo.db --base main~1 --head main
# http://127.0.0.1:7878
```

## Five-minute demo

### 0:00 — The question

> "Before I merge this change: what did it touch, what depends on that, which tests have evidence
> of exercising it, and did it break an architecture rule? Ripplepath answers that from Git objects
> alone. It never builds or runs the repository it analyses."

### 0:20 — Open the UI: what changed

Open <http://127.0.0.1:7878>. The **Change** view (key `1`) shows:

- The verdict strip: policy `FAIL`, risk `99/100 CRITICAL`, full-suite decision.
- 15 changed symbols in 10 files.
- The impact graph and the evidence inspector.

Points to make:

- The unit is **symbols**, not files. `Money.java` changed in the diff, but only a comment changed,
  so it is not in the changed-symbol list.
- `persistence.AccountRepository#findById(String,boolean)` is classified **signature changed**, not as
  a delete plus an add. The CLI shows `was …#findById(String)`.

Same data in the terminal:

```bash
ripplepath analyze --repo ripplepath-demo --base main~1 --head main
```

```text
Files changed 10  |  symbols changed 15  |  impacted 4 across 4 module(s)  |  tests 3 of 3

Changed symbols
  modified  api.AccountController#balance(String)  (src/main/java/com/acme/bank/api/AccountController.java:13)
  deleted   api.AccountController#legacyBalance(String)  (…/AccountController.java:18)
  …
  signature persistence.AccountRepository#findById(String,boolean)  (…/AccountRepository.java:7)
            was persistence.AccountRepository#findById(String)
```

### 1:00 — Blast radius and one dependency path

In the graph, select `api.TransferControllerTest#returnsOkOnSuccessfulTransfer()`. Its explaining
path is highlighted. Each hop has an edge kind, an evidence class and a file:line:

```text
changed      application.TransferService#load(String)
← CALLS        application.TransferService#transfer(String,String,Money)  (…/TransferService.java:18)
← CALLS        api.TransferController#transfer(String,String,String,String)  (…/TransferController.java:14)
← CALLS        api.TransferControllerTest#returnsOkOnSuccessfulTransfer()  (…/TransferControllerTest.java:21)
```

Points to make:

- Every hop is `RESOLVED_EXACT`, meaning the resolver followed Java's own rules over declarations in
  the repository. Anything weaker is labelled `STATIC_INFERRED`, never presented as certain.
- `domain.FeePolicy#feeFor(Money)` is impacted by a change to `StandardFeePolicy#feeFor`. Callers
  are bound to the interface, so traversal follows `OVERRIDES` *forward* to the interface method,
  then backwards to its callers.

### 1:45 — Recommended tests and their evidence

Press `3` for the **Tests** view.

```text
Test selection (Balanced mode): run the FULL SUITE
  HIGH MIGRATION_CHANGED: src/main/resources/db/migration/V2__account_frozen.sql changed; its effects are not traced through code
Recommended tests, in run order (decision support, not proof of safety)
  Strong  application.TransferServiceTest#movesMoneyAndChargesFee()   — test code changed
  Medium  api.TransferControllerTest#returnsOkOnSuccessfulTransfer() — depth 3, weakest evidence ResolvedExact
```

Points to make:

- The change adds a SQL migration, and no code edge can show what a schema change breaks. So the
  decision **widens** to the full suite. Uncertainty broadens the recommendation; it is never hidden.
- The ranked list is still shown, as the best place to start.
- Tiers come from the evidence on the path. With ingested JaCoCo or LCOV coverage, a measured `TESTS`
  hop appears, drawn dotted and green, alongside the static ones.

### 2:20 — Architecture violation

Press `5` for **Architecture** (the delta between base and head):

```text
Architecture: 2 new, 0 pre-existing, 0 removed violation(s); 1 new, 0 pre-existing cycle(s)
  New domain -> api: domain must not depend on api, application, persistence  Account.java -Imports-> api.ApiErrors  (…/domain/Account.java:3, ResolvedExact)
  New domain -> api: …  domain.Account#withdraw(Money) -Calls-> api.ApiErrors#accountFrozen(String)  (…/domain/Account.java:25, ResolvedExact)
  New cycle: api <-> application <-> domain <-> persistence
```

Points to make:

- One import from domain into api closes a four-layer cycle.
- Rules come from the **base** revision's `ripplepath.yml`, so the pull request cannot delete the
  rule it breaks.

Press `4` for **Risk & policy**. The score of 99 is a sum of capped signals: API change 24,
migration 15, critical paths 20, violations 20, cycle 10, blast radius 5, centrality 5. Say it out
loud: *it is not a probability*. The merge gate fails on the concrete violation, never on the score.

### 3:00 — What the pull request would show (GitHub-style summary)

```bash
ripplepath analyze --repo ripplepath-demo --base main~1 --head main --format markdown | head -12
```

```text
## Ripplepath — Change Intelligence

**Policy: FAIL** · **Risk: CRITICAL (99/100)** — a decomposition of review signals (model v1), not a probability

Base `main~1` (`b0ec42b898`) → head `main` (`453551ec51`)

| Changed | Blast radius | Tests to run | Architecture | Coverage gaps |
|---|---|---|---|---|
| 15 symbol(s) in 10 file(s) | 4 symbol(s) in 4 module(s) | **FULL SUITE** (3 tests) | 2 new violation(s), 1 new cycle(s) | not measured (no coverage ingested) |
```

```bash
ripplepath analyze --repo ripplepath-demo --base main~1 --head main --fail-on-policy > /dev/null; echo "exit $?"
# exit 2
```

The GitHub Action publishes that Markdown as the step summary and an optional PR comment. It also
produces annotations and SARIF, and the step fails with exit status 2. A full rendered example is in
[assets/github-summary-example.md](assets/github-summary-example.md).

### 3:40 — Make a one-file change and show the incremental index

```bash
cd ripplepath-demo
sed -i 's/return new Money(amount.subtract(other.amount), currency);/return new Money(this.amount.subtract(other.amount), currency);/' \
  src/main/java/com/acme/bank/domain/Money.java
git commit -qam "refactor: spell out this.amount in Money#minus"
cd ..
ripplepath index --repo ripplepath-demo --rev main --db demo.db
```

Before the edit, from the preparation step (`main~1` into an empty database, then `main` on top of
it) and a third run with nothing changed:

```text
Indexed main~1 (b0ec42b898) into demo.db
  files 15  |  parsed 13  |  reused from cache 0
  symbols 65 (+65 -0 ~0)  |  edges 178 (+178 -0 ~0)
Indexed main (453551ec51) into demo.db
  files 17  |  parsed 9  |  reused from cache 5
  symbols 70 (+8 -3 ~22)  |  edges 185 (+16 -9 ~32)
Indexed main (453551ec51) into demo.db
  files 17  |  parsed 0  |  reused from cache 14
  symbols 70 (+0 -0 ~0)  |  edges 185 (+0 -0 ~0)
```

After the one-file commit:

```text
Indexed main (16ba879dfd) into demo.db
  files 17  |  parsed 1  |  reused from cache 13
  symbols 70 (+0 -0 ~1)  |  edges 185 (+0 -0 ~0)
```

Points to make:

- One file was parsed. One symbol row changed (its fingerprint), and no edge did.
- Resolution still ran over the whole snapshot. Only *facts* are incremental, and the stored graph is
  updated by difference.
- The invariant "incremental index == clean re-index" is a property test over random edit
  histories. It was mutation-checked: disabling stale-edge deletion makes it fail.

The new change, analysed:

```bash
ripplepath analyze --repo ripplepath-demo --base main~1 --head main --db demo.db
```

```text
Files changed 1  |  symbols changed 1  |  impacted 6 across 3 module(s)  |  tests 3 of 3
Changed symbols
  modified  domain.Money#minus(Money)  (src/main/java/com/acme/bank/domain/Money.java:24)
Test selection (Balanced mode): run 3 of 3 tests
  Medium  domain.MoneyTest#subtractingMoreThanBalanceIsNegative()  — depth 1
  Medium  application.TransferServiceTest#movesMoneyAndChargesFee() — depth 3
  Medium  api.TransferControllerTest#returnsOkOnSuccessfulTransfer() — depth 4
```

(The fixture has only three tests, so a deep change reaches all of them. The point here is the
explaining paths, not the reduction.)

### 4:20 — Offline evaluation: is the selection any good?

`fixtures/eval-history` holds twelve Java snapshots plus the JaCoCo and JUnit output of really
running their tests at each snapshot. Build a repository from them, ingest the evidence oldest first,
and replay:

```bash
mkdir evalrepo && git -C evalrepo init -q -b main
for v in $(seq 1 12); do
  find evalrepo -mindepth 1 -maxdepth 1 ! -name .git -exec rm -rf {} +
  cp -r fixtures/eval-history/v$v/. evalrepo/
  git -C evalrepo -c core.autocrlf=false add -A
  git -C evalrepo commit -qm "v$v"
done
for v in $(seq 1 12); do
  rev="main~$((12 - v))"
  for f in fixtures/eval-history/evidence/v$v/coverage/*.xml; do
    ripplepath ingest coverage "$f" --format jacoco --test "$(basename "$f" .xml)" --repo evalrepo --rev "$rev" --db eval.db > /dev/null
  done
  ripplepath ingest junit fixtures/eval-history/evidence/v$v/junit/run-1.xml --repo evalrepo --rev "$rev" --db eval.db > /dev/null
done
ripplepath evaluate --repo evalrepo --cases fixtures/eval-history/cases.json --db eval.db
```

```text
Offline evaluation: 11 case(s), 5 with failing tests, 15 failing test(s)
  small sample — not statistically meaningful

mode                   recall  missed  full suite  mean tests saved        mean time saved         mean first failure
conservative     15/15 1.0000       0        2/11            0.4554      0.4528 (11 cases)    #1.6, 12.0 ms (5 cases)
balanced         10/15 0.6667       5        0/11            0.6372      0.6346 (11 cases)    #1.8, 14.0 ms (4 cases)
fast_feedback    10/15 0.6667       5        0/11            0.6372      0.6346 (11 cases)    #1.8, 14.0 ms (4 cases)
…
  v6..v7  [5 failing]
    conservative   20/20 FULL caught 5/5 first@1 after 4 ms
    balanced       0/20 caught 0/5
      missed java:com.acme.shop.pricing.TaxCalculatorTest#loadsRatesFromResource()
      …
```

Points to make:

- Each case sees only evidence recorded at its base or earlier. A leakage test proves that head-time
  evidence cannot change the result.
- The sample size is printed first, and the label "small sample" is attached automatically.
- All five misses come from one change to `tax-rates.properties`, a file the code reads at run time.
  That miss led to the `RESOURCE_CHANGED` fallback. Conservative mode's 15/15 is therefore not an
  independent result. Say so.

### 4:50 — Close

> "Every claim is a path with a file and line, uncertainty is part of the output, and the
> numbers I showed are measured on small fixtures with their caveats printed next to them."

Clean up: `rm -rf ripplepath-demo evalrepo demo.db eval.db`.

### Optional: a real project (QuantaRun)

From the dogfooding run ([DOGFOODING_QUANTARUN.md](DOGFOODING_QUANTARUN.md)); nothing in the clone is
built or executed:

```bash
git clone https://github.com/martiaaguilera/quantarun ../quantarun   # Windows: set core.longpaths first
ripplepath analyze --repo ../quantarun \
  --base cb13484adac69df8ab56934c12110a08c00b3edc --head 3b9ec7d129055a6ca5242df362b9389a4525a1df
```

```text
Files changed 12  |  symbols changed 24  |  impacted 74 across 2 module(s)  |  tests 70 of 337
```

The dogfooding report covers what is a true positive and what is not. In this range, 17 of 60
static-path tests were reached through sibling implementations of an interface.

## 60-second demo (for a recruiter or non-specialist)

Have the UI open on the java-banking change before starting.

1. **(0–10 s)** "When you change code, the hard question is what else it can break. Ripplepath reads
   the Git history and answers that, without running anything from the repository."
2. **(10–25 s)** Change view: "These 15 items are the functions and classes this change touched. The
   graph shows everything that depends on them." Click the controller test: "This is *why* the test
   matters: three calls, each with the exact file and line."
3. **(25–40 s)** Tests view: "It recommends tests and explains each one. Here it says to run
   everything, because a database migration changed and code analysis cannot see what that breaks.
   When it is unsure, it widens instead of guessing."
4. **(40–50 s)** Architecture view: "This change made the domain layer depend on the API layer. That
   breaks a rule the team wrote down, and the pull request would be blocked on that concrete finding,
   not on a score."
5. **(50–60 s)** "It is Rust and React. It runs locally, gives the same output for the same input,
   and has been tried on my other project, QuantaRun, a Java/Spring control plane. Doing that cut
   unresolved references from 2,529 to 120. Everything it claims is measured or labelled as
   uncertain."
