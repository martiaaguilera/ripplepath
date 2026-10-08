#!/usr/bin/env bash
# Provides the ripplepath binary for the composite action, in order of preference:
#   1. `binary-path` (a binary the workflow built or cached itself);
#   2. a release archive for this runner, verified against its published SHA-256;
#   3. a source build of the action's own checkout (the exact ref the workflow pinned).
# Inputs arrive as environment variables, never as `${{ }}` text spliced into this script.
set -euo pipefail

exe=""
if [ "${RUNNER_OS:-}" = "Windows" ]; then exe=".exe"; fi

if [ -n "${INPUT_BINARY_PATH:-}" ]; then
  if [ ! -f "$INPUT_BINARY_PATH" ]; then
    echo "::error title=Ripplepath::binary-path '$INPUT_BINARY_PATH' does not exist"
    exit 1
  fi
  bin="$(cd "$(dirname "$INPUT_BINARY_PATH")" && pwd)/$(basename "$INPUT_BINARY_PATH")"
  echo "bin=$bin" >> "$GITHUB_OUTPUT"
  echo "source=binary-path" >> "$GITHUB_OUTPUT"
  exit 0
fi

version="${INPUT_VERSION:-}"
if [ -z "$version" ]; then
  # `uses: owner/ripplepath@v1.2.3` pins a release; a branch or SHA pins source.
  case "${ACTION_REF:-}" in
    v[0-9]*) version="$ACTION_REF" ;;
    *) version="source" ;;
  esac
fi

target=""
ext=""
case "${RUNNER_OS:-}/${RUNNER_ARCH:-}" in
  Linux/X64) target="x86_64-unknown-linux-musl"; ext="tar.gz" ;;
  macOS/ARM64) target="aarch64-apple-darwin"; ext="tar.gz" ;;
  Windows/X64) target="x86_64-pc-windows-msvc"; ext="zip" ;;
esac

sha256() {
  if command -v sha256sum > /dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

if [ "$version" != "source" ] && [ -n "$target" ]; then
  name="ripplepath-$version-$target"
  url="https://github.com/$RELEASE_REPOSITORY/releases/download/$version"
  work="$RUNNER_TEMP/ripplepath-download"
  rm -rf "$work"
  mkdir -p "$work"
  if curl -fsSL --retry 3 -o "$work/$name.$ext" "$url/$name.$ext" \
    && curl -fsSL --retry 3 -o "$work/$name.$ext.sha256" "$url/$name.$ext.sha256"; then
    expected="$(cut -d' ' -f1 < "$work/$name.$ext.sha256" | tr -d '\r\n')"
    actual="$(sha256 "$work/$name.$ext")"
    if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
      # Never fall back to a source build here: a mismatch means the download is not what was
      # published, and silently continuing would hide that.
      echo "::error title=Ripplepath::checksum mismatch for $name.$ext (expected '$expected', got '$actual')"
      exit 1
    fi
    if [ "$ext" = "zip" ]; then
      if command -v unzip > /dev/null; then
        unzip -q "$work/$name.$ext" -d "$work"
      else
        pwsh -NoProfile -Command "Expand-Archive -LiteralPath '$work/$name.$ext' -DestinationPath '$work'"
      fi
    else
      tar -xzf "$work/$name.$ext" -C "$work"
    fi
    bin="$work/$name/ripplepath$exe"
    chmod +x "$bin" 2> /dev/null || true
    echo "Using released ripplepath $version ($target), SHA-256 $actual"
    echo "bin=$bin" >> "$GITHUB_OUTPUT"
    echo "source=release" >> "$GITHUB_OUTPUT"
    exit 0
  fi
  echo "::notice title=Ripplepath::no release asset $name.$ext for $version; building from source instead"
fi

if ! command -v cargo > /dev/null; then
  echo "::error title=Ripplepath::cargo not found. Install Rust (e.g. rustup) before this action, use a released version, or pass binary-path."
  exit 1
fi
root="$RUNNER_TEMP/ripplepath-cargo"
# The action's own checkout, not the analysed repository: nothing from the repository under
# analysis is built or executed.
cargo install --locked --quiet --path "$ACTION_PATH/crates/cli" --root "$root"
echo "bin=$root/bin/ripplepath$exe" >> "$GITHUB_OUTPUT"
echo "source=cargo" >> "$GITHUB_OUTPUT"
