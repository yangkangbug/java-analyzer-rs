#!/usr/bin/env python3
"""Opt-in javac declaration comparison. No downloads; no fixture execution."""
import argparse
import sys
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
ORACLE = ROOT / "dev-tools/oracle"
FILES = ("app/Use.java", "app/Declarations.java", "app/Payload.java",
         "lib/Base.java", "lib/Value.java", "lib/Holder.java", "model/Payload.java",
         "app/Calls.java", "lib/CallBase.java")
MARKER = re.compile(rb"/\*([DQ]):([a-z_]+)\*/([A-Za-z_$][A-Za-z0-9_$]*)")


def run(argv, env, success=True):
    result = subprocess.run([str(x) for x in argv], env=env, text=True, capture_output=True, timeout=120)
    if success and result.returncode:
        raise AssertionError(f"command failed ({result.returncode}): {argv}\n{result.stdout}{result.stderr}")
    return result


def records(output):
    result = {}
    for line in output.splitlines():
        cells = line.split("\t")
        assert cells[0] not in result, f"duplicate output: {cells[0]}"
        result[cells[0]] = cells[1:]
    return result


def compare(expected, queries, declarations, javac, native):
    assert set(expected) == set(queries) == set(javac) == set(native), "missing/extra query evidence"
    for query, (status, target_id) in expected.items():
        path, start, end, name = declarations[target_id]
        jpath, jkind, jowner, jname, jstart, jend = javac[query]
        assert (jpath, jname) == (path, name), f"{query}: javac declaration differs"
        assert int(jstart) <= start < end <= int(jend), f"{query}: javac declaration range differs"
        # The compiler's actual declaration tree must select exactly this marker,
        # not just some surrounding class/method that happens to contain it.
        contained = [key for key, (p, s, e, n) in declarations.items()
                     if p == jpath and n == jname and int(jstart) <= s < e <= int(jend)]
        assert contained == [target_id], f"{query}: non-unique declaration identity {contained}"
        actual_status, count, *targets = native[query]
        assert actual_status == status, f"{query}: native status {actual_status}, expected {status}"
        assert int(count) == len(targets), f"{query}: incomplete native records"
        if status == "Resolved":
            assert len(targets) == 1, f"{query}: expected one native target"
            npath, nkind, nowner, nname, nstart, nend = targets[0].split("|")
            kinds = {"CLASS": "Class", "FIELD": "Field", "METHOD": "Method",
                     "PARAMETER": "Parameter", "LOCAL_VARIABLE": "Local"}
            assert (npath, nkind, nname, int(nstart), int(nend)) == (path, kinds[jkind], name, start, end), f"{query}: native target differs: {targets}"
            expected_owner = "" if jkind in ("PARAMETER", "LOCAL_VARIABLE") else jowner
            assert nowner == expected_owner, f"{query}: native owner differs"
        elif status in ("OverloadNotResolved", "SignatureNotCompared"):
            assert int(count) >= 2, f"{query}: overload candidates missing"
        elif status in ("ReceiverNotBound", "Unresolved"):
            assert int(count) == 0, f"{query}: unsupported receiver fabricated target"
        else:
            raise AssertionError(f"unrecognized expectation status: {status}")


def mixed_suite(jdk, native_binary, env, work, helper, empty):
    """Compile dependencies only; compare consumer source declarations over binaries."""
    root = ORACLE / "mixed-v1"
    dependency_files = ("dep/Base.java", "dep/Generic.java", "dep/Payload.java", "other/Payload.java")
    source_files = ("app/Use.java", "app/GenericUse.java")
    for directory, files in (("dependency", dependency_files), ("source", source_files)):
        assert sorted(p.relative_to(root / directory).as_posix()
                      for p in (root / directory).rglob("*.java")) == sorted(files)
    binaries = work / "dependency-classes"
    binaries.mkdir()
    run([jdk / "bin/javac", "--release", "21", "-proc:none", "-implicit:none",
         "-encoding", "UTF-8", "-classpath", empty, "-sourcepath", empty,
         "-d", binaries, *[root / "dependency" / p for p in dependency_files]], env)
    assert sorted(p.relative_to(binaries).as_posix() for p in binaries.rglob("*.*")) == \
        sorted(p.replace(".java", ".class") for p in dependency_files), "binary inventory differs"
    declarations, queries = {}, {}
    requests = [f"F\t{path}" for path in source_files]
    for path in source_files:
        source = (root / "source" / path).read_bytes()
        source.decode("utf-8")
        matches = list(MARKER.finditer(source))
        assert source.count(b"/*D:") + source.count(b"/*Q:") == len(matches)
        for match in matches:
            kind, identity, name = [part.decode() for part in match.groups()]
            destination = declarations if kind == "D" else queries
            assert identity not in destination
            destination[identity] = (path, match.start(3), match.end(3), name)
            if kind == "Q":
                requests.append(f"Q\t{identity}\t{path}\t{match.start(3)}")
    expected = {}
    for line in (root / "expected.tsv").read_text().splitlines():
        identity, status, target = line.split("\t")
        assert identity not in expected and target in declarations
        expected[identity] = (status, target)
    request = work / "mixed-requests.tsv"
    request.write_text("\n".join(requests) + "\n")
    javac = records(run([jdk / "bin/java", "-cp", helper, "BindingOracle",
                         root / "source", request, binaries, empty], env).stdout)
    native = records(run([native_binary, root / "source", request, binaries], env).stdout)
    for query in expected:
        print(query + "\tjavac=" + "|".join(javac[query]) + "\tnative=" + "|".join(native[query]))
    compare(expected, queries, declarations, javac, native)
    # Refusal candidates must be precisely this source declaration and inherited
    # binary declaration, not merely any two same-name targets.
    for query, (status, target) in expected.items():
        if status == "Resolved":
            continue
        path, start, end, name = declarations[target]
        generic = query == "binary_generic_refusal"
        owner = "dep.Generic" if generic else "dep.Base"
        source_owner = "app.GenericUse" if generic else "app.Use"
        binary_path = binaries / ("dep/Generic.class" if generic else "dep/Base.class")
        targets = {f"{path}|Method|{source_owner}|{name}|{start}|{end}",
                   f"{binary_path}|Method|{owner}|{name}|0|0"}
        assert native[query][1] == "2" and set(native[query][2:]) == targets, \
            f"{query}: wrong mixed refusal candidates: {native[query]}"
    missing_binary = run([jdk / "bin/java", "-cp", helper, "BindingOracle",
                          root / "source", request, empty, empty], env, False)
    assert missing_binary.returncode == 2 and "compiler.err." in missing_binary.stderr, \
        "compiler unexpectedly found dependency sources/binaries outside explicit classpath"
    omitted = records(run([native_binary, root / "source", request], env).stdout)
    assert omitted["binary_distinct_package"][0:2] == ["Resolved", "1"], \
        "omitted-binary negative control did not change candidate evidence"
    try:
        compare(expected, queries, declarations, javac, omitted)
    except AssertionError:
        pass
    else:
        raise AssertionError("mixed comparison accepted omitted binary inputs")
    perturbed = dict(expected)
    perturbed["binary_scalar_override"] = ("Resolved", "mixed_array")
    try:
        compare(perturbed, queries, declarations, javac, native)
    except AssertionError:
        pass
    else:
        raise AssertionError("mixed comparison accepted wrong source declaration")
    print("PASS: 5 mixed-source/binary override bindings; 4 conservative refusals; "
          "exact refusal candidates, omitted-binary and wrong-target controls verified")


def main():
    if sys.flags.optimize:
        raise RuntimeError("oracle validation requires Python without -O/PYTHONOPTIMIZE")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--jdk", required=True, type=Path, help="checksum-verified Temurin 21.0.13+9 root")
    parser.add_argument("--native", type=Path, default=ROOT / "target/debug/examples/oracle_bindings")
    args = parser.parse_args()
    jdk = args.jdk.resolve()
    # Use only caller-selected pinned tools, with no inherited Java injection settings.
    env = {"PATH": "", "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8"}
    optimized = run([sys.executable, "-O", Path(__file__).resolve(), "--help"], env, False)
    assert optimized.returncode != 0 and "requires Python without -O" in optimized.stderr, "optimized Python bypassed guard"
    version = run([jdk / "bin/java", "-version"], env).stderr
    assert '21.0.13' in version and 'Temurin-21.0.13+9' in version, version
    fixture = ORACLE / "fixtures-v1"
    assert sorted(p.relative_to(fixture).as_posix() for p in fixture.rglob("*.java")) == sorted(FILES), "fixture file inventory changed"
    declarations, queries = {}, {}
    requests = [f"F\t{path}" for path in FILES]
    for path in FILES:
        source = (fixture / path).read_bytes()
        source.decode("utf-8")
        matches = list(MARKER.finditer(source))
        assert source.count(b"/*D:") + source.count(b"/*Q:") == len(matches), "malformed markers"
        for match in matches:
            kind, identity, name = [part.decode() for part in match.groups()]
            destination = declarations if kind == "D" else queries
            assert identity not in destination, f"duplicate marker {identity}"
            destination[identity] = (path, match.start(3), match.end(3), name)
            if kind == "Q":
                requests.append(f"Q\t{identity}\t{path}\t{match.start(3)}")
    expected = {}
    for line in (ORACLE / "expected.tsv").read_text().splitlines():
        if not line or line.startswith("#"):
            continue
        identity, status, target = line.split("\t")
        assert identity not in expected and target in declarations
        expected[identity] = (status, target)
    with tempfile.TemporaryDirectory(prefix="java-binding-oracle-") as temp:
        work = Path(temp)
        empty, classes = work / "empty", work / "helper"
        empty.mkdir()
        classes.mkdir()
        request = work / "requests.tsv"
        request.write_text("\n".join(requests) + "\n")
        run([jdk / "bin/javac", "--release", "21", "-proc:none", "-implicit:none", "-encoding", "UTF-8",
             "-classpath", empty, "-sourcepath", empty, "-d", classes, ORACLE / "BindingOracle.java"], env)
        command = [jdk / "bin/java", "-cp", classes, "BindingOracle", fixture, request, empty]
        javac = records(run(command, env).stdout)
        native = records(run([args.native.resolve(), fixture, request], env).stdout)
        compare(expected, queries, declarations, javac, native)
        # Reject both a sibling inside overlapping javac ranges and a same-name
        # field in the caller package instead of the declaration's imported type.
        # Invocation controls also reject same-spelled value declarations.
        for query, wrong_target in (("parameter", "shadow_field"),
                                    ("second_field", "field_first"),
                                    ("imported_scalar_member", "decoy_count"),
                                    ("parameter_name_call", "call_parameter"),
                                    ("local_name_call", "call_local"),
                                    ("field_name_call", "call_field"),
                                    ("nested_outer_call", "call_nested_field")):
            perturbed = dict(expected)
            perturbed[query] = ("Resolved", wrong_target)
            try:
                compare(perturbed, queries, declarations, javac, native)
            except AssertionError:
                pass
            else:
                raise AssertionError(f"oracle accepted deliberately wrong declaration: {query}")
        corrupted = {key: list(value) for key, value in javac.items()}
        corrupted["parameter"][4] = "0"
        corrupted["parameter"][5] = "1"
        try:
            compare(expected, queries, declarations, corrupted, native)
        except AssertionError:
            pass
        else:
            raise AssertionError("oracle accepted corrupted compiler declaration range")
        mixed_suite(jdk, args.native.resolve(), env, work, classes, empty)
        # Compiler failures must never become a green empty comparison.
        for invalid in ("Broken.java", "Missing.java", "NoCallable.java"):
            request.write_text(f"F\t{invalid}\n")
            bad = run([jdk / "bin/java", "-cp", classes, "BindingOracle", ORACLE / "invalid", request, empty], env, False)
            assert bad.returncode == 2 and "compiler.err." in bad.stderr, f"invalid fixture accepted: {invalid}"
        missing_source = (ORACLE / "invalid/Missing.java").read_bytes()
        missing_offset = missing_source.index(b"absent")
        request.write_text(f"F\tMissing.java\nQ\tmissing\tMissing.java\t{missing_offset}\n")
        negative = records(run([args.native.resolve(), ORACLE / "invalid", request], env).stdout)
        assert negative == {"missing": ["Unresolved", "0"]}, "missing name produced a native target"
        no_callable_source = (ORACLE / "invalid/NoCallable.java").read_bytes()
        no_callable_queries = [(match.group(2).decode(), match.start(3))
                               for match in MARKER.finditer(no_callable_source)]
        assert no_callable_source.count(b"/*Q:") == len(no_callable_queries) == 5, \
            "no-callable marker inventory changed"
        assert {identity for identity, _ in no_callable_queries} == {
            "field_only_call", "parameter_only_call", "local_only_call",
            "class_only_call", "unrelated_method_call"}, "no-callable query inventory changed"
        request.write_text("F\tNoCallable.java\n" + "".join(
            f"Q\t{identity}\tNoCallable.java\t{offset}\n"
            for identity, offset in no_callable_queries))
        no_callable = records(run([args.native.resolve(), ORACLE / "invalid", request], env).stdout)
        assert no_callable == {identity: ["Unresolved", "0"] for identity, _ in no_callable_queries}, \
            f"non-method names produced invocation targets: {no_callable}"
        print(f"PASS: {sum(s == 'Resolved' for s, _ in expected.values())} supported declaration bindings; "
              f"{sum(s != 'Resolved' for s, _ in expected.values())} explicit unsupported boundaries; "
              "wrong-target detection, 3 invalid/incomplete compiler failures, "
              "and 5 no-callable refusals verified")
        for query in expected:
            print(query + "\tjavac=" + "|".join(javac[query]) + "\tnative=" + "|".join(native[query]))


if __name__ == "__main__":
    main()
