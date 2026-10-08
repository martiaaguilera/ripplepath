#!/usr/bin/env bash
# Creates or updates the single Ripplepath comment on a pull request. Never fails the job: a missing
# permission (fork PRs get a read-only GITHUB_TOKEN) degrades to a notice, because the summary is
# already in the job's step summary.
set -uo pipefail

marker='<!-- ripplepath -->'
# Only comments written by this identity are updated; anyone can post a comment containing the marker.
author="${COMMENT_AUTHOR:-github-actions[bot]}"

case "${EVENT_NAME:-}" in
  pull_request | pull_request_target) ;;
  *)
    echo "::notice title=Ripplepath::comment: only pull_request events have a pull request to comment on"
    exit 0
    ;;
esac
if [ -z "${PR_NUMBER:-}" ]; then exit 0; fi
if [ "${EVENT_NAME}" = "pull_request" ] && [ "${HEAD_REPOSITORY:-}" != "${GITHUB_REPOSITORY:-}" ]; then
  echo "::notice title=Ripplepath::comment skipped: pull requests from forks get a read-only token; the summary is in the job summary"
  exit 0
fi

body="$RUNNER_TEMP/ripplepath-comment.md"
{
  echo "$marker"
  cat "$SUMMARY_FILE"
} > "$body"

if ! ids="$(gh api --paginate "repos/$GITHUB_REPOSITORY/issues/$PR_NUMBER/comments" \
  --jq ".[] | select(.user.login == \"$author\" and (.body | startswith(\"$marker\"))) | .id")"; then
  echo "::warning title=Ripplepath::cannot list pull request comments (does the job grant 'pull-requests: write'?); comment skipped"
  exit 0
fi
id="$(printf '%s\n' "$ids" | head -n 1)"

if [ -n "$id" ]; then
  if gh api -X PATCH "repos/$GITHUB_REPOSITORY/issues/comments/$id" -F "body=@$body" > /dev/null; then
    echo "Updated pull request comment $id"
  else
    echo "::warning title=Ripplepath::cannot update the pull request comment (permission 'pull-requests: write' missing?)"
  fi
else
  if gh api -X POST "repos/$GITHUB_REPOSITORY/issues/$PR_NUMBER/comments" -F "body=@$body" > /dev/null; then
    echo "Created pull request comment"
  else
    echo "::warning title=Ripplepath::cannot create a pull request comment (permission 'pull-requests: write' missing?)"
  fi
fi
