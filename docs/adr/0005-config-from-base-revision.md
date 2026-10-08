# ADR 0005 — Configuration is read from the base revision

Status: accepted (2026-10-08)

## Context
`ripplepath.yml` declares layer rules, critical paths and merge-policy gates. A pull request is
analysed as base → head. If the head revision's configuration were applied, a change could delete
the rule it violates, or move a gate from `fail_on` to nothing, and pass its own check. The working
tree is no better: in CI it is the head checkout, locally it may contain uncommitted edits, and
reading it would make results depend on the machine.

## Decision
- The configuration is read from the **base revision's tree** through the git crate's object access
  (no working tree, no `git` executable, nothing executed). Bounded to 64 KiB before parsing.
- The same base configuration evaluates both the base and the head graphs, so the architecture delta
  compares like with like.
- Head's file is read and validated too, but never applied. When head adds, modifies or removes it,
  the report says so (`config.head_change`), the `config_changed` gate fires and the
  `config_changed` risk signal scores 5 points. New rules take effect after merge.
- An invalid base file does not abort the analysis: defaults are used, the errors are reported and
  the non-configurable `config_invalid` gate fails. An invalid head file also fails it — that only
  makes a change stricter, never weaker. Single-revision `architecture check` reads that revision's
  own file and treats an invalid file as an error.
- No file → built-in defaults: no layers, no critical paths, all configurable gates except
  `architecture_violation` at warn.

## Consequences
- A PR that introduces a configuration is analysed without it; its effect is visible from the next
  change on. Reviewers see `head_change: ADDED`.
- Bootstrapping a policy fix requires merging it first — intended: policy edits are reviewed like
  code, not applied by the change under review.
- CODEOWNERS is *not* policy in Ripplepath (ownership is metadata), so it is read from head; edits to
  it are flagged (`owners.changed_in_head`).
- Rejected: head configuration (self-weakening); working-tree configuration (non-deterministic,
  bypassable); a configuration flag on the command line as the default (CI and local runs would
  diverge).
