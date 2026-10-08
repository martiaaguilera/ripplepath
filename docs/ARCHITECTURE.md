# Architecture

Status: Java and TypeScript/JavaScript frontends. Planned components are marked as such.

```
                      ┌──────────────────── ripplepath-engine ───────────────────┐
 Git repo ──► git ──► │ snapshot(base) ─┐                                         │
  (gix,      (trees,  │ snapshot(head) ─┼─► path diff + renames ─► symbol change  │──► AnalysisReport
  read-only)  blobs)  │   │  ▲ fact cache│                         classification  │    (analysis.json v1)
                      │   ▼  │ (blob,path)                              │          │        │
                      │  lang::java::extract ─► lang::java::resolve ─► graph::impact│        ├─► CLI text / JSON
                      └───────────────────────────────────────────────────────────┘        └─► server ─► web UI
```

## Crates

| Crate | Responsibility | I/O |
|---|---|---|
| `core` | `SymbolId`, `Symbol`, `Edge`, `EdgeKind`, `Evidence`, `Fingerprint`. Plain data with total orderings. | none |
| `git` | Open repo (isolated config), resolve revisions, list trees, read blobs with size cap, path validation, line diff, rename pairing. | object DB only |
| `lang` | Language frontends `java` and `ts`, each `extract` (per file, pure) + `resolve` (per snapshot, pure). Languages resolve independently; no cross-language edges. | none |
| `graph` | `CodeGraph` (sorted edges, adjacency indices) and `impact` (BFS with explaining paths). | none |
| `evidence` | Bounded parsers for JaCoCo XML, LCOV and JUnit XML (no entity expansion). Test evidence semantics: docs/TEST_INTELLIGENCE.md. | none |
| `storage` | SQLite: migrations, persistent fact cache, indexed graph updated by difference. | one DB file |
| `engine` | Orchestration: snapshots, fact cache, change classification, hunks, uncertainty, report. | via `git` |
| `cli` | `ripplepath analyze | serve | demo`. Text renderer neutralises terminal control characters. | stdout/files |
| `server` | Axum API `/api/v1/*` + static UI, Host validation, CSP, bounded analysis concurrency. | HTTP |
| `web/` | React + TypeScript strict UI. Renders the report; never infers relationships. | HTTP |

Planned: `risk`, `mcp`.

## Two-phase language frontends

`extract(path, source) → facts` depends only on the file, so its result is cached by
`(blob id, path)`; a file unchanged between base and head is parsed once. `resolve(&[facts]) →
symbols + edges` depends on the whole snapshot (imports, hierarchy) and is recomputed per snapshot.
This split is the basis for incremental indexing: parse cost is incremental, resolution is a cheap
pure pass. Making resolution itself incremental is deferred until measurements show it matters.

## Why base *and* head graphs

Most impact is computed on the head graph. Dependents of a **deleted** symbol only exist in the base
graph (in head the reference is either gone or now broken), so deleted roots are traversed there and
the result is labelled `graph: "base"`.

## Determinism

- All serialised collections are sorted (`BTreeMap`, sorted `Vec`); `HashMap` is used only for
  lookups that never reach output.
- Edge deduplication keeps (strongest evidence, earliest line, rule) — independent of visit order.
- Fixture repositories are built with fixed author/committer/dates, so commit ids are reproducible.
- Tested: same inputs ⇒ byte-identical JSON (`output_is_byte_identical_across_runs`), resolution
  independent of file order, impact independent of edge order (property test).

## Concurrency

Parsing is the only parallel stage (rayon, bounded by core count). The server runs analyses on
Tokio's blocking pool, at most 2 at a time; the permit is held inside the blocking task.

## Bounds

| Bound | Default | On hit |
|---|---|---|
| File size parsed | 1 MiB | `FILE_TOO_LARGE` uncertainty |
| Files per snapshot | 200 000 | analysis refused |
| Parse time per file | 2 s | `PARSE_FAILURE` uncertainty |
| Impact depth | 6 | configurable `--max-depth` (1–32) |
| Impacted symbols | 5 000 | `IMPACT_TRUNCATED` (high) |
| Graph slice nodes | 400 | `clamped: true`, UI notice |
| Type nesting / receiver chains | 32 / 64 | treated as unresolvable |
| Rename similarity pairs | 10 000 | `RENAME_DETECTION_SKIPPED` |
