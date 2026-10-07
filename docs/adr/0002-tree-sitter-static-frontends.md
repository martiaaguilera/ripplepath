# ADR 0002 — Tree-sitter frontends with our own resolver, not compilers or LSP

Status: accepted (2026-10-07)

## Context
Exact name binding needs a compiler (javac, tsc) or a language server. Both require a configured
build (classpath, tsconfig, dependencies) — which means running the repository's build tooling,
i.e. executing untrusted code, and minutes of setup per repository.

## Decision
Parse with tree-sitter and implement a documented static resolver per language. Every edge records
how it was resolved (`rule`) and how certain it is (`evidence`). What cannot be bound is reported as
unresolved instead of guessed.

## Consequences
- Works on any checkout in seconds, without dependencies, without executing anything.
- Lower precision than a compiler for overloads, generics and inference; this is surfaced per edge
  and in docs/LANGUAGE_SUPPORT.md.
- Two-phase design (per-file extraction cached by blob, per-snapshot resolution) keeps incremental
  indexing simple.
- A compiler/LSP-backed frontend could later *upgrade* edges to exact evidence as an optional,
  sandboxed mode; it would not replace this path.
