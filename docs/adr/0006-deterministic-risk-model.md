# ADR 0006 — A deterministic, versioned risk decomposition (not a probability)

Status: accepted (2026-10-08)

## Context
Reviewers want to know where to look first. The common answer in this space is a single 0–100
"risk" or "health" number, often presented with the authority of a probability. Ripplepath has no
outcome dataset (which changes later caused incidents), so any probability it printed would be
fabricated precision. ML ranking is excluded for the same reason (docs/RESEARCH.md §6).

## Decision
- Risk is a **sum of named signals**, each `min(units × weight, cap)` with integer arithmetic only,
  capped at 100 in total, with four fixed bands (LOW < 15 ≤ MEDIUM < 40 ≤ HIGH < 70 ≤ CRITICAL).
- Every signal reports its definition, measured value, units, weight, cap, points and evidence (the
  symbol ids or paths counted). Signals without input are reported as not evaluated, not as zero
  risk silently.
- The output and the docs state that the score is **not a probability of failure**.
- Weights, caps, scales and bands are part of `risk.model_version`. Any change is a new version,
  documented in docs/RISK_MODEL.md; there are no repository-specific weight overrides in
  `ripplepath.yml`, so scores are comparable across repositories at one model version.
- The score is **never a merge gate**. Gates evaluate concrete findings (new violation, parse
  failure, breaking API…), each explainable on its own.

## Consequences
- Same inputs ⇒ same score, byte for byte; golden tests assert exact decompositions.
- Weights are judgement, documented with their reasons; they may be revised, visibly, when replay
  evaluation produces evidence. Until then the decomposition — not the total — is the product.
- Correlated signals (a new violation that also closes a cycle) both score; per-signal caps bound
  the effect. The java-banking fixture scores 99, mostly from a domain → api dependency, a breaking
  API change and a migration — high by design for that combination.
- Rejected: probabilities or percentages; ML-learned weights; per-repository weight overrides;
  thresholds on the score as gates.
