#!/bin/sh
# Build examples first with Cargo. This script never downloads or invokes Java.
# Optional argument: already-downloaded Apache Commons Lang 3.17.0 JAR.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
# Empty environment: no JAVA_HOME/JDK_HOME and no executable search path.
env -i PATH=/nonexistent "$root/target/debug/examples/project"
env -i PATH=/nonexistent "$root/target/debug/examples/scopes"
if [ "$#" -eq 0 ]; then
    exit 0
fi
if [ "$#" -ne 1 ]; then
    echo "usage: $0 [commons-lang3-3.17.0.jar]" >&2
    exit 2
fi
jar=$(realpath "$1")
expected=6ee731df5c8e5a2976a1ca023b6bb320ea8d3539fbe64c8a1d5cb765127c33b4
actual=$(sha256sum "$jar")
if [ "${actual%% *}" != "$expected" ]; then
    echo 'unexpected Commons Lang artifact SHA-256' >&2
    exit 1
fi
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT HUP INT TERM
mkdir -p "$scratch/src/app"
cat > "$scratch/src/app/Use.java" <<'JAVA'
package app;
import org.apache.commons.lang3.StringUtils;
class Use { Object title() { return StringUtils.capitalize("native rust"); } }
JAVA
# The capitalize identifier starts at byte 106 in the exact UTF-8 fixture above.
output=$(env -i PATH=/nonexistent "$root/target/debug/examples/inspect_project" "$scratch/src" app/Use.java 106 "$jar")
printf '%s\n' "$output"
printf '%s\n' "$output" | grep -q '^1 sources, 395 library classes, 0 shadowed classes$'
printf '%s\n' "$output" | grep -q '^Resolved$'
printf '%s\n' "$output" | grep -q '^org.apache.commons.lang3.StringUtils::capitalize at '
