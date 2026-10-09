#!/usr/bin/env bash
# Runs `ripplepath analyze` for the composite action and publishes its outputs: step summary,
# annotations and step outputs. The policy verdict is enforced by a later step, so artifacts,
# SARIF and the PR comment are produced even when the policy fails.
set -uo pipefail

repo="${INPUT_REPO_PATH:-.}"
out="${INPUT_OUTPUT_DIR:-}"
if [ -z "$out" ]; then out="$RUNNER_TEMP/ripplepath-${INPUT_ARTIFACT_NAME:-ripplepath}"; fi
head="${INPUT_HEAD:-}"
if [ -z "$head" ]; then head="HEAD"; fi

base="${INPUT_BASE:-}"
if [ -z "$base" ]; then
  case "${EVENT_NAME:-}" in
    pull_request | pull_request_target) base="${PR_BASE_SHA:-}" ;;
    push) base="${PUSH_BEFORE:-}" ;;
    merge_group) base="${MERGE_GROUP_BASE_SHA:-}" ;;
  esac
fi
if [ -z "$base" ] || [ "$base" = "0000000000000000000000000000000000000000" ]; then
  echo "::error title=Ripplepath: no base revision::Cannot infer the base revision for event '${EVENT_NAME:-}' (only pull_request, push and merge_group provide one, and a push that creates a branch has no previous commit). Pass the 'base' input, e.g. base: origin/main."
  exit 1
fi

# Annotation and SARIF paths are resolved against the workspace; the repository may be a
# subdirectory of it.
prefix=""
repo_abs="$(cd "$repo" && pwd -P)" || { echo "::error title=Ripplepath::repo-path '$repo' does not exist"; exit 1; }
workspace_abs="$(cd "${GITHUB_WORKSPACE:-.}" && pwd -P)"
case "$repo_abs" in
  "$workspace_abs") prefix="" ;;
  "$workspace_abs"/*) prefix="${repo_abs#"$workspace_abs"/}" ;;
  *) echo "::notice title=Ripplepath::repo-path is outside the workspace; annotation paths are relative to the repository" ;;
esac

args=(analyze --repo "$repo" --base "$base" --head "$head" --output-dir "$out" --path-prefix "$prefix" --format text --fail-on-policy)
if [ -n "${INPUT_MODE:-}" ]; then args+=(--mode "$INPUT_MODE"); fi

# The text report quotes repository content (paths, symbol names). A line of it that looked like a
# workflow command (`::add-mask::`, `::stop-commands::`, …) would be executed by the runner, so
# command processing is suspended while it prints, with an unguessable resume token.
token="ripplepath-$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')"
echo "::stop-commands::$token"
"$RIPPLEPATH_BIN" "${args[@]}"
status=$?
echo "::$token::"

# Status 2 means "policy FAIL" only when a report was written; anything else (an older binary's
# usage error, a crash) is an error, never a verdict.
if [ "$status" -eq 2 ] && [ ! -s "$out/analysis.json" ]; then
  echo "::error title=Ripplepath: analysis failed::ripplepath exited with status 2 but wrote no analysis.json; see the log above."
  status=1
fi

echo "exit-code=$status" >> "$GITHUB_OUTPUT"
echo "output-dir=$out" >> "$GITHUB_OUTPUT"
if [ "$status" -ne 0 ] && [ "$status" -ne 2 ]; then
  if [ -f "$repo/.git/shallow" ]; then
    echo "::error title=Ripplepath: shallow clone::The checkout is shallow, so base revision '$base' is probably missing. Use actions/checkout with 'fetch-depth: 0' (or fetch the base commit) before this action."
  else
    echo "::error title=Ripplepath: analysis failed::ripplepath exited with status $status; see the log above."
  fi
  exit "$status"
fi

if [ "${INPUT_STEP_SUMMARY:-true}" = "true" ]; then
  cat "$out/summary.md" >> "$GITHUB_STEP_SUMMARY"
fi
if [ "${INPUT_ANNOTATIONS:-true}" = "true" ]; then
  # Escaped for workflow commands by ripplepath itself (see crates/cli/src/annotations.rs).
  cat "$out/annotations.txt"
fi

{
  echo "policy=$(jq -r '.policy.result' "$out/analysis.json")"
  echo "risk-score=$(jq -r '.risk.score' "$out/analysis.json")"
  echo "risk-level=$(jq -r '.risk.level' "$out/analysis.json")"
  echo "summary-file=$out/summary.md"
  echo "sarif-file=$out/ripplepath.sarif"
  echo "analysis-file=$out/analysis.json"
} >> "$GITHUB_OUTPUT"
