#!/usr/bin/env bash
# Reproduces the measurements in docs/BENCHMARKS.md.
#
#   scripts/bench.sh [OUT_DIR]
#
# Environment (all optional):
#   SIZES       synthetic dataset sizes in source files   (default: "1000 5000 20000")
#   SEED        generator seed                            (default: 42)
#   ITERATIONS  runs per workload                         (default: 5)
#   QUERIES     impact queries for the graph workload     (default: 1000)
#   PROFILE_SIZE  size for the in-memory phase profile    (default: 5000; empty to skip)
#   REAL_REPO   path to a local clone to benchmark too    (default: none)
#   REAL_BASE / REAL_HEAD  revisions for REAL_REPO        (default: HEAD~1 / HEAD)
#   WORK        scratch directory (repositories, databases; can be large)
#
# Writes <OUT_DIR>/meta.txt, host.json, synthetic-<N>.{json,md}, profile-<N>.json, real.{json,md}.
# The harness never runs code from the benchmarked repository; it only reads Git objects.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:-"$root/target/bench-results"}
work=${WORK:-"$out/work"}
sizes=${SIZES:-"1000 5000 20000"}
seed=${SEED:-42}
iterations=${ITERATIONS:-5}
queries=${QUERIES:-1000}
profile_size=${PROFILE_SIZE-5000}
mkdir -p "$out" "$work"

cargo build --release --locked -p ripplepath-bench
bin="$root/target/release/ripplepath-bench"

{
  echo "date_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "ripplepath_commit: $(git -C "$root" rev-parse HEAD)"
  echo "worktree_dirty_files: $(git -C "$root" status --porcelain --untracked-files=no | wc -l | tr -d ' ')"
  echo "rustc: $(rustc --version)"
  echo "build: cargo build --release --locked -p ripplepath-bench"
  echo "sizes: $sizes  seed: $seed  iterations: $iterations  queries: $queries"
} | tee "$out/meta.txt"
"$bin" host | tee "$out/host.json"

for n in $sizes; do
  echo "== synthetic $n files" >&2
  "$bin" suite --work "$work" --files "$n" --seed "$seed" --iterations "$iterations" --queries "$queries" \
    --out "$out/synthetic-$n.json" | tee "$out/synthetic-$n.md"
done

if [ -n "$profile_size" ]; then
  echo "== in-memory phase profile, $profile_size files" >&2
  "$bin" profile --files "$profile_size" --seed "$seed" --iterations "$iterations" | tee "$out/profile-$profile_size.json"
fi

if [ -n "${REAL_REPO:-}" ]; then
  echo "== repository $REAL_REPO" >&2
  "$bin" suite --work "$work/real" --repo "$REAL_REPO" --base "${REAL_BASE:-HEAD~1}" --head "${REAL_HEAD:-HEAD}" \
    --iterations "$iterations" --queries "$queries" --out "$out/real.json" | tee "$out/real.md"
fi
