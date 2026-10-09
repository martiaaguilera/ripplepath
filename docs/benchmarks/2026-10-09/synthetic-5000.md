### Index

| Workload | n | median ms | p95 ms | min ms | max ms | source files | parsed | reused | files/s | symbols | edges | symbols/s | delta symbols +/-/~ | delta edges +/-/~ | DB MiB | CPU ms (median) | peak RSS MiB (median) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| cold (empty database) | 5 | 3400 | 3523 | 3333 | 3523 | 5000 | 5000 | 0 | 1471 | 41837 | 105213 | 12305 | 41837/0/0 | 105213/0/0 | 68.7 | 7719 | 160 |
| warm, unchanged revision | 5 | 1354 | 1379 | 1348 | 1379 | 5000 | 0 | 5000 | 3693 | 41837 | 105213 | 30897 | 0/0/0 | 0/0/0 | 68.7 | 1359 | 219 |
| update: 1 file | 5 | 1408 | 1446 | 1383 | 1446 | 5000 | 1 | 4999 | 3551 | 41837 | 105213 | 29713 | 0/0/6 | 0/0/6 | 68.7 | 1344 | 220 |
| update: 100 files | 5 | 1524 | 3053 | 1514 | 3053 | 5000 | 100 | 4900 | 3282 | 41837 | 105213 | 27460 | 0/0/705 | 0/0/810 | 69.4 | 1562 | 226 |

### Index stages (one extra run per workload, ms)

| Workload | open | build_snapshot | indexed_graph | apply_graph | close | load_graph (separate call) |
|---|---|---|---|---|---|---|
| cold (empty database) | 14.7 | 2124 | 74.5 | 1169 | 9.6 | 0.0 |
| warm, unchanged revision | 7.4 | 818 | 75.3 | 375 | 67.2 | 191 |
| update: 1 file | 7.9 | 830 | 71.9 | 388 | 66.1 | 184 |
| update: 100 files | 7.7 | 899 | 74.1 | 483 | 4.1 | 191 |

### Analyze

| Workload | n | median ms | p95 ms | min ms | max ms | files changed | symbols changed | impacted | tests recommended | CPU ms (median) | peak RSS MiB (median) |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 file, no database | 5 | 2102 | 2144 | 1903 | 2144 | 1 | 1 | 9 | 2 | 6531 | 182 |
| 1 file, warm database | 5 | 1628 | 1694 | 1608 | 1694 | 1 | 1 | 9 | 2 | 1625 | 206 |
| 100 files, no database | 5 | 2642 | 2709 | 2449 | 2709 | 100 | 100 | 4009 | 767 | 7188 | 187 |
| 100 files, warm database | 5 | 2110 | 2150 | 2104 | 2150 | 100 | 100 | 4009 | 767 | 2141 | 212 |

### Graph

| Symbols | Edges | load_graph ms | CodeGraph::new median ms | p95 ms | queries | query p50 µs | p95 µs | max µs | mean impacted | max impacted | truncated | peak RSS MiB |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 41837 | 105213 | 185 | 57.9 | 61.9 | 1000 | 18.1 | 874 | 8506 | 39.3 | 1485 | 0 | 190 |

### Machine load

- suite start: "cpu load 20%, 0 rustc/cargo processes"
- after "index" / "cold (empty database)": "cpu load 0%, 0 rustc/cargo processes"
- after "index" / "warm, unchanged revision": "cpu load 1%, 0 rustc/cargo processes"
- after "index" / "update: 1 file": "cpu load 2%, 0 rustc/cargo processes"
- after "index" / "update: 100 files": "cpu load 7%, 0 rustc/cargo processes"
- after "analyze" / "1 file, no database": "cpu load 4%, 0 rustc/cargo processes"
- after "analyze" / "1 file, warm database": "cpu load 6%, 0 rustc/cargo processes"
- after "analyze" / "100 files, no database": "cpu load 4%, 0 rustc/cargo processes"
- after "analyze" / "100 files, warm database": "cpu load 1%, 0 rustc/cargo processes"
- after "graph" / "1000 random single-symbol roots": "cpu load 5%, 0 rustc/cargo processes"
