# ADR 0004 — SQLite: persistent fact cache plus a graph updated by difference

Status: accepted (2026-10-07)

## Context
Re-parsing a whole repository on every run wastes the dominant cost. Resolution, however, depends on
every file (imports, hierarchies): a change to one file can change edges in files that did not
change. Making resolution itself incremental would require dependency tracking between files —
real complexity, with a correctness risk the project's core invariant exists to catch.

## Decision
- SQLite (bundled, single file, migrations via `PRAGMA user_version`).
- **Facts are incremental**: per-file extraction results are cached by (blob id, path, extractor
  version). Only files whose content changed are parsed.
- **Resolution is recomputed** for the whole snapshot from cached facts — a pure, in-memory pass.
- **The stored graph is updated by difference**: removed symbols/edges/unresolved rows are deleted,
  changed ones replaced, unchanged rows untouched; the run reports the delta.
- Mandatory invariant, tested on fixtures and on random edit histories: after any sequence of index
  runs, the stored index equals a clean index of the final revision.

## Consequences
- One-file updates parse one file; measured numbers are in docs/BENCHMARKS.md.
- Resolution cost grows with repository size on every run. If profiling shows it dominates, the
  next step is per-file dependency tracking for resolution, guarded by the same invariant test.
- Fact payloads are serialized records (an opaque cache); everything queried is in structured
  tables (`symbols`, `edges`, `unresolved`, `indexed_files`).
- Rejected: a graph database (no need for traversal at the storage layer; traversal runs in memory);
  per-revision graph copies (storage grows with history for no query that needs it).
