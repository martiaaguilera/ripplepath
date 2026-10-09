# Benchmarks

Measured on 2026-10-09 at commit `13399df` (clean worktree). Raw output of that run, unedited:
[`docs/benchmarks/2026-10-09/`](benchmarks/2026-10-09/). Every number below comes from those files;
ratios are computed from them. Nothing here compares Ripplepath with other tools.

## What is measured, and how

The harness is `crates/bench` (`ripplepath-bench`), driven by `scripts/bench.sh`.

- **Fresh process per timed run.** `suite` orchestrates; each timed run is a child process
  (`ripplepath-bench measure ...`), so "cold" is cold for in-process state and each run's peak RSS
  and CPU time are its own.
- **Timing covers the library call only** (`index_revision`, `analyze`, `load_graph`,
  `CodeGraph::new`, `impact`). Process start-up, repository generation and database preparation
  are outside every measured interval.
- **N = 5 runs per workload.** Tables report median, p95, min and max. Percentiles use the
  nearest-rank method, so every value was actually measured; with N = 5, p95 equals the max.
- **Cold index** uses a new database every run, after one discarded warm-up run that pays for the OS
  reading pack files and the binary from disk.
- **Incremental workloads** start each run from a copy of the database as it was after indexing the
  base revision.
- **Analyze, warm database** is primed with one untimed run, so both revisions' facts are cached.
- **Graph queries:** 1000 `impact` calls with default options, each from one random symbol (fixed
  seed), on the graph of the cold-indexed revision.
- **Index stages:** one extra run per workload, with `index_revision` rebuilt from its public parts
  and each stage timed separately. A single run, so it shows proportions, not exact costs.
- **Phase profile** (`profile`): parsing, fact encoding and decoding, resolution and graph
  construction, timed in memory on generated sources, without Git or SQLite.
- **Machine load:** after each workload the harness records CPU load and the number of running
  `rustc`/`cargo` processes. It was 0 for every workload in this run.

### Synthetic repositories

`crates/bench/src/synth.rs` deterministically generates Java and TypeScript repositories from a
seed (own SplitMix64 PRNG; no time, environment or hash-map order). Their shape is a layered
service codebase: 20-file packages alternating languages, an interface with two implementations
per package, services with 3 field dependencies each (30 % from earlier packages), and unit tests
for some services. Calls use constructs the frontends resolve exactly, so the graph is dense with
edges and has 0 unresolved references. Each repository has three commits: the original, a 1-file
edit and a 100-file edit. "update: 1 file" goes from the original to the first edit;
"update: 100 files" goes from the first edit to the second. Analyze uses the same pairs.

The file edited by the 1-file change depends on the generator, which is why its blast radius
differs between sizes (43, 9 and 3 impacted symbols).

### Reproduce

```bash
SIZES="1000 5000 20000" ITERATIONS=5 \
REAL_REPO=<path to a QuantaRun clone> REAL_BASE=cb13484 REAL_HEAD=3b9ec7d \
scripts/bench.sh
```

Other defaults used: `SEED=42`, `QUERIES=1000`, `PROFILE_SIZE=5000`. The script builds with
`cargo build --release --locked -p ripplepath-bench` and writes to `target/bench-results/`. The
harness only reads Git objects of the benchmarked repository and never runs its code.

## Environment

| | |
|---|---|
| Ripplepath | `13399dfcb36d998ae62a0de0a05435872aa400ea`, 0 dirty files, release profile |
| Toolchain | rustc 1.99.0 (b940084d7 2026-09-28) |
| CPU | Intel Core i7-6700HQ @ 2.60 GHz, 4 physical cores, 8 logical CPUs (x86_64) |
| RAM | 21,359,263,744 bytes (19.9 GiB) |
| OS | Microsoft Windows 11 Home 10.0.26200 |
| Started | 2026-10-09T13:52:26Z |

The CPU-load samples ranged from 0 % to 38 % (laptop background activity). No compiler was running
during any workload. Per-workload samples are listed in each raw `.md` file.

## Summary

Medians of 5 runs, in ms, except where noted. RSS is the median peak resident set of the runs.

| | 1,000 files | 5,000 files | 20,000 files | QuantaRun |
|---|---:|---:|---:|---:|
| Symbols / edges | 8,365 / 21,022 | 41,837 / 105,213 | 167,400 / 421,306 | 4,039 / 12,883 |
| Cold index | 693 | 3,400 | 15,493 | 646 (260 files) |
| Warm re-index, unchanged | 285 | 1,354 | 5,976 | 245 |
| Update, 1 file | 291 | 1,408 | 10,441 | 311 (6 files parsed) |
| Update, 100 files | 566 | 1,524 | 6,008 | n/a |
| Analyze 1 file, no database | 543 | 2,102 | 8,834 | 493 (12 files) |
| Analyze 1 file, warm database | 344 | 1,628 | 6,708 | 328 (12 files) |
| Graph query p50 / p95 (µs) | 17.3 / 934 | 18.1 / 874 | 17.6 / 936 | 9.7 / 376 |
| Peak RSS, cold / warm index (MiB) | 43 / 52 | 160 / 219 | 590 / 835 | 42 / 48 |
| Database size (MiB) | 13.6 | 68.7 | 276.2 | 14.1 |

## How it scales (synthetic)

From 1,000 to 20,000 files (20×):

- **Cold index** grows 22.4× (693 → 15,493 ms), about linearly. Throughput was 1,443, 1,471 and
  1,291 files/s. CPU time is about 2.2× wall time (e.g. 33,297 ms CPU for 15,493 ms wall at 20,000
  files), because parsing is the only parallel stage.
- **Warm re-index of an unchanged revision** grows 21.0× (285 → 5,976 ms) and is 39–41 % of a cold
  index at every size. Nothing is parsed, but resolution and graph construction run over the
  whole snapshot, and the stored graph is loaded and diffed, so this cost is linear in repository
  size, not in change size. CPU time is about equal to wall time.
- **1-file and 100-file updates** cost about the same as a warm re-index at 1,000 and 5,000 files
  (291 and 1,408 ms vs 285 and 1,354 ms). At 20,000 files the 1-file update median (10,441 ms) was
  slower than both the unchanged re-index (5,976 ms) and the 100-file update (6,008 ms). These data
  do not explain that; it parses one file and writes 6 changed symbols and 6 changed edges. Treat it
  as an open anomaly, not a property.
- **Analyze** grows 16.3× without a database (543 → 8,834 ms) and 19.5× with a warm one
  (344 → 6,708 ms). It builds both revisions' graphs, so it also scales with repository size.
  For the 1-file change, the warm database saves 23–37 % of wall time and about 75 % of CPU time.
- **Graph queries** are flat: p50 17.3–18.1 µs and p95 874–936 µs at all three sizes, because the
  traversal visits the impacted subgraph, not the whole graph. No query hit a bound (0 truncated).
  At 20,000 files the slowest query took 10.2 ms and the largest blast radius was 1,576 symbols.
- **Loading the graph** is not flat: `load_graph` took 57, 185 and 973 ms and `CodeGraph::new`
  15.7, 57.9 and 259 ms.
- **Memory and disk** are linear: peak RSS for a warm re-index is 52, 219 and 835 MiB; the database
  is 13.6, 68.7 and 276.2 MiB, about 1.7 KiB per symbol.

## What dominates

Index stages of the cold run (single extra run, ms):

| Files | open | build_snapshot | indexed_graph | apply_graph | close |
|---:|---:|---:|---:|---:|---:|
| 1,000 | 15.1 | 393 (60 %) | 15.2 | 233 (35 %) | 3.8 |
| 5,000 | 14.7 | 2,124 (63 %) | 74.5 | 1,169 (34 %) | 9.6 |
| 20,000 | 20.9 | 9,953 (63 %) | 305 | 5,482 (35 %) | 32.9 |

`build_snapshot` (read blobs, parse or fetch cached facts, resolve) takes about 60 % and
`apply_graph` (writing the graph delta to SQLite) about 35 %. The proportions hold across sizes.

In-memory phase profile at 5,000 files (3,170,986 source bytes, 8 threads), median of 5 runs:

| Phase | median ms | p95 ms |
|---|---:|---:|
| parse, sequential | 2,367 | 3,242 |
| parse, parallel (rayon) | 791 | 846 |
| encode facts (JSON) | 115 | 130 |
| decode facts (JSON) | 157 | 169 |
| resolve Java | 244 | 428 |
| resolve TypeScript | 228 | 471 |
| `CodeGraph::new` | 62 | 87 |

Parallel parsing is 3.0× faster than sequential on 4 cores / 8 threads. Parsing is the largest
single phase even in parallel. The encoded facts take 18,298,339 bytes, 5.8× the source size; that
payload is what the fact cache stores and decodes on a warm run.

## Real repository: QuantaRun

`cb13484..3b9ec7d`: 260 indexed source files at head, 12 files and 24 symbols changed. Result:
74 impacted symbols, 70 recommended tests.

- Cold index 646 ms (p95 778), warm unchanged 245 ms, incremental update 311 ms (6 files parsed,
  254 reused; +19 symbols, 62 modified; +87/−5/~230 edges).
- Analyze 493 ms without a database, 328 ms with a warm one.
- Graph: 4,039 symbols, 12,883 edges, 114 unresolved references (the synthetic graphs have 0);
  query p50 9.7 µs, p95 376 µs.
- Peak RSS 42–51 MiB; database 14.1 MiB.

QuantaRun has 2.4× more edges per file than the 1,000-file synthetic repository (49.6 vs 21.0).
Its cold index took about as long (646 vs 693 ms) with a quarter of the files, at 403 files/s
compared with 1,443. Synthetic throughput does not carry over to real code as files/s.

## Caveats

- One run on one laptop (4-core 2016 mobile CPU, Windows 11). Different hardware, OS or file system
  will give different numbers. Windows file and process overhead is part of every figure.
- Numbers vary between runs: max/median reached 2.6× (1,000 files, 100-file update) and 2.1×
  (20,000 files, warm re-index) in this run. The CPU-load samples show background activity.
- Synthetic code is regular, small-file and fully resolvable. Real code has other file sizes, more
  edges per file and unresolved references (114 on QuantaRun). Use the synthetic results
  for how costs scale, not as a prediction for a given repository.
- Five runs per workload is a small sample; p95 is the slowest run.
- Earlier CLI timings in [DOGFOODING_QUANTARUN.md](DOGFOODING_QUANTARUN.md) measured the whole
  `ripplepath` process, not the library call, so they are not comparable with these.
