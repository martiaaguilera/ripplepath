# ADR 0007 — A hand-written, read-only, dual-era MCP adapter

Status: accepted (2026-10-08)

## Context
Coding agents ask the questions Ripplepath already answers — "if I modify this method, what else
should I inspect, which tests should I run, would this cross an architecture boundary?" — but they
ask them through the Model Context Protocol, before a change exists. Ripplepath must stay useful
without any agent; MCP is one more adapter over the engine, like the CLI and the HTTP API.

Facts checked on 2026-10-08:
- The **current** MCP revision is `2026-07-28`. It is stateless: there is no `initialize`; every
  request carries `_meta["io.modelcontextprotocol/protocolVersion"]` and
  `_meta["io.modelcontextprotocol/clientCapabilities"]`, servers must implement `server/discover`,
  results carry `resultType`, and an unsupported version is error `-32022` listing supported ones.
- Most deployed clients still speak the handshake revisions (`2025-11-25` and earlier:
  `initialize` → `notifications/initialized` → `tools/list` / `tools/call`, `ping`). The spec
  defines a **dual-era** server that serves both, selected per request.
- The official Rust SDK `rmcp` is maintained and Apache-2.0 (allowed by `deny.toml`), at 3.5.1
  with three major versions and ~30 releases in the last seven months.

## Decision
- **Implement the protocol subset by hand** in `crates/mcp` (a few hundred lines of framing and dispatch):
  newline-delimited JSON-RPC 2.0 on stdio; legacy `initialize` (echoes `2025-11-25`, `2025-06-18`,
  `2025-03-26` or `2024-11-05`, else offers `2025-11-25`), `ping`, `tools/list`, `tools/call`;
  modern `server/discover` and stateless `tools/*` with `resultType` and `serverInfo` in `_meta`.
  Standard codes: `-32700` parse, `-32600` invalid request (also batches, oversized messages,
  legacy calls before `initialize`), `-32601` unknown method, `-32602` invalid params / unknown
  tool / missing modern `_meta`, `-32022` unsupported modern version. Invalid *tool arguments*
  (bad revision, unknown symbol, out-of-range limit) are tool results with `isError: true`, as the
  spec asks, so the model can read and correct them.
- **Why not `rmcp`**: the server needs a dozen messages; the SDK brings an async runtime, schema
  derivation and macro layers into a synchronous, sequential adapter, and its release cadence across
  the 2025 → 2026 protocol transition means tracking breaking SDK changes for features this server
  does not use. Owning the framing also lets the reader enforce the message-size bound before
  buffering. Revisit if Ripplepath ever needs HTTP transport, subscriptions or multi-round-trip
  requests.
- **Read-only and fixed scope**: the repository (and optional database) is chosen on the command
  line. Tools accept revision specs (validated like the HTTP API: 1–256 printable characters, given
  to gix rev-parse, never a shell) and symbol ids — never paths, commands or URLs. Nothing from the
  analysed repository is executed. Tool annotations say `readOnlyHint: true`,
  `destructiveHint: false`, `openWorldHint: false`. The only write is the fact cache in the
  database file the user chose, exactly as `analyze --db` does.
- **Bounded**: messages over 1 MiB are refused without being buffered; requests are processed one at
  a time; every list in a result is capped with an explicit `truncation` notice; serialized results
  over 512 KiB are refused with advice to narrow the query; impact depth and path search are bounded.
- **Evidence or nothing**: tool outputs are projections of engine types (`Hop`, `Edge`,
  `Uncertainty`, `TestRecommendation`, `Violation`) with their evidence class and `file:line`; no
  generated text beyond fixed explanatory notes. `what_if` is the engine's own impact traversal and
  test recommendation run from a hypothetical *modified* root (`explore::RevisionView::what_if`), so
  it agrees with what `analyze` reports once such a change is committed.

## Consequences
- Supporting a future protocol revision means editing `protocol.rs`, not upgrading a framework.
- Working-tree edits are invisible (Git objects only); agents must commit (or pass a commit) to
  analyse their change. Documented in docs/MCP.md.
- Up to four indexed revisions and four analysis reports are memoised per process, keyed by spec
  and resolved commit/tree; evidence ingested while the server runs is seen after those entries are
  evicted or the server restarts.
