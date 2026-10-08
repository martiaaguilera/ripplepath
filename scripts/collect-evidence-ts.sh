#!/usr/bin/env bash
# Collects REAL test evidence for Ripplepath's own `fixtures/typescript-checkout` snapshots:
#   - testwise LCOV coverage: each test file runs alone under Vitest with V8 coverage, so every
#     tracefile says what that one file executed;
#   - JUnit XML results of whole-suite runs for test history. The fixture contains one
#     deliberately nondeterministic test (src/clock.test.ts), so repeated runs at one commit record
#     real pass/fail flips.
#
# Opt-in developer tooling. It installs pinned npm packages into a temporary directory and executes
# the fixture code shipped in this repository, nothing else; Ripplepath itself never runs code
# from an analysed repository. In a real project this collection is the job of the project's CI.
#
# Requirements: bash, node 20+, npm, network access to the npm registry.
#
# Usage: scripts/collect-evidence-ts.sh     (writes fixtures/typescript-checkout/evidence/v1|v2)
# Env:   RUNS_V1=8 RUNS_V2=4               whole-suite runs per snapshot
set -euo pipefail

# Same Vitest as web/package.json, so the evidence matches the toolchain this project uses.
VITEST_VERSION=5.0.3
RUNS_V1=${RUNS_V1:-8}
RUNS_V2=${RUNS_V2:-4}

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
FIXTURE="$ROOT/fixtures/typescript-checkout"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

# One install serves both snapshots: their package.json files are identical and only src/ differs.
cp "$FIXTURE/v1/package.json" "$WORK/package.json"
npm install --prefix "$WORK" --no-audit --no-fund --ignore-scripts --save-exact \
  "vitest@$VITEST_VERSION" "@vitest/coverage-v8@$VITEST_VERSION" >/dev/null
VITEST="$WORK/node_modules/vitest/vitest.mjs"

# Vitest's JUnit report names the machine (`hostname`), and failure stack traces carry the
# absolute path of the temporary work directory. Both identify the machine, not the run; they are
# replaced with placeholders and nothing else is edited.
sanitize_junit() {
  local native_work
  native_work=$(cd "$WORK" && pwd -W 2>/dev/null || pwd)
  sed -i -e 's/ hostname="[^"]*"/ hostname="redacted"/g' \
    -e "s#${native_work//\\//}#<work>#g" -e "s#${WORK}#<work>#g" "$1"
}

collect() { # <snapshot name> <suite runs>
  local version=$1 runs=$2
  local out="$FIXTURE/evidence/$version"
  rm -rf "$out" "$WORK/src"
  mkdir -p "$out/coverage" "$out/junit"
  cp -r "$FIXTURE/$version/src" "$WORK/src"

  # Testwise coverage. `include` lists untested files too (as zero hits), so "measured and not
  # executed" is distinguishable from "not measured". `reportOnFailure` keeps the coverage of a
  # run whose test failed: the code it executed is still evidence.
  while read -r test_file; do
    echo "[$version] coverage of $test_file"
    rm -rf "$WORK/coverage"
    node "$VITEST" run "$test_file" --root "$WORK" \
      --coverage.enabled --coverage.provider=v8 --coverage.reporter=lcovonly \
      --coverage.reportsDirectory=coverage '--coverage.include=src/**' --coverage.reportOnFailure \
      >/dev/null 2>&1 || echo "[$version]   (test failed; coverage kept)"
    mkdir -p "$out/coverage/$(dirname "$test_file")"
    cp "$WORK/coverage/lcov.info" "$out/coverage/$test_file.lcov"
  done < <(cd "$WORK" && find src -name '*.test.ts' -o -name '*.test.tsx' | sort)

  for run in $(seq 1 "$runs"); do
    rm -f "$WORK/junit.xml"
    node "$VITEST" run --root "$WORK" --reporter=junit --outputFile="$WORK/junit.xml" >/dev/null 2>&1 \
      && echo "[$version] suite run $run/$runs: passed" \
      || echo "[$version] suite run $run/$runs: failures"
    cp "$WORK/junit.xml" "$out/junit/run-$run.xml"
    sanitize_junit "$out/junit/run-$run.xml"
  done
}

collect v1 "$RUNS_V1"
collect v2 "$RUNS_V2"
echo "evidence written to $FIXTURE/evidence"
