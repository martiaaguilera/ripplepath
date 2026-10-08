# Security model

Status: controls listed here are implemented and tested unless marked *planned*.

## Assets and trust boundaries
- **Analysed repository**: untrusted. Paths, file contents, identifiers, Git config and object sizes
  may be adversarial.
- **Revision specs**: come from the CLI user or HTTP requests.
- **Host machine**: must not run repository code; other local repositories must not be readable
  through the server.
- **Viewer** (terminal, browser): must not be attacked by text that originated in the repository.

## Threats and controls

| Threat | Control | Verified by |
|---|---|---|
| Code execution via repo config (`core.fsmonitor`, `diff.external`, textconv, hooks) | Git read through `gix` object access only; repo opened with `isolated` permissions (no global/system config, no env overrides). The `git` binary is run only by the fixture builder on Ripplepath's own fixtures. | design; `crates/git` has no process spawning |
| Path traversal / malicious filenames in trees | Tree paths validated: UTF-8, relative, no `.`/`..`/empty components, no `.git` component (any case), no control chars, no backslashes, no drive prefixes. Rejected entries are reported. | `git::path` tests |
| Symlink escape | Symlinks and submodules are never followed; reported as such. | `IndexStatus::Symlink` |
| Repository bomb (huge blob) | Blob size read from the object header *before* inflating; above 1 MiB not loaded. | `Repo::read_text` |
| Huge snapshots | Hard file-count limit (200k). | `AnalysisError::TooManyFiles` |
| Parser DoS (pathological input) | Per-file parse time budget; failure recorded as uncertainty. | `syntax::parse` |
| Stack exhaustion from deep nesting | Iterative tree walks; recursion bounded (types 32, type names/receivers/expression chains 64). | `hostile_nesting_does_not_overflow_the_stack` (Java and TS), `deep_qualified_names_in_extends_and_new_do_not_overflow` (both reproduced overflows before their fixes) |
| Exponential re-export resolution (`export *` barrels) | Per-query visited set over (module, name) plus memoised top-level results; chains capped at 16 hops. | `star_export_fan_out_is_not_exponential` (old resolver: >120 s timeout; new: 10 ms) |
| Quadratic work on large files (members, fingerprints, suite locals) | Hash-indexed member merging, file lookups and fingerprint exclusions; suite locals visible to each test capped at 256. | `many_members_and_suite_locals_stay_linear`, `large_classes_resolve_in_linear_time` |
| Terminal escape / Trojan-Source text | Text output escapes all control characters (except newline), Unicode Cf format characters and line/paragraph separators. | `cli::text` tests |
| XSS from identifiers in the UI | React escapes text; no `dangerouslySetInnerHTML`; strict CSP (`script-src 'self'`). | server header test |
| DNS rebinding against the local server | `Host` must be loopback (`localhost`, `127.0.0.1`, `[::1]`) or explicitly `--allow-host`ed. | `dns_rebinding_hosts_are_rejected` |
| Server reading arbitrary local repos | Repository fixed at startup; API accepts revisions only. | API design |
| Coding agent (MCP) reading other repositories or running commands | `ripplepath mcp` fixes the repository at startup; tools accept revision specs and symbol ids only (validated: printable, bounded length), never paths, commands or URLs; no process is spawned. stdio only, no listener. | `crates/mcp` protocol tests, ADR 0007 |
| Oversized or malformed MCP messages | Lines above 1 MiB are discarded without buffering; batches, bad ids and invalid JSON get JSON-RPC errors; requests are processed sequentially; results above 512 KiB are refused. | `oversized_lines_are_dropped_and_the_stream_resyncs`, `protocol_errors_use_json_rpc_codes` |
| Request floods | At most 2 concurrent analyses; permit held inside the blocking task so client disconnects cannot release it early. | design |
| Malformed revision input | 1–256 chars, no control characters; passed to gix rev-parse, never a shell. | API test |
| Malicious XML (coverage/JUnit) | *Planned* with ingestion: no DTD/entity expansion, size caps. | — |
| Forged index shipped inside an analysed repository | The default index lives in the user cache directory (keyed by canonical repo path), never in the working tree; the index is trusted local state. | `default_db` |
| Escape sequences in stored values reaching the terminal | Stored values in errors are escaped and truncated; all CLI error output is neutralised. | `corrupt`, CLI `main` |
| Source leakage via logs | Logs contain revision specs, counts and timings; never file contents. | review |

## Non-goals
- Sandboxing optional test/coverage execution (not implemented; any future feature will be
  explicit opt-in and containerised).
- Authentication for the local server (it binds to localhost by default and warns otherwise).
