# ADR 0003 — Read Git through gix, never the git CLI

Status: accepted (2026-10-07)

## Context
The obvious implementation shells out to `git diff -M`. On an untrusted repository the git CLI can
run arbitrary programs from repository config (`core.fsmonitor`, `diff.external`, textconv drivers,
filters), and its output depends on the user's global config.

## Decision
Open repositories with `gix` using `open::Options::isolated()`, read trees and blobs from the object
database, and compute line diffs (imara-diff, histogram) and renames (exact blob, then line-Dice ≥ 50%)
ourselves.

## Consequences
- No execution vector through Git; results independent of the user's git config.
- We own rename detection semantics (documented, deterministic, bounded).
- The `git` binary is used only by the fixture builder, on Ripplepath's own fixtures.
