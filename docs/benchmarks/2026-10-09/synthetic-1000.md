### Index

| Workload | n | median ms | p95 ms | min ms | max ms | source files | parsed | reused | files/s | symbols | edges | symbols/s | delta symbols +/-/~ | delta edges +/-/~ | DB MiB | CPU ms (median) | peak RSS MiB (median) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| cold (empty database) | 5 | 693 | 860 | 657 | 860 | 1000 | 1000 | 0 | 1443 | 8365 | 21022 | 12073 | 8365/0/0 | 21022/0/0 | 13.6 | 1578 | 43 |
| warm, unchanged revision | 5 | 285 | 319 | 275 | 319 | 1000 | 0 | 1000 | 3505 | 8365 | 21022 | 29323 | 0/0/0 | 0/0/0 | 13.6 | 281 | 52 |
| update: 1 file | 5 | 291 | 339 | 284 | 339 | 1000 | 1 | 999 | 3437 | 8365 | 21022 | 28749 | 0/0/6 | 0/0/6 | 13.6 | 297 | 52 |
| update: 100 files | 5 | 566 | 1455 | 421 | 1455 | 1000 | 100 | 900 | 1766 | 8365 | 21022 | 14774 | 0/0/692 | 0/0/784 | 14.4 | 562 | 57 |

### Index stages (one extra run per workload, ms)

| Workload | open | build_snapshot | indexed_graph | apply_graph | close | load_graph (separate call) |
|---|---|---|---|---|---|---|
| cold (empty database) | 15.1 | 393 | 15.2 | 233 | 3.8 | 0.0 |
| warm, unchanged revision | 8.1 | 170 | 13.9 | 73.3 | 14.8 | 38.9 |
| update: 1 file | 7.4 | 159 | 13.4 | 72.0 | 18.6 | 38.3 |
| update: 100 files | 10.0 | 349 | 33.0 | 125 | 73.4 | 47.6 |

### Analyze

| Workload | n | median ms | p95 ms | min ms | max ms | files changed | symbols changed | impacted | tests recommended | CPU ms (median) | peak RSS MiB (median) |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 file, no database | 5 | 543 | 635 | 417 | 635 | 1 | 1 | 43 | 5 | 1453 | 45 |
| 1 file, warm database | 5 | 344 | 383 | 314 | 383 | 1 | 1 | 43 | 5 | 359 | 50 |
| 100 files, no database | 5 | 574 | 736 | 477 | 736 | 100 | 100 | 1266 | 307 | 1594 | 49 |
| 100 files, warm database | 5 | 458 | 511 | 412 | 511 | 100 | 100 | 1266 | 307 | 469 | 54 |

### Graph

| Symbols | Edges | load_graph ms | CodeGraph::new median ms | p95 ms | queries | query p50 µs | p95 µs | max µs | mean impacted | max impacted | truncated | peak RSS MiB |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 8365 | 21022 | 57.4 | 15.7 | 23.2 | 1000 | 17.3 | 934 | 5892 | 31.0 | 598 | 0 | 44 |

### Machine load

- suite start: "cpu load 26%, 0 rustc/cargo processes"
- after "index" / "cold (empty database)": "cpu load 9%, 0 rustc/cargo processes"
- after "index" / "warm, unchanged revision": "cpu load 3%, 0 rustc/cargo processes"
- after "index" / "update: 1 file": "cpu load 3%, 0 rustc/cargo processes"
- after "index" / "update: 100 files": "cpu load 20%, 0 rustc/cargo processes"
- after "analyze" / "1 file, no database": "cpu load 38%, 0 rustc/cargo processes"
- after "analyze" / "1 file, warm database": "cpu load 6%, 0 rustc/cargo processes"
- after "analyze" / "100 files, no database": "cpu load 16%, 0 rustc/cargo processes"
- after "analyze" / "100 files, warm database": "cpu load 17%, 0 rustc/cargo processes"
- after "graph" / "1000 random single-symbol roots": "cpu load 3%, 0 rustc/cargo processes"
