### Index

| Workload | n | median ms | p95 ms | min ms | max ms | source files | parsed | reused | files/s | symbols | edges | symbols/s | delta symbols +/-/~ | delta edges +/-/~ | DB MiB | CPU ms (median) | peak RSS MiB (median) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| cold (empty database) | 5 | 646 | 778 | 640 | 778 | 260 | 260 | 0 | 403 | 4039 | 12883 | 6255 | 4039/0/0 | 12883/0/0 | 14.1 | 1703 | 42 |
| warm, unchanged revision | 5 | 245 | 270 | 241 | 270 | 260 | 0 | 260 | 1061 | 4039 | 12883 | 16476 | 0/0/0 | 0/0/0 | 14.1 | 250 | 48 |
| update: cb13484..3b9ec7d | 5 | 311 | 325 | 283 | 325 | 260 | 6 | 254 | 835 | 4039 | 12883 | 12969 | 19/0/62 | 87/5/230 | 14.4 | 312 | 51 |

### Index stages (one extra run per workload, ms)

| Workload | open | build_snapshot | indexed_graph | apply_graph | close | load_graph (separate call) |
|---|---|---|---|---|---|---|
| cold (empty database) | 14.6 | 379 | 9.7 | 210 | 3.5 | 0.0 |
| warm, unchanged revision | 9.1 | 159 | 9.8 | 59.8 | 16.5 | 31.4 |
| update: cb13484..3b9ec7d | 7.5 | 171 | 10.6 | 62.8 | 16.8 | 26.5 |

### Analyze

| Workload | n | median ms | p95 ms | min ms | max ms | files changed | symbols changed | impacted | tests recommended | CPU ms (median) | peak RSS MiB (median) |
|---|---|---|---|---|---|---|---|---|---|---|---|
| cb13484..3b9ec7d, no database | 5 | 493 | 553 | 430 | 553 | 12 | 24 | 74 | 70 | 1516 | 46 |
| cb13484..3b9ec7d, warm database | 5 | 328 | 360 | 321 | 360 | 12 | 24 | 74 | 70 | 344 | 46 |

### Graph

| Symbols | Edges | load_graph ms | CodeGraph::new median ms | p95 ms | queries | query p50 µs | p95 µs | max µs | mean impacted | max impacted | truncated | peak RSS MiB |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 4039 | 12883 | 27.3 | 8.7 | 9.8 | 1000 | 9.7 | 376 | 1172 | 16.1 | 173 | 0 | 42 |

### Machine load

- suite start: "cpu load 27%, 0 rustc/cargo processes"
- after "index" / "cold (empty database)": "cpu load 4%, 0 rustc/cargo processes"
- after "index" / "warm, unchanged revision": "cpu load 5%, 0 rustc/cargo processes"
- after "index" / "update: cb13484..3b9ec7d": "cpu load 3%, 0 rustc/cargo processes"
- after "analyze" / "cb13484..3b9ec7d, no database": "cpu load 9%, 0 rustc/cargo processes"
- after "analyze" / "cb13484..3b9ec7d, warm database": "cpu load 10%, 0 rustc/cargo processes"
- after "graph" / "1000 random single-symbol roots": "cpu load 12%, 0 rustc/cargo processes"
