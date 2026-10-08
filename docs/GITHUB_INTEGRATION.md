# GitHub integration

Ripplepath runs in GitHub Actions as a composite action (`action.yml` at the repository root). It
analyses the change of a pull request or push and publishes:

- a **step summary** (Markdown) with the policy verdict, risk decomposition, blast radius, test
  selection, new architecture violations, coverage gaps and one evidence path;
- **annotations** on the lines of located findings;
- the **outputs as an artifact**: `analysis.json`, `summary.md`, `ripplepath.sarif`,
  `annotations.txt`;
- optionally **SARIF** in code scanning and **one pull-request comment** that is updated in place;
- a **merge gate**: the step fails with exit status 2 when the merge policy is `FAIL`.

The action only reads Git objects of the checkout. It never builds, tests or runs code of the
analysed repository (see [SECURITY.md](../SECURITY.md)). An example of the summary, produced by the
self-test on the java-banking demo, is in [assets/github-summary-example.md](assets/github-summary-example.md).

## Usage

No binary release has been published yet. Until the first `v*` tag exists, reference the action
by commit SHA; it then builds Ripplepath from that commit with cargo (see "Obtaining the binary").
The examples below show the intended release pin.

### Pull requests

```yaml
name: Ripplepath
on: pull_request
permissions:
  contents: read
jobs:
  ripplepath:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      pull-requests: write   # only for `comment: true`
    steps:
      - uses: actions/checkout@v7
        with:
          fetch-depth: 0      # the base commit must be in the clone
          persist-credentials: false
      - uses: martiaaguilera/ripplepath@v0.1.0   # pin a release tag or a commit SHA
        with:
          comment: true
```

Defaults: `base` is `github.event.pull_request.base.sha`, `head` is `HEAD`, which for the default
checkout of a `pull_request` event is the merge commit GitHub prepared. Comparing base with the merge
commit shows the pull request's effect on the current base branch, even when the base branch moved
after the pull request was opened (comparing base with the PR head commit would then report the
base branch's newer commits as reverted). If you check out the PR head commit instead, set
`head: ${{ github.event.pull_request.head.sha }}` and expect that difference.

### Pushes

```yaml
on:
  push:
    branches: [main]
jobs:
  ripplepath:
    runs-on: ubuntu-latest
    permissions:
      contents: read
    steps:
      - uses: actions/checkout@v7
        with:
          fetch-depth: 0
          persist-credentials: false
      - uses: martiaaguilera/ripplepath@v0.1.0
        with:
          fail-on-policy: false   # report on main, do not fail the push
```

`base` defaults to `github.event.before`. A push that creates a branch has no previous commit
(`0000…`); the action then stops with an error asking for an explicit `base` (e.g. `origin/main`).

### Code scanning (SARIF)

```yaml
    permissions:
      contents: read
      security-events: write
    steps:
      - uses: actions/checkout@v7
        with: { fetch-depth: 0, persist-credentials: false }
      - uses: martiaaguilera/ripplepath@v0.1.0
        with:
          sarif: true
```

## Inputs

| Input | Default | Meaning |
|---|---|---|
| `base` | event-specific (above) | Base revision. |
| `head` | `HEAD` | Head revision. |
| `repo-path` | `.` | Repository to analyse, relative to the workspace. Annotation and SARIF paths are prefixed with it. |
| `mode` | from base `ripplepath.yml`, else `balanced` | `conservative`, `balanced` or `fast-feedback`. |
| `fail-on-policy` | `true` | Fail the step with exit status 2 when the policy is `FAIL`. |
| `step-summary` | `true` | Append `summary.md` to the job summary. |
| `annotations` | `true` | Emit annotations for located findings. |
| `comment` | `false` | Create or update one PR comment. |
| `sarif` | `false` | Upload SARIF to code scanning. |
| `sarif-category` | `ripplepath` | Code scanning category. |
| `upload-artifact` | `true` | Upload the output directory as an artifact. |
| `artifact-name` | `ripplepath` | Artifact name; must be unique when the action runs more than once in a workflow run. |
| `output-dir` | `$RUNNER_TEMP/ripplepath-<artifact-name>` | Where the four files are written. |
| `version` | the action's tag, else `source` | Release to download (`v0.2.0`), or `source` to build with cargo. |
| `binary-path` | — | Use an existing `ripplepath` binary. |
| `release-repository` | `martiaaguilera/ripplepath` | Where releases are downloaded from. |
| `github-token` | `github.token` | Token for the PR comment. |

## Outputs

`policy` (`PASS`/`WARN`/`FAIL`), `risk-score` (0–100, **not a probability**), `risk-level`,
`exit-code`, `output-dir`, `summary-file`, `sarif-file`.

## Permissions per feature

| Feature | Permission |
|---|---|
| Analysis, step summary, annotations, artifact | `contents: read` |
| PR comment (`comment: true`) | `pull-requests: write` |
| SARIF upload (`sarif: true`) | `security-events: write` (and code scanning available for the repository) |
| Merge gate | none; make the job a required status check in branch protection |

Grant nothing else. `persist-credentials: false` on checkout keeps the token out of `.git/config`.

## Obtaining the binary

1. `binary-path`, when given.
2. A release: when the action is referenced by a `v*` tag (or `version` names one), the archive for
   the runner (`x86_64-unknown-linux-musl`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`) and its
   `.sha256` file are downloaded from the release and the checksum is verified. A mismatch fails the
   step; it never falls back to another source. The checksum detects corrupted or swapped downloads
   from the same release, not a compromised release itself.
3. Otherwise `cargo install --locked --path <action checkout>/crates/cli`: the exact source of the
   ref the workflow pinned (branch or SHA), not the analysed repository. Needs Rust on the runner
   (GitHub-hosted runners have it) and takes a few minutes; prefer a release tag.

Releases are built by `.github/workflows/release.yml` when a `v*` tag is pushed: one archive per
target with `ripplepath`, `LICENSE` and `README.md`, a `.sha256` per archive and a combined
`SHA256SUMS`. Pull requests that modify the workflow build the same archives without publishing.

## Shallow clones

`actions/checkout` fetches one commit by default, so the base revision is missing. Use
`fetch-depth: 0` (or fetch the base commit explicitly). When the analysis fails and the clone is
shallow (`.git/shallow` exists), the action reports exactly that instead of a bare "revision not
found". It checks the file rather than running `git`, because the action never runs Git commands
on the analysed repository.

## Fork pull requests

For `pull_request` events from forks GitHub issues a read-only `GITHUB_TOKEN` regardless of the
workflow's `permissions`. The analysis, step summary, annotations and artifact work; the comment
and SARIF upload are skipped with a notice instead of failing the job. Do not switch to
`pull_request_target` just to comment: it runs with a write token in the context of the base
repository, and while Ripplepath itself executes nothing from the checkout, any other step in such
a job that builds or runs the fork's code would do so with that token.

## Exit codes

| Status | Meaning |
|---|---|
| 0 | Analysis done; policy `PASS` or `WARN`, or `fail-on-policy: false`. |
| 1 | Error (revision not found, invalid input, I/O). |
| 2 | Policy `FAIL` with `fail-on-policy: true` (`ripplepath analyze --fail-on-policy`). |

The artifact, summary, annotations, SARIF and comment are produced before the gate step fails, so a
failing policy is always explained.

## SARIF

SARIF 2.1.0, one run, tool `Ripplepath`. Only findings that have a source location are included;
graph-level results (blast radius, test selection, layer cycles, risk) have no single line to point
at and stay in the summary and `analysis.json`.

Rule ids (stable; code scanning keys alert history on them):

- <a id="new-architecture-violation"></a>`ripplepath/new-architecture-violation` — a dependency
  introduced by the change breaks a layer rule of the base revision's `ripplepath.yml`. Location: the
  edge's source line. Pre-existing violations are not reported.
- <a id="breaking-api-change"></a>`ripplepath/breaking-api-change` — public API removed, narrowed or
  re-signed. Location: the declaration (head, or base for removals).
- <a id="parse-failure-in-changed-file"></a>`ripplepath/parse-failure-in-changed-file` — a changed
  file has syntax errors, could not be parsed, or exceeds the size limit in head.
- <a id="config-invalid"></a>`ripplepath/config-invalid` — `ripplepath.yml` is invalid in base or
  head. Configuration errors have no line; they are anchored at line 1 of the file
  (`properties.fileLevel: true`) and omitted when head no longer has the file.

Each result's `level` follows its policy gate: `error` when the gate fails the policy, `warning` when
it warns, `note` when the gate is off. `partialFingerprints["ripplepath/findingHash/v1"]` hashes the
finding's identity (e.g. rule, source and target symbol and edge kind), not its line, so alerts
survive unrelated line moves. URIs are relative to the workspace and percent-encoded. Output is
byte-identical for the same inputs (no timestamps or absolute paths).

## Annotations

GitHub workflow commands (`::error file=…,line=…,title=…::message`), one per located finding, plus
one for the policy verdict when it is `WARN` or `FAIL`. Errors come first, and at most 50 are emitted
(GitHub displays 10 errors and 10 warnings per step); the remainder is counted in a final notice.

Escaping follows `@actions/core`: in messages `%`, CR and LF become `%25`, `%0D`, `%0A`; in property
values `:` and `,` also become `%3A` and `%2C`. Control and bidi characters are shown as visible
`\u{…}` escapes.

## The pull-request comment

With `comment: true` the action posts the summary as one comment that starts with the hidden marker
`<!-- ripplepath -->`. Later runs find the comment written by `github-actions[bot]` with that marker
and edit it, so a pull request never accumulates Ripplepath comments. Comments by anyone else that
contain the marker are ignored. Without permission (fork, missing `pull-requests: write`) it logs a
notice or warning and continues.

## Security notes

- Nothing from the analysed repository is executed: no build, tests, scripts, hooks or Git filters.
  The action does not run `git` either.
- Action inputs and event fields reach the scripts as environment variables, never as `${{ }}`
  expressions spliced into shell code, so a branch name cannot inject commands.
- The text report printed to the log quotes repository content. It is printed between
  `::stop-commands::<random token>` and `::<token>::`, so a file or symbol name that looks like a
  workflow command (`::add-mask::`, `::stop-commands::`) is not executed by the runner.
- Markdown (summary and comment): repository text is rendered inside code spans or with every ASCII
  punctuation character backslash-escaped, and line breaks are removed, so it cannot add HTML,
  links, headings or table cells. `@` and `#` in escaped text are followed by an invisible
  WORD JOINER (U+2060) so that symbol names cannot mention users or reference issues. The summary is
  limited to 60 000 bytes (GitHub rejects comments above 65 536 characters); longer lists are
  truncated with a pointer to `analysis.json`.
- The configuration is always the base revision's (ADR 0005): a pull request cannot relax the
  policy that gates it.
