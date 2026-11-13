#!/usr/bin/env python3
"""Optional compiler-only traditional-switch scope oracle. Never executes fixtures."""
import argparse
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "dev-tools/oracle/traditional-switch-v1"
BASELINE_SHA = "47c1724ac3a59c537047f6ef14adf31d019ea578a7660288353f6ee0d69f7e5a"

def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / "scripts" / filename)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result

semantic = module("traditional_switch_semantic", "check-semantic-oracle.py")

def require(ok, message):
    if not ok:
        raise AssertionError(message)

def expected_native(predictions, declarations):
    kinds = {"PARAMETER": "Parameter", "LOCAL_VARIABLE": "Local", "FIELD": "Field"}
    result = {}
    for query, (status, target, kind, owner) in predictions.items():
        if status == "Unresolved":
            result[query] = [status, "0"]
        else:
            require(status == "Resolved", "invalid predicted status")
            path, start, end, name = declarations[target]
            public_owner = "" if kind in ("PARAMETER", "LOCAL_VARIABLE") else owner
            result[query] = [status, "1", f"{path}|{kinds[kind]}|{public_owner}|{name}|{start}|{end}"]
    return result

def compare_native(actual, expected):
    require(actual == expected, "complete native query records differ")

def check_compiler(predictions, declarations, queries, compiler):
    require(set(predictions) == set(queries) == set(compiler), "compiler query completeness")
    for query, (_, target, kind, owner) in predictions.items():
        path, start, end, name = declarations[target]
        row = compiler[query]
        require(len(row) == 6 and row[:4] == [path, kind, owner, name], "compiler identity " + query)
        a, b = map(int, row[4:])
        contained = [key for key, (p, s, e, n) in declarations.items()
                     if p == path and n == name and a <= s < e <= b]
        require(contained == [target], "compiler declaration marker " + query)
        require(a <= start < end <= b, "compiler declaration range " + query)

def mutation_controls(expected):
    query = next(q for q, row in expected.items() if row[0] == "Resolved")
    refused = next(q for q, row in expected.items() if row[0] == "Unresolved")
    mutations = []
    changed = copy.deepcopy(expected); del changed[query]; mutations.append(changed)
    changed = copy.deepcopy(expected); changed["extra"] = changed[query]; mutations.append(changed)
    changed = copy.deepcopy(expected); changed[query][0] = "Ambiguous"; mutations.append(changed)
    changed = copy.deepcopy(expected); changed[query].append(changed[query][2]); mutations.append(changed)
    changed = copy.deepcopy(expected); changed[query][2] += "wrong"; mutations.append(changed)
    changed = copy.deepcopy(expected); changed[refused] = changed[query]; mutations.append(changed)
    for changed in mutations:
        try:
            compare_native(changed, expected)
        except AssertionError:
            continue
        raise AssertionError("mutation accepted")
    return len(mutations)

def read_inputs():
    require(hashlib.sha256((FIXTURE / "manifest.json").read_bytes()).hexdigest() == "889d07ed146a34baa227aeed92cf45d4f9a59c54ca95921679d9bfa833a1ac21", "frozen manifest changed")
    require(hashlib.sha256((ROOT / "dev-tools/oracle/BindingOracle.java").read_bytes()).hexdigest() == "4a64c6e1a85a8e0160e49863032015b6f25e205f42bed2ce83013181489862b6", "compiler helper changed")
    manifest = json.loads((FIXTURE / "manifest.json").read_text())
    actual = sorted(p.relative_to(FIXTURE).as_posix() for p in FIXTURE.rglob("*.java"))
    require(actual == sorted(n for n in manifest if n.endswith(".java")), "fixture inventory changed")
    for name, digest in manifest.items():
        require(hashlib.sha256((FIXTURE / name).read_bytes()).hexdigest() == digest, "fixture hash " + name)
    predictions = json.loads((FIXTURE / "predictions.json").read_text())
    declarations, queries, requests = semantic.fixture_markers(FIXTURE / "source",
        ("Containment.java", "Enums.java", "Flow.java"))
    require(set(predictions) == set(queries), "prediction completeness")
    return predictions, declarations, queries, requests

def main():
    if sys.flags.optimize:
        raise RuntimeError("oracle validation requires Python without -O/PYTHONOPTIMIZE")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--jdk", required=True, type=Path)
    parser.add_argument("--jdk-archive", required=True, type=Path)
    parser.add_argument("--native", required=True, type=Path, help="oracle_bindings executable")
    parser.add_argument("--baseline", action="store_true")
    parser.add_argument("--capture-baseline", action="store_true")
    args = parser.parse_args()
    predictions, declarations, queries, requests = read_inputs()
    native = args.native.resolve(strict=True)
    digest = hashlib.sha256(native.read_bytes()).hexdigest()
    if args.baseline or args.capture_baseline:
        require(digest == BASELINE_SHA, "exact pre-implementation executable required")
    contract_path = FIXTURE / "contract.json"
    if args.capture_baseline:
        require(not contract_path.exists(), "refuse overwrite of frozen contract")
    verifier = module("traditional_switch_jdk", "check-strict-reference-oracle.py")
    jdk = args.jdk.resolve()
    print("verified-jdk=" + verifier.verify_jdk(jdk, args.jdk_archive.resolve()), flush=True)
    env = {"PATH": "", "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8"}
    optimized = semantic.run([sys.executable, "-O", Path(__file__).resolve(), "--help"], env, False)
    require(optimized.returncode != 0 and "requires Python without -O" in optimized.stderr, "-O bypass")
    with tempfile.TemporaryDirectory(prefix="switch-oracle-") as tmp:
        work = Path(tmp); helper = work / "helper"; empty = work / "empty"
        helper.mkdir(); empty.mkdir()
        request = work / "requests.tsv"; request.write_text(requests)
        semantic.run([jdk / "bin/javac", "--release", "21", "-proc:none", "-implicit:none",
            "-encoding", "UTF-8", "-classpath", empty, "-sourcepath", empty, "-d", helper,
            ROOT / "dev-tools/oracle/BindingOracle.java"], env)
        compiler = semantic.records(semantic.run([jdk / "bin/java", "-cp", helper, "BindingOracle",
            FIXTURE / "source", request, empty], env).stdout)
        check_compiler(predictions, declarations, queries, compiler)
        invalid = {}
        for case in ("leak", "duplicate_local", "duplicate_case"):
            bad_request = work / (case + ".tsv"); bad_request.write_text("F\tBad.java\n")
            run = semantic.run([jdk / "bin/java", "-cp", helper, "BindingOracle",
                FIXTURE / "invalid" / case, bad_request, empty], env, False)
            require(run.returncode == 2 and "compiler.err." in run.stderr, "compiler must reject " + case)
            invalid[case] = sorted(line.split(":")[0] for line in run.stderr.splitlines() if "compiler.err." in line)
        actual = semantic.records(semantic.run([native, FIXTURE / "source", request],
            {"PATH": "/nonexistent"}).stdout)
        candidate = expected_native(predictions, declarations)
        if args.capture_baseline:
            contract_path.write_text(json.dumps({"baseline_sha256": BASELINE_SHA,
                "baseline": actual, "candidate": candidate, "compiler": compiler,
                "compiler_rejections": invalid}, indent=2) + "\n")
        require(hashlib.sha256(contract_path.read_bytes()).hexdigest() == "089033089e425d7acddb64423a9b59ab7b494a7259d9d8a098a85156d5117349", "frozen contract changed")
        contract = json.loads(contract_path.read_text())
        require(contract["baseline_sha256"] == BASELINE_SHA, "baseline identity changed")
        require(contract["candidate"] == candidate, "candidate differs from handwritten predictions")
        require(contract["compiler"] == compiler and contract["compiler_rejections"] == invalid, "compiler drift")
        compare_native(actual, contract["baseline"] if args.baseline or args.capture_baseline else candidate)
        controls = mutation_controls(candidate)
        print(json.dumps({"baseline": args.baseline or args.capture_baseline, "queries": len(queries),
            "supported": sum(x[0] == "Resolved" for x in candidate.values()),
            "refusals": sum(x[0] == "Unresolved" for x in candidate.values()),
            "compiler_rejected_sources": len(invalid), "mutation_controls": controls,
            "native_sha256": digest}))
    print("PASS: switch scope declarations, shared labels, nested bodies, label/local-class refusals; compiler only")

if __name__ == "__main__":
    main()
