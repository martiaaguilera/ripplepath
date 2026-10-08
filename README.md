# Ripplepath

**See what a code change can break before you merge it.**

Ripplepath maps a Git change to the *symbols* it touches, follows typed dependency edges outward,
and explains — with a source location for every step — which code and which tests sit in its blast
radius, and what it could not determine.

> **Status: early development.** Works end to end for **Java** and **TypeScript/JavaScript**: Git
> diff → symbol-level change set → evidence-backed impact paths → static test recommendations →
> CLI, JSON, local API and web UI, with a persistent incremental index and optional ingestion of
> JaCoCo/LCOV/JUnit evidence (coverage edges, flakiness, selection modes; see
> [docs/TEST_INTELLIGENCE.md](docs/TEST_INTELLIGENCE.md)). Architecture rules, the risk model and
> the GitHub Action are planned and not yet implemented. Nothing below claims otherwise.

## What it does today

```text
$ ripplepath demo
Files changed 10  |  symbols changed 15  |  impacted 4 across 4 module(s)  |  tests 2 of 3

Recommended tests (static evidence only; not a guarantee of coverage)
  application.TransferServiceTest#movesMoneyAndChargesFee()  — test itself changed
  api.TransferControllerTest#returnsOkOnSuccessfulTransfer()  — depth 3, weakest evidence ResolvedExact
      changed      application.TransferService#load(String)
      ← CALLS        application.TransferService#transfer(String,String,Money)  (…/TransferService.java:18)
      ← CALLS        api.TransferController#transfer(String,String,String,String)  (…/TransferController.java:14)
      ← CALLS        api.TransferControllerTest#returnsOkOnSuccessfulTransfer()  (…/TransferControllerTest.java:21)
```

- **Symbol-level change detection.** Each symbol has a fingerprint of its own tokens (comments and
  nested members excluded): comment-only edits are not changes, editing a method does not mark its
  class, and signature changes and probable moves are recognised.
- **Typed, evidence-carrying edges.** `CALLS`, `OVERRIDES`, `INSTANTIATES`, `REFERENCES`, `EXTENDS`,
  `IMPLEMENTS`, `IMPORTS`, each with an evidence class (`RESOLVED_EXACT`, `STATIC_INFERRED`, …), file,
  line and the resolver rule that produced it.
- **Explained blast radius.** Reverse reachability with one deterministic explaining path per
  impacted symbol, including dynamic dispatch through overridden methods and dependents of deleted
  symbols (traced on the base revision).
- **TypeScript too.** On the TypeScript fixture, a change to `applyDiscount` reaches the cart test
  through an interface:

  ```text
  cart.test.ts#test:Cart > totals line items  — depth 4, weakest evidence ResolvedExact
      changed      discount.ts#applyDiscount
      ← CALLS        pricing-service.ts#PricingService.quote  (src/pricing/pricing-service.ts:11)
      ← dispatched via OVERRIDES price-source.ts#PriceSource.quote  (src/pricing/pricing-service.ts:9)
      ← CALLS        cart.ts#Cart.total  (src/cart.ts:20)
      ← CALLS        cart.test.ts#test:Cart > totals line items  (src/cart.test.ts:14)
  ```
- **Uncertainty is output, not hidden.** Unresolved calls, syntax errors, oversized or unsupported
  changed files and truncated traversals are listed with severity.
- **Safe on untrusted repositories.** No repository code is executed; Git is read through `gix`, not
  the `git` CLI. See [SECURITY.md](SECURITY.md).

## Quick start

Requirements: Rust (stable, 1.99+), Git (only for the demo fixtures), Node 24 (for the web UI).

```bash
cargo build --release
./target/release/ripplepath demo                       # builds a Java fixture repo and analyses it
./target/release/ripplepath demo --fixture typescript-checkout --dir ts-demo
./target/release/ripplepath analyze --repo . --base main --head HEAD --format json > analysis.json
./target/release/ripplepath index --repo . --rev HEAD   # persistent, incremental index (user cache dir)

cd web && npm ci && npm run build && cd ..
./target/release/ripplepath serve --repo ripplepath-demo --base main~1 --head main
# open http://127.0.0.1:7878
```

## For coding agents (MCP)

`ripplepath mcp --repo .` serves read-only [Model Context Protocol](https://modelcontextprotocol.io)
tools over stdio, so an agent can ask *before* editing: what depends on this method (`what_if`), how
are these two symbols connected (`dependency_path`), which tests should run and does this change
break an architecture rule (`tests_for_change`, `architecture_status`). Answers carry the same
evidence (edge kind, evidence class, file:line) and uncertainty as the report. Claude Code:
`claude mcp add ripplepath -- ripplepath mcp --repo .`. Details and security model:
[docs/MCP.md](docs/MCP.md).

## How it works

1. Resolve `base` and `head` to trees; list files; pair renames (exact blob, then ≥ 50% line similarity).
2. Parse Java and TypeScript/JavaScript with tree-sitter into per-file facts (cached by blob id),
   then resolve names (imports, re-exports, declared types, hierarchies) across the snapshot.
3. Classify symbols of changed files as added / deleted / modified / signature-changed.
4. Traverse dependents breadth-first with bounded depth; keep the shortest, strongest-evidence path.
5. Recommend tests that changed or have a static path to a change; report everything uncertain.

Details: [docs/SPEC.md](docs/SPEC.md), [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md),
[docs/LANGUAGE_SUPPORT.md](docs/LANGUAGE_SUPPORT.md), [docs/RESEARCH.md](docs/RESEARCH.md).

## Known limitations

- Static resolution is not a compiler: overloads are matched by arity, generics are erased, lambda
  parameters are untyped. Ambiguity yields `STATIC_INFERRED` edges or reported unresolved calls.
- Reflection, dependency injection and generated code create runtime dependencies that static
  analysis cannot see.
- Test recommendations are *decision support*: a static path shows a test can reach a change, not that
  it exercises it, and passing tests do not make a change safe to merge.
- Java and TypeScript/JavaScript only, with no cross-language edges. TypeScript `paths` aliases and
  CommonJS are not resolved yet.

## Roadmap (not implemented yet)

Architecture rules and base/head drift · versioned deterministic risk decomposition ·
GitHub Action with step summary and SARIF · offline selection evaluation ·
benchmarks.

## Author

Martí Aguilera — [GitHub](https://github.com/martiaaguilera) ·
[LinkedIn](https://www.linkedin.com/in/martiaaguilera/)

Licensed under the [MIT License](LICENSE).
