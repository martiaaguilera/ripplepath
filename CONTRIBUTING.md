# Contributing to Ripplepath

Thanks for looking. This file covers how to build and test, the project's non-negotiable rules, and
the two most common larger contributions: a language frontend and an evaluation or benchmark run.

## Build and test

Requirements: Rust stable ≥ 1.99 (MSVC build tools on Windows, for tree-sitter's C grammars), Git,
and Node 24 for `web/`.

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check

cd web
npm ci
npm run typecheck && npm run lint && npm test && npm run build
npx playwright test          # end-to-end; needs `ripplepath serve` running (see .github/workflows/ci.yml)
```

CI (`.github/workflows/ci.yml`) runs all of the above plus `cargo-deny` and `npm audit`. Re-run the
web checks after adding **any** file under `web/`: config files are typechecked too.

On Windows the cargo bin directory may be missing from `PATH` in a fresh shell. The repository pins
LF line endings (`.gitattributes`), because fixture blob ids depend on them.

## Rules (from CLAUDE.md)

- **Deterministic core.** Same inputs must give byte-identical output. Sort before serialising
  (`BTreeMap`, sorted `Vec`). `HashMap` iteration order must never reach output.
- **Never execute code from the analysed repository**: no builds, tests, scripts, hooks or filters.
  Git is read through `gix` object access, never the `git` CLI. The CLI is used only by the fixture
  builder, on Ripplepath's own fixtures.
- **Every edge carries evidence**: kind, evidence class, file, span, rule. Heuristic is not
  certain: anything not resolved by the language's own rules is `STATIC_INFERRED` or weaker.
- **Uncertainty broadens recommendations and is never hidden.** Unsupported constructs are
  documented in `docs/LANGUAGE_SUPPORT.md`, not silently skipped.
- **The risk score is a decomposition, not a probability.** Any change to signals, weights, caps or
  bands is a new `model_version` (docs/RISK_MODEL.md).
- **No invented numbers.** Benchmarks and evaluation results are measured or absent.
- **No AI-generated dependency facts.**
- **Incremental index output must equal a clean full re-index.** If you touch storage or
  extraction, `crates/engine/tests/incremental.rs` must still pass.
- **Bound everything**: file size, depth, response size, concurrency.
- Errors are typed with `thiserror` per crate. No `unwrap()`/`expect()` on production paths without
  an invariant comment. No `unsafe` (the workspace forbids it).
- Comments explain *why*, not *what*.
- TypeScript stays `strict`. The frontend renders the report and never re-implements a business
  rule.

## Commits and documentation

- [Conventional Commits](https://www.conventionalcommits.org) (`feat(lang): …`, `fix(engine): …`,
  `docs: …`), each a coherent change.
- When semantics or design change, update `docs/SPEC.md`, `docs/ARCHITECTURE.md` and
  `docs/ENGINEERING_LOG.md` (meaningful discoveries only, newest first). Significant decisions get an
  ADR in `docs/adr/`.
- A bug fix comes with a regression test that **fails without the fix**. Check it by reverting the
  fix; several tests in this repository were found to pass against the old code and had to be
  widened.

## Adding a language frontend

Frontends live in `crates/lang/src/<language>/` and follow the two-phase design (docs/ARCHITECTURE.md,
ADR 0002):

1. **`facts.rs`.** Serialisable per-file facts: declarations, references, imports, spans.
2. **`extract.rs`.** `extract(path: &str, source: &str, budget: Duration) -> Result<YourFile, ParseError>`
   (see `java::extract`), a pure function of one file. Use the shared helpers in `crates/lang/src/syntax.rs`: parsing under a time budget,
   iterative walks, and `fingerprint` (BLAKE3 over leaf tokens excluding comments and nested member
   spans). Keep recursion bounded, because hostile input will find any unbounded recursion.
3. **`resolve.rs`.** `resolve(files: &[&YourFile]) -> LanguageGraph`, a pure function of the snapshot. Produce
   symbols with line-free identities (docs/SPEC.md §2), edges with `kind`, `evidence`, file, span
   and a `rule` name, and `UnresolvedRef`s for references that *could* be in-repo but were not
   bound. Output must be sorted.
4. **`mod.rs`.** Export `extract`, `resolve` and an `EXTRACTOR_VERSION`. Bump it whenever
   extraction output for the same input can change, and update the compile-time assertion and the
   cache key in `crates/engine/src/snapshot.rs`.
5. **Wire it in.** Add a `Language` variant (`crates/core/src/symbol.rs`), the file-extension
   mapping and the extract/resolve dispatch in `crates/engine/src/snapshot.rs`, and the facts
   encoding for the cache.
6. **Tests.** Add a fixture under `fixtures/`, frontend tests in `crates/lang/tests/`, a hostile-input
   test (deep nesting, huge names), and an end-to-end analysis test in `crates/engine/tests/`.
   Determinism and incremental == clean must hold with the new language included.
7. **Documentation.** Add a section in `docs/LANGUAGE_SUPPORT.md` listing what is resolved and what
   is not, and the parser version.

Dogfood on a real repository written in the language before calling it done. For Java, fixtures
reported no unresolved references while a real Spring service had 2,478
(docs/DOGFOODING_QUANTARUN.md).

## Running the offline evaluation

`ripplepath evaluate` replays historical changes against recorded test outcomes
(docs/TEST_INTELLIGENCE.md §6). The pinned results come from `fixtures/eval-history` and are
asserted by `crates/engine/tests/evaluation.rs`. To run it by hand, follow
[docs/DEMO.md](docs/DEMO.md) ("Offline evaluation"): build a repository from the twelve snapshots,
ingest each snapshot's evidence oldest first, then

```bash
ripplepath evaluate --repo evalrepo --cases fixtures/eval-history/cases.json --db eval.db [--format json]
```

To re-collect the evidence itself (this compiles and runs only the fixture's own tests), run
`scripts/collect-eval-history.sh`. See `fixtures/eval-history/evidence/README.md` for tools,
versions and sanitisation. Results quoted anywhere must state the sample size.

## Running benchmarks

```bash
scripts/bench.sh [OUT_DIR]       # SIZES, SEED, ITERATIONS, QUERIES, REAL_REPO … (see the script header)
```

The harness (`crates/bench`) generates synthetic repositories deterministically from a seed and
times each run in a fresh process. It records host information next to the results. Published
numbers belong in `docs/BENCHMARKS.md` together with the hardware and command that produced them.
The `Benchmarks` workflow runs the same harness on demand and weekly. It is never a merge gate.

## Security issues

Do not open a public issue. See [SECURITY.md](SECURITY.md).
