#!/usr/bin/env bash
# Collects REAL test evidence for Ripplepath's own `fixtures/java-banking` snapshots:
#   - testwise JaCoCo XML coverage: each test class runs alone under the JaCoCo agent, so every
#     report says what that one class executed;
#   - JUnit XML results of whole-suite runs (repeated RUNS times per snapshot) for test history.
#
# Opt-in developer tooling. It compiles and executes the fixture code shipped in this repository
# and nothing else; Ripplepath itself never runs code from an analysed repository. In a real
# project this collection is the job of the project's own CI (Maven/Gradle + JaCoCo).
#
# Requirements: bash, curl, sha1sum, JDK 21+ (`java`, `javac` on PATH). Maven is not needed: the
# three tool jars are downloaded from Maven Central into a cache directory outside the repository
# and verified against pinned SHA-1 digests. Under Git Bash on Windows the console launcher prints
# "stty: /dev/tty: No such device or address" while probing the terminal width; it is harmless.
#
# Usage: scripts/collect-evidence-java.sh            (writes fixtures/java-banking/evidence/v1|v2)
# Env:   RUNS=3                       whole-suite runs per snapshot
#        RIPPLEPATH_TOOL_CACHE=dir    where downloaded jars are kept (default ~/.cache/ripplepath-tools)
set -euo pipefail

JUNIT_VERSION=6.1.3
JACOCO_VERSION=0.8.15
JUNIT_SHA1=bb65bef57072d36433e8a863016163858c926113
AGENT_SHA1=14df437c4e8ff131c0ed9a0c8802b30927fa91b5
CLI_SHA1=1da22eb914b9176037589aaaac612b3f9b65f7ea
RUNS=${RUNS:-3}

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
FIXTURE="$ROOT/fixtures/java-banking"
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

# The JUnit legacy XML reporter embeds every JVM system property (user name, home and work
# directories, host name) and JaCoCo's session id starts with the host name. Those identify the
# machine, not the test run, so they are removed; nothing else in the tool output is edited.
sanitize_junit() {
  sed -i -e '/<properties>/,/<\/properties>/d' -e 's/ hostname="[^"]*"/ hostname="redacted"/' "$1"
}
sanitize_jacoco() {
  sed -i -e 's/<sessioninfo id="[^"]*"/<sessioninfo id="redacted"/g' "$1"
}

collect() { # <snapshot name>
  local version=$1
  local src="$FIXTURE/$version"
  local out="$FIXTURE/evidence/$version"
  local work
  work=$(mktemp -d)
  trap 'rm -rf "$work"' RETURN

  rm -rf "$out"
  mkdir -p "$out/coverage" "$out/junit" "$work/classes" "$work/test-classes"

  (cd "$src" && find src/main/java -name '*.java' | sort) > "$work/main.txt"
  (cd "$src" && find src/test/java -name '*.java' | sort) > "$work/test.txt"
  (cd "$src" && javac --release 21 -d "$(native "$work/classes")" @"$(native "$work/main.txt")")
  (cd "$src" && javac --release 21 -d "$(native "$work/test-classes")" \
    -cp "$(native "$work/classes")$SEP$(native "$CONSOLE")" @"$(native "$work/test.txt")")
  local cp
  cp="$(native "$work/classes")$SEP$(native "$work/test-classes")"

  # Testwise coverage: one JVM per test class, a fresh exec file each time.
  while read -r file; do
    local class
    class=$(printf '%s' "$file" | sed -e 's#^src/test/java/##' -e 's#\.java$##' -e 's#/#.#g')
    echo "[$version] coverage of $class"
    java "-javaagent:$(native "$AGENT")=destfile=$(native "$work/$class.exec"),append=false" \
      -jar "$(native "$CONSOLE")" execute --disable-banner --details=none \
      --class-path "$cp" --select-class "$class" \
      --reports-dir "$(native "$work/reports-$class")" </dev/null
    java -jar "$(native "$JACOCO_CLI")" report "$(native "$work/$class.exec")" \
      --classfiles "$(native "$work/classes")" --sourcefiles "$(native "$src/src/main/java")" \
      --xml "$(native "$out/coverage/$class.xml")" --quiet </dev/null
    sanitize_jacoco "$out/coverage/$class.xml"
  done < <(grep 'Test\.java$' "$work/test.txt")

  # History: the whole suite, RUNS times, without coverage instrumentation.
  for run in $(seq 1 "$RUNS"); do
    echo "[$version] suite run $run/$RUNS"
    java -jar "$(native "$CONSOLE")" execute --disable-banner --details=none \
      --class-path "$cp" --scan-class-path "$(native "$work/test-classes")" \
      --reports-dir "$(native "$work/suite-$run")" </dev/null
    cp "$work/suite-$run/TEST-junit-jupiter.xml" "$out/junit/run-$run.xml"
    sanitize_junit "$out/junit/run-$run.xml"
  done
}

collect v1
collect v2
echo "evidence written to $FIXTURE/evidence"
