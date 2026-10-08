#!/usr/bin/env bash
# Collects REAL test evidence for every snapshot of `fixtures/eval-history` (v1..v12), the history
# replayed by `ripplepath evaluate` (docs/TEST_INTELLIGENCE.md, "Offline evaluation"):
#   - testwise JaCoCo XML coverage: each test class runs alone under the JaCoCo agent;
#   - JUnit XML of one whole-suite run per snapshot. Its failures are the observed outcome the
#     evaluation scores recommendations against, so failing runs are kept, never retried.
#
# Opt-in developer tooling. It compiles and executes only the fixture code shipped in this
# repository; Ripplepath itself never runs code from an analysed repository.
#
# Requirements: bash, curl, sha1sum, JDK 21+ (`java`, `javac` on PATH). The tool jars are the same
# pinned Maven Central artifacts as scripts/collect-evidence-java.sh, verified by SHA-1.
#
# Usage: scripts/collect-eval-history.sh       (writes fixtures/eval-history/evidence/v1..v12)
# Env:   RIPPLEPATH_TOOL_CACHE=dir             where downloaded jars are kept (default ~/.cache/ripplepath-tools)
set -euo pipefail

JUNIT_VERSION=6.1.3
JACOCO_VERSION=0.8.15
JUNIT_SHA1=bb65bef57072d36433e8a863016163858c926113
AGENT_SHA1=14df437c4e8ff131c0ed9a0c8802b30927fa91b5
CLI_SHA1=1da22eb914b9176037589aaaac612b3f9b65f7ea
SNAPSHOTS=12

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
FIXTURE="$ROOT/fixtures/eval-history"
CACHE=${RIPPLEPATH_TOOL_CACHE:-$HOME/.cache/ripplepath-tools}
CENTRAL=https://repo1.maven.org/maven2

# Java on Windows is a native program: it needs Windows paths and ';' between classpath entries.
if command -v cygpath >/dev/null 2>&1; then
  native() { cygpath -w "$1"; }
  SEP=';'
else
  native() { printf '%s' "$1"; }
  SEP=':'
fi

fetch() { # <maven path> <sha1> -> prints local path
  local dest="$CACHE/$(basename "$1")"
  if [[ ! -f "$dest" ]]; then
    mkdir -p "$CACHE"
    curl --fail --silent --show-error --location -o "$dest.part" "$CENTRAL/$1"
    mv "$dest.part" "$dest"
  fi
  if [[ "$(sha1sum "$dest" | cut -d' ' -f1)" != "$2" ]]; then
    echo "checksum mismatch for $dest; delete it and retry" >&2
    exit 1
  fi
  printf '%s' "$dest"
}

CONSOLE=$(fetch "org/junit/platform/junit-platform-console-standalone/$JUNIT_VERSION/junit-platform-console-standalone-$JUNIT_VERSION.jar" "$JUNIT_SHA1")
AGENT=$(fetch "org/jacoco/org.jacoco.agent/$JACOCO_VERSION/org.jacoco.agent-$JACOCO_VERSION-runtime.jar" "$AGENT_SHA1")
JACOCO_CLI=$(fetch "org/jacoco/org.jacoco.cli/$JACOCO_VERSION/org.jacoco.cli-$JACOCO_VERSION-nodeps.jar" "$CLI_SHA1")

# Machine-identifying values only (JVM system properties, host names, JaCoCo session ids); nothing
# else in the tool output is edited. Java stack traces name source files, not absolute paths.
sanitize_junit() {
  sed -i -e '/<properties>/,/<\/properties>/d' -e 's/ hostname="[^"]*"/ hostname="redacted"/' "$1"
}
sanitize_jacoco() {
  sed -i -e 's/<sessioninfo id="[^"]*"/<sessioninfo id="redacted"/g' "$1"
}

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

collect() { # <snapshot name>
  local version=$1
  local src="$FIXTURE/$version"
  local out="$FIXTURE/evidence/$version"
  local work="$WORK/$version"

  rm -rf "$out"
  mkdir -p "$out/coverage" "$out/junit" "$work/classes" "$work/test-classes"

  (cd "$src" && find src/main/java -name '*.java' | sort) > "$work/main.txt"
  (cd "$src" && find src/test/java -name '*.java' | sort) > "$work/test.txt"
  (cd "$src" && javac --release 21 -d "$(native "$work/classes")" @"$(native "$work/main.txt")")
  # Resources are read at runtime (tax-rates.properties); they belong on the classpath.
  cp -r "$src/src/main/resources/." "$work/classes/"
  (cd "$src" && javac --release 21 -d "$(native "$work/test-classes")" \
    -cp "$(native "$work/classes")$SEP$(native "$CONSOLE")" @"$(native "$work/test.txt")")
  local cp
  cp="$(native "$work/classes")$SEP$(native "$work/test-classes")"

  # Testwise coverage. A failing class still executed code; its coverage is kept.
  while read -r file; do
    local class
    class=$(printf '%s' "$file" | sed -e 's#^src/test/java/##' -e 's#\.java$##' -e 's#/#.#g')
    java "-javaagent:$(native "$AGENT")=destfile=$(native "$work/$class.exec"),append=false" \
      -jar "$(native "$CONSOLE")" execute --disable-banner --details=none \
      --class-path "$cp" --select-class "$class" \
      --reports-dir "$(native "$work/reports-$class")" </dev/null >/dev/null 2>&1 \
      && echo "[$version] coverage of $class" \
      || echo "[$version] coverage of $class (tests failed; coverage kept)"
    java -jar "$(native "$JACOCO_CLI")" report "$(native "$work/$class.exec")" \
      --classfiles "$(native "$work/classes")" --sourcefiles "$(native "$src/src/main/java")" \
      --xml "$(native "$out/coverage/$class.xml")" --quiet </dev/null
    sanitize_jacoco "$out/coverage/$class.xml"
  done < <(grep 'Test\.java$' "$work/test.txt")

  # The observed outcome: one whole-suite run without instrumentation.
  java -jar "$(native "$CONSOLE")" execute --disable-banner --details=none \
    --class-path "$cp" --scan-class-path "$(native "$work/test-classes")" \
    --reports-dir "$(native "$work/suite")" </dev/null >/dev/null 2>&1 \
    && echo "[$version] suite: passed" \
    || echo "[$version] suite: failures"
  cp "$work/suite/TEST-junit-jupiter.xml" "$out/junit/run-1.xml"
  sanitize_junit "$out/junit/run-1.xml"
}

for i in $(seq 1 "$SNAPSHOTS"); do
  collect "v$i"
done
echo "evidence written to $FIXTURE/evidence"
