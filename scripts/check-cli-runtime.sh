#!/bin/sh
# Build the optional native binary first. No network or Java is used here.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
python3 - "$root" <<'PY'
import json
from pathlib import Path
import subprocess
import sys
import tempfile

root = Path(sys.argv[1])
native = root / "target/debug/java-analyzer"

def require(condition, message):
    if not condition:
        raise RuntimeError(message)

with tempfile.TemporaryDirectory(prefix="java-native-cli-") as directory:
    work = Path(directory).resolve()
    (work / "src").mkdir()
    (work / "classes/lib").mkdir(parents=True)
    (work / "classes/lib/Lib.class").write_bytes((root / "tests/fixtures/classes/Lib.class").read_bytes())
    source = "import lib.Lib; class Smoke { int answer; int read(Lib lib) { lib.run(); return answer; } }\n"
    (work / "src/Smoke.java").write_text(source, encoding="utf-8")
    compilation = {"project": "smoke", "source_set": "main"}
    manifest = work / "project.json"
    manifest.write_text(json.dumps({"schema_version": 1, "compilations": [{
        "id": compilation, "source_roots": ["src"],
        "classpath": [{"kind": "directory", "path": "classes"}],
    }]}), encoding="utf-8")
    requests = work / "requests.json"
    requests.write_text(json.dumps({"schema_version": 1, "queries": [{
        "compilation": compilation, "file": "src/Smoke.java",
        "position": {"encoding": "utf8", "offset": offset},
    } for offset in [source.rindex("answer"), source.index("run()")]]}), encoding="utf-8")

    def run(command, *args):
        result = subprocess.run([str(native), command, "--manifest", str(manifest), *args],
                                cwd=root, env={"PATH": "/nonexistent"},
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                timeout=15, check=False)
        require(result.returncode == 0, result.stderr.decode("utf-8", errors="replace"))
        require(not result.stderr, "unexpected stderr")
        data = json.loads(result.stdout)
        require(data["schema_version"] == 1, "unexpected output version")
        require(data["capabilities"]["runtime"] == "native_rust_no_jvm", "runtime boundary missing")
        return data["result"]

    inventory = run("inspect")["compilations"]
    require(len(inventory) == 1 and len(inventory[0]["source_paths"]) == 1, "source inventory differs")
    require(inventory[0]["class_count"] == 1 and "class_origins" not in inventory[0],
            "summary binary count or detail contract differs")
    configured = subprocess.run([str(native), "configure", "--compilation", "smoke", "main",
                                 "--source-root", "src", "--classpath-dir", "classes", "--no-platform"],
                                cwd=work, env={"PATH": "/nonexistent"},
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15, check=False)
    require(configured.returncode == 0 and not configured.stderr,
            "native explicit configuration failed: " + configured.stderr.decode("utf-8", errors="replace"))
    configuration = json.loads(configured.stdout)
    require(configuration["schema_version"] == 1 and
            configuration["compilations"][0]["source_roots"] == [str(work / "src")],
            "configuration did not preserve absolute source roots")
    manifest = work / "generated-project.json"
    manifest.write_bytes(configured.stdout)
    inventory[0]["configuration"]["source_roots"] = [str(work / "src")]
    inventory[0]["configuration"]["classpath"] = [{"kind": "directory", "path": str(work / "classes")}]
    require(run("inspect")["compilations"] == inventory, "generated configuration changed inventory")
    declaration = run("declaration", "--requests", str(requests))
    require(declaration["index_builds"] == 1, "batch did not use one prepared snapshot")
    queries = declaration["queries"]
    # No Object.class is provided: preserve the incomplete-platform control.
    # A unique binary name is no longer proof of invocation applicability.
    require(len(queries) == 2 and [q["status"] for q in queries] == ["Resolved", "CallNotResolved"],
            "unexpected source selection or incomplete-platform binary refusal")
    source_target = queries[0]["targets"]
    require(len(source_target) == 1, "source target count differs")
    target = source_target[0]
    require(target["origin"] == "source" and target["source_available"], "source provenance differs")
    require(target["range"]["start_byte"] == source.index("answer") and
            target["range"]["end_byte"] == source.index("answer") + len("answer"), "wrong source name range")
    binary_targets = queries[1]["targets"]
    require(len(binary_targets) == 1, "binary target count differs")
    target = binary_targets[0]
    require(target["origin"] == "binary" and not target["source_available"] and "range" not in target,
            "binary target fabricated a source range")
    require(target["owner"] == "lib.Lib" and target["name"] == "run", "wrong binary declaration")
    reports = run("parser-report")
    require(reports["classification"] == "parser_limit_or_invalid_syntax" and
            not reports["java_semantic_diagnostics"], "parser report advertised semantic diagnostics")
    require(reports["source_count"] == 1 and reports["report_count"] == 0, "unexpected parser inventory")

print("PASS: native configuration, inventory, prepared source selection/binary refusal batch and parser report with no Java environment")
PY
