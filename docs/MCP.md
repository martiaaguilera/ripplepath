# MCP server for coding agents

`ripplepath mcp` exposes Ripplepath's deterministic analysis to coding agents over the
[Model Context Protocol](https://modelcontextprotocol.io) (stdio). It is optional: it adds no
analysis of its own, and everything it answers is also available through `ripplepath analyze`.
Design and protocol choices: [ADR 0007](adr/0007-read-only-mcp.md).

The question it is built for: *"if I modify this method, what else should I inspect, which tests
should I run, and would this cross an architecture boundary?"* — asked before the change exists.

## Setup

Build the binary (`cargo build --release`) and put `target/release/ripplepath` on `PATH`.

```bash
ripplepath mcp --repo /path/to/repo [--db /path/to/index.db] [--base main] [--head HEAD]
```

- `--repo` is fixed for the life of the process; tool calls cannot change it.
- `--db` (optional) is the fact cache and test-evidence database written by `ripplepath index` and
  `ripplepath ingest`. Without it, the per-repository default is used if it exists. With ingested
  coverage, `TESTS` edges with `COVERAGE_OBSERVED` evidence join the graph.
- `--base` / `--head` are the revisions used when a call omits them (defaults `HEAD~1` / `HEAD`).

**Claude Code** — from the repository you work in:

```bash
claude mcp add ripplepath -- ripplepath mcp --repo .
```

or commit a project-scoped `.mcp.json`:

```json
{
  "mcpServers": {
    "ripplepath": {
      "type": "stdio",
      "command": "ripplepath",
      "args": ["mcp", "--repo", ".", "--base", "main"]
    }
  }
}
```

**Other MCP clients** use the same command and arguments in their stdio server configuration.
Logs go to stderr (`-v` / `RUST_LOG`); stdout carries protocol messages only.

**Committed revisions only.** Ripplepath reads Git objects, never the working tree. To ask about an
edit in progress, commit it (a local WIP commit is enough) and pass that revision, or use `what_if`
on the symbol you are about to change.

## Protocol

| | |
|---|---|
| Transport | stdio, one JSON-RPC 2.0 message per line; no network listener |
| Revisions | `2026-07-28` (stateless, per-request `_meta`, `server/discover`) and the handshake revisions `2025-11-25`, `2025-06-18`, `2025-03-26`, `2024-11-05` (`initialize`, `ping`) |
| Capabilities | `tools` only (`listChanged: false`) |
| Concurrency | sequential: one request at a time |
| Message limit | 1 MiB per incoming line (larger: `-32600`, line discarded) |
| Result limit | 512 KiB per tool result (larger: tool error asking for a smaller `limit`/`max_depth`) |

Errors: `-32700` parse error, `-32600` invalid request (batches, bad `jsonrpc`/`id`, oversized,
`tools/*` before `initialize` in the handshake revisions), `-32601` unknown method, `-32602` invalid
params or unknown tool or missing `_meta` fields, `-32022` unsupported `2026-07-28`-style version
(with `data.supported`). Problems with tool **arguments** — invalid revision, unknown symbol,
out-of-range limit, unknown argument name — are results with `isError: true` so the model can
correct the call. An unknown symbol error lists similar ids.

Every result has `structuredContent` (JSON) and the same JSON as a text block.

## Tools

All tools are read-only (`readOnlyHint: true`, `destructiveHint: false`, `openWorldHint: false`),
reject unknown arguments, and default revisions to the server's `--base` / `--head`.

| Tool | Arguments | Answer |
|---|---|---|
| `analyze_change` | `base?`, `head?`, `mode?` | Summary of the change: risk decomposition (contributing signals with evidence; **not a probability**), policy verdict and triggered gates, changed symbols (≤ 50), top impacted symbols with paths (≤ 25), test selection and tests with reasons (≤ 25), **new** architecture violations (≤ 20), API surface counts, uncertainty (≤ 30), `truncation` notices. |
| `impacted_symbols` | `base?`, `head?`, `limit?` (1–500, 50) | Dependents of the change, nearest first, each with `depth`, `root`, `weakest_evidence` and `path`. |
| `tests_for_change` | `base?`, `head?`, `mode?` | Test selection (`decision`, `fallback_reasons`, run `ordered`, unit counts, runtimes when known) and each test's reason, tier, history and path (≤ 100). |
| `architecture_status` | `rev?` **or** `base?`/`head?` | With `base`/`head`: violations and cycles **introduced** by the change, judged by the base revision's rules. With `rev`: all violations of that revision against its own `ripplepath.yml`. |
| `find_symbols` | `query`, `rev?`, `limit?` (1–100, 20) | Symbols whose id or name contains `query` (case-insensitive) or whose file is `query`, sorted by id. Use it to get exact ids. |
| `symbol_info` | `symbol`, `rev?` | Kind, location, module, layer, generated flag, coverage; incoming and outgoing edges (≤ 50 each) with evidence; architecture violations the symbol takes part in; tests with evidence of reaching it; unresolved references inside it. |
| `dependency_path` | `from`, `to`, `rev?`, `max_depth?` (1–16, 8) | Shortest chain of dependency edges `from → … → to` (CONTAINS excluded), strongest evidence among equals; if none, the reverse chain (`direction: "reverse"`); else `found: false` with an explicit note that absence is not proof. |
| `what_if` | `symbol`, `rev?`, `max_depth?` (1–12, 6), `limit?` (1–500, 50) | Impact of modifying a symbol that has not changed yet: dependents and tests to run with evidence paths, modules and layers reached, uncertainty. |

`mode` is `conservative`, `balanced` or `fast_feedback` (docs/TEST_INTELLIGENCE.md).

### Semantics of `what_if`

`what_if` runs the same impact traversal and test recommendation as `analyze` on the revision's
graph, treating the symbol as **modified** (same id). It therefore matches what `analyze` would
report once a body-only edit to that symbol is committed. It cannot know what a real edit adds:
a signature change or deletion additionally breaks direct dependents bound to the old signature, and
new calls the edit introduces are not in the graph yet. The answer says so in `note`.

### Example (java-banking fixture)

`what_if {"symbol": "java:com.acme.bank.domain.Money#minus(Money)", "limit": 3}` on the demo
repository (`ripplepath demo`), abridged:

```json
{
  "root": { "id": "java:com.acme.bank.domain.Money#minus(Money)", "kind": "method",
            "location": "src/main/java/com/acme/bank/domain/Money.java:24-27", "layer": "domain" },
  "summary": { "impacted": 6, "impacted_tests": 3, "tests_recommended": 3,
               "layers": ["api", "application", "domain"], "impact_truncated": false, "max_depth": 6 },
  "impacted_symbols": [
    { "id": "java:com.acme.bank.domain.Account#withdraw(Money)", "depth": 1,
      "weakest_evidence": "RESOLVED_EXACT",
      "path": [{ "symbol": "java:com.acme.bank.domain.Account#withdraw(Money)",
                 "edge": { "from": "java:com.acme.bank.domain.Account#withdraw(Money)",
                           "to": "java:com.acme.bank.domain.Money#minus(Money)", "kind": "CALLS",
                           "evidence": "RESOLVED_EXACT",
                           "file": "src/main/java/com/acme/bank/domain/Account.java", "line": 27,
                           "rule": "java.call.field" },
                 "via_dispatch": false }] },
    "…"
  ],
  "tests": [ { "id": "java:com.acme.bank.domain.MoneyTest#subtractingMoreThanBalanceIsNegative()",
               "reason": "STATIC_PATH", "tier": "MEDIUM", "depth": 1, "path": ["…"] }, "…" ],
  "uncertainty": [],
  "truncation": ["impacted_symbols: showing 3 of 6"],
  "note": "Hypothetical: computed on HEAD's graph as if the body of … were modified. …"
}
```

`dependency_path` from `TransferController#transfer(String,String,String,String)` to
`Money#minus(Money)` returns three `CALLS` hops (TransferController.java:14 →
TransferService.java:21 → Account.java:27), all `RESOLVED_EXACT`.

## Security model

- **Read-only.** No tool writes to the repository, runs a build, test, hook or script from it, or
  spawns any process. Git is read through `gix` object access (ADR 0003). The only write is the
  parsed-fact cache in the database chosen by `--db` (or the per-user default), as with `analyze`.
- **Fixed scope.** The repository is set at startup and validated then; callers pass revision specs
  and symbol ids only — never paths, commands or URLs. Revision specs: 1–256 printable characters,
  given to gix rev-parse (never a shell). Symbol ids: 1–2048 printable characters, looked up, never
  interpreted.
- **No network.** stdio only; there is no listener to reach from a browser or another host.
- **Bounded.** Message size, result size, list lengths, impact depth/size and path search are all
  capped; caps that bite are reported in `truncation` / `search_truncated` / `impact_truncated`.
- **Evidence, not opinions.** Answers are projections of deterministic engine output. No AI generates
  any fact; identical requests on identical revisions produce identical results.
- Repository content (identifiers, paths) appears in results. An agent that relays results to other
  systems relays that content; treat it as untrusted text, like any source file.
