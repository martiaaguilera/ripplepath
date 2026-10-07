# ADR 0001 — Rust for the analysis core

Status: accepted (2026-10-07)

## Context
The core parses many files, builds graphs, runs in CI and on developer machines, and must handle
hostile input with bounded memory. The author's first project (QuantaRun) is Java/Spring.

## Decision
Rust for every crate except the web UI. Tree-sitter has first-class Rust bindings; `gix` gives
pure-Rust Git object access; a single static binary simplifies CI use.

## Consequences
- Predictable memory and no GC pauses during large graph builds; easy bounded parallelism (rayon).
- Native toolchain needed on Windows (MSVC build tools, for tree-sitter's C grammars).
- Rejected: Java (would duplicate project #1's stack; JVM startup per CI invocation), TypeScript
  (weaker for CPU-bound parsing of large repos), Go (fine, but weaker tree-sitter and Git tooling).
