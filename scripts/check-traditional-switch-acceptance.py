#!/usr/bin/env python3
"""Complete pinned Commons/Gson parser comparison; native-only, no upstream execution."""
import argparse
import importlib.util
import json
import sys
from pathlib import Path
import tempfile

ROOT = Path(__file__).resolve().parents[1]

def module(name, file):
    spec = importlib.util.spec_from_file_location(name, ROOT / "scripts" / file)
    result = importlib.util.module_from_spec(spec)
    sys.modules[name] = result
    spec.loader.exec_module(result)
    return result

contract = module("switch_parser", "traditional-switch-parser-contract.py")
commons = module("switch_commons", "check-cli-acceptance.py")
second = module("switch_second", "check-second-project.py")

def portable(result, root):
    for row in result["files"]:
        row["path"] = Path(row["path"]).relative_to(root).as_posix()
    return result

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("sources_jar", type=Path)
    parser.add_argument("java_base_jmod", type=Path)
    parser.add_argument("gson_inputs", type=Path)
    parser.add_argument("--native", type=Path, required=True)
    parser.add_argument("--baseline", action="store_true")
    args = parser.parse_args()
    native = args.native.resolve(strict=True)
    stage = "baseline" if args.baseline else "candidate"
    spec = json.loads((ROOT / "scripts/acceptance-inputs.json").read_text())
    with tempfile.TemporaryDirectory(prefix="traditional-switch-acceptance-") as tmp:
        work = Path(tmp)
        source, notices, cwd = work / "sources", work / "notices", work / "cwd"
        for directory in (source, notices, cwd): directory.mkdir()
        sources = commons.extract_sources(args.sources_jar.resolve(), spec["sources"], source, notices)
        platform = args.java_base_jmod.resolve(strict=True)
        commons.same(commons.sha256(platform), spec["jdk"]["jmod_sha256"], "platform pin")
        manifest = work / "project.json"
        manifest.write_text(json.dumps({"schema_version": 1, "compilations": [{
            "id": commons.COMPILATION, "source_roots": ["sources"], "dependencies": [], "classpath": [],
            "target_release": 21, "platform": [{"kind": "jmod", "path": str(platform)}]}]}))
        result = commons.run_cli(native, cwd, manifest, "parser-report", 90, 20 * 1024 * 1024)["result"]
        contract.check("commons", portable(result, source), sources, stage)
        gson_work = work / "gson"; gson_work.mkdir()
        fixture = second.prepare_fixture(args.gson_inputs.resolve(), gson_work)
        result = second.successful(native, fixture, "parser-report", strict_json=True)["result"]
        contract.check("gson", portable(result, fixture.source_root),
                       {name: (fixture.source_root / name).read_bytes()
                        for name in second.all_source_hashes(fixture)}, stage)
    print("PASS: exact complete Commons/Gson parser records and source hashes; stage=" + stage)

if __name__ == "__main__": main()
