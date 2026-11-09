#!/bin/sh
# Optional offline check: supply only java.base.jmod data, not Java tools.
set -eu
if [ "$#" -ne 1 ]; then
    echo "usage: $0 /explicit/path/to/java.base.jmod" >&2
    exit 2
fi
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
input=$(realpath "$1")
# The compiled analyzer receives no Java home variables or executable search path.
env -i PATH=/nonexistent "$root/target/debug/examples/platform" "$input"
