### Index

| Workload | n | median ms | p95 ms | min ms | max ms | source files | parsed | reused | files/s | symbols | edges | symbols/s | delta symbols +/-/~ | delta edges +/-/~ | DB MiB | CPU ms (median) | peak RSS MiB (median) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| cold (empty database) | 5 | 15493 | 17250 | 15131 | 17250 | 20000 | 20000 | 0 | 1291 | 167400 | 421306 | 10805 | 167400/0/0 | 421306/0/0 | 276.2 | 33297 | 590 |
| warm, unchanged revision | 5 | 5976 | 12374 | 5736 | 12374 | 20000 | 0 | 20000 | 3347 | 167400 | 421306 | 28011 | 0/0/0 | 0/0/0 | 276.2 | 5953 | 835 |
| update: 1 file | 5 | 10441 | 13203 | 9803 | 13203 | 20000 | 1 | 19999 | 1916 | 167400 | 421306 | 16033 | 0/0/6 | 0/0/6 | 276.2 | 10188 | 836 |
| update: 100 files | 5 | 6008 | 7148 | 5775 | 7148 | 20000 | 100 | 19900 | 3329 | 167400 | 421306 | 27863 | 0/0/706 | 0/0/812 | 276.9 | 5828 | 840 |

### Index stages (one extra run per workload, ms)

| Workload | open | build_snapshot | indexed_graph | apply_graph | close | load_graph (separate call) |
|---|---|---|---|---|---|---|
| cold (empty database) | 20.9 | 9953 | 305 | 5482 | 32.9 | 0.0 |
| warm, unchanged revision | 17.9 | 7481 | 595 | 3179 | 234 | 1784 |
| update: 1 file | 8.8 | 5921 | 499 | 3705 | 161 | 1066 |
| update: 100 files | 7.0 | 3293 | 313 | 1872 | 3.7 | 756 |

### Analyze

| Workload | n | median ms | p95 ms | min ms | max ms | files changed | symbols changed | impacted | tests recommended | CPU ms (median) | peak RSS MiB (median) |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 file, no database | 5 | 8834 | 9850 | 8463 | 9850 | 1 | 1 | 3 | 2 | 27312 | 698 |
| 1 file, warm database | 5 | 6708 | 7258 | 6600 | 7258 | 1 | 1 | 3 | 2 | 6703 | 799 |
| 100 files, no database | 5 | 11810 | 14665 | 11389 | 14665 | 100 | 100 | 4479 | 775 | 30328 | 700 |
| 100 files, warm database | 5 | 11075 | 12809 | 9929 | 12809 | 100 | 100 | 4479 | 775 | 11047 | 800 |

### Graph

| Symbols | Edges | load_graph ms | CodeGraph::new median ms | p95 ms | queries | query p50 µs | p95 µs | max µs | mean impacted | max impacted | truncated | peak RSS MiB |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 167400 | 421306 | 973 | 259 | 261 | 1000 | 17.6 | 936 | 10229 | 42.7 | 1576 | 0 | 732 |

### Machine load

- suite start: "cpu load 6%, 0 rustc/cargo processes"
- after "index" / "cold (empty database)": "cpu load 5%, 0 rustc/cargo processes"
- after "index" / "warm, unchanged revision": "cpu load 17%, 0 rustc/cargo processes"
- after "index" / "update: 1 file": "cpu load 21%, 0 rustc/cargo processes"
- after "index" / "update: 100 files": "cpu load 4%, 0 rustc/cargo processes"
- after "analyze" / "1 file, no database": "cpu load 2%, 0 rustc/cargo processes"
- after "analyze" / "1 file, warm database": "cpu load 1%, 0 rustc/cargo processes"
- after "analyze" / "100 files, no database": "cpu load 20%, 0 rustc/cargo processes"
- after "analyze" / "100 files, warm database": "cpu load 13%, 0 rustc/cargo processes"
- after "graph" / "1000 random single-symbol roots": "cpu load 6%, 0 rustc/cargo processes"
