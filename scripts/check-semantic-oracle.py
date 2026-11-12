#!/usr/bin/env python3
"""Opt-in javac declaration comparison. No downloads; no fixture execution."""
import argparse
import hashlib
import json
import zipfile
import sys
from pathlib import Path, PurePosixPath
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


def local_type_suite(jdk, native_binary, env, work, helper, empty):
    """Contain unsupported local classes without claiming bindings inside them."""
    root = ORACLE / "local-type-v1"
    declarations, queries, requests = fixture_markers(root / "source", ("Containment.java",))
    expected = {}
    for query, status, target in rows(root / "expected.tsv", 3):
        assert query not in expected and target in declarations, "duplicate or invalid local-type golden"
        expected[query] = (status, target)
    assert len(expected) == len(queries) == 15
    assert sum(status == "Resolved" for status, _ in expected.values()) == 11
    request = work / "local-type-requests.tsv"
    request.write_text(requests)
    javac = records(run([jdk / "bin/java", "-cp", helper, "BindingOracle",
                         root / "source", request, empty], env).stdout)
    native = records(run([native_binary, root / "source", request], {"PATH": "/nonexistent"}).stdout)
    compiler_targets = {
        "before_local": ("LOCAL_VARIABLE", "oracle.Containment"),
        "outer_parameter": ("PARAMETER", "oracle.Containment"),
        "outer_field": ("FIELD", "oracle.Containment"),
        "after_local": ("LOCAL_VARIABLE", "oracle.Containment"),
        "block_local": ("LOCAL_VARIABLE", "oracle.Containment"),
        "local_type": ("CLASS", "Hidden"),
        "hidden_field": ("FIELD", "Hidden"),
        "hidden_parameter": ("PARAMETER", "Hidden"),
        "later_parameter": ("PARAMETER", "oracle.Containment"),
        "later_method": ("METHOD", "oracle.Containment"),
    }
    assert set(javac) == set(expected), "local-type compiler query completeness"
    for query, (_, target) in expected.items():
        assert tuple(javac[query][1:3]) == compiler_targets[target], f"{query}: compiler kind/owner differs"
    for query in expected:
        print(query + "\tjavac=" + "|".join(javac[query]) + "\tnative=" + "|".join(native[query]))
    compare(expected, queries, declarations, javac, native)
    wrong = dict(expected)
    wrong["after_shadow"] = ("Resolved", "outer_field")
    must_reject(lambda: compare(wrong, queries, declarations, javac, native), "local type wrong shadow target")
    for label in ("missing", "extra", "owner", "range", "unsupported_target"):
        changed = {key: list(value) for key, value in native.items()}
        if label == "missing":
            del changed["after_type"]
        elif label == "extra":
            changed["not_requested"] = native["after_type"]
        elif label == "unsupported_target":
            changed["inside_capture"] = native["after_type"]
        else:
            parts = changed["after_type"][2].split("|")
            if label == "owner":
                parts[2] = "oracle.Containment"
            else:
                parts[-2:] = ["0", "1"]
            changed["after_type"][2] = "|".join(parts)
        must_reject(lambda: compare(expected, queries, declarations, javac, changed), "local type " + label)
    print("PASS: 11 local-type surrounding bindings; four explicit unsupported type/body queries; mutation controls")


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


def fixture_markers(root, files):
    """Read fixed handwritten inputs, never generate goldens from native output."""
    assert sorted(p.relative_to(root).as_posix() for p in root.rglob("*.java")) == sorted(files), \
        "primitive fixture file inventory differs"
    declarations, queries = {}, {}
    requests = [f"F\t{path}" for path in files]
    for path in files:
        source = (root / path).read_bytes()
        source.decode("utf-8")
        matches = list(MARKER.finditer(source))
        assert source.count(b"/*D:") + source.count(b"/*Q:") == len(matches), "malformed primitive markers"
        for match in matches:
            kind, identity, name = (part.decode() for part in match.groups())
            destination = declarations if kind == "D" else queries
            assert identity not in destination, f"duplicate primitive marker: {identity}"
            destination[identity] = (path, match.start(3), match.end(3), name)
            if kind == "Q":
                requests.append(f"Q\t{identity}\t{path}\t{match.start(3)}")
    return declarations, queries, "\n".join(requests) + "\n"


def rows(path, width):
    result = []
    for line in path.read_text().splitlines():
        if not line or line.startswith("#"):
            continue
        cells = line.split("\t")
        assert len(cells) == width, f"malformed golden row: {path}"
        result.append(cells)
    return result


def marker_target(declarations, candidate):
    marker, owner = candidate.split("@")
    path, start, end, name = declarations[marker]
    return f"{path}|Method|{owner}|{name}|{start}|{end}"


def check_candidates(query, status, candidates, declarations, actual):
    actual_status, count, *targets = actual
    assert actual_status == status, f"{query}: native status {actual_status}, expected {status}"
    assert int(count) == len(targets), f"{query}: incomplete native candidate records"
    wanted = [] if candidates == "-" else [marker_target(declarations, item) for item in candidates.split(",")]
    assert len(set(wanted)) == len(wanted), f"{query}: duplicate expected candidate"
    assert sorted(targets) == sorted(wanted), f"{query}: exact native candidates differ: {targets}"


def compare_primitive(expected, queries, declarations, javac, native):
    assert set(expected) == set(queries) == set(javac) == set(native), "missing/extra primitive query evidence"
    for query, (status, selected, candidates) in expected.items():
        target_id, target_owner = selected.split("@")
        path, start, end, name = declarations[target_id]
        jpath, jkind, jowner, jname, jstart, jend = javac[query]
        assert (jpath, jkind, jname) == (path, "METHOD", name), f"{query}: compiler target differs"
        assert int(jstart) <= start < end <= int(jend), f"{query}: compiler range differs"
        contained = [key for key, (p, s, e, n) in declarations.items()
                     if p == jpath and n == jname and int(jstart) <= s < e <= int(jend)]
        assert contained == [target_id], f"{query}: non-unique compiler declaration identity"
        assert jowner == target_owner, f"{query}: compiler owner differs"
        assert status in ("Resolved", "OverloadNotResolved", "SignatureNotCompared", "CallNotResolved", "ReceiverNotBound", "Unresolved")
        if status == "Resolved":
            assert candidates == f"{target_id}@{jowner}", f"{query}: resolved golden needs one compiler target"
        elif status in ("ReceiverNotBound", "Unresolved"):
            assert candidates == "-", f"{query}: unsupported receiver must have no native target"
        elif status == "CallNotResolved":
            assert candidates != "-" and len(candidates.split(",")) == 1
        else:
            assert candidates != "-" and len(candidates.split(",")) >= 2
        check_candidates(query, status, candidates, declarations, native[query])


def must_reject(check, label):
    try:
        check()
    except AssertionError:
        return
    raise AssertionError(f"comparator accepted {label}")


def pinned_object(jdk, work):
    """Extract one explicit platform class as data; never run a JMOD tool."""
    pins = json.loads((ROOT / "scripts/acceptance-inputs.json").read_text())
    jmod = jdk / "jmods/java.base.jmod"
    with jmod.open("rb") as source:
        assert hashlib.file_digest(source, "sha256").hexdigest() == pins["jdk"]["jmod_sha256"], \
            "oracle platform JMOD checksum differs"
    platform = work / "primitive-platform"
    target = platform / "java/lang/Object.class"
    target.parent.mkdir(parents=True)
    with zipfile.ZipFile(jmod) as archive:
        info = archive.getinfo("classes/java/lang/Object.class")
        assert 0 < info.file_size < 1024 * 1024
        target.write_bytes(archive.read(info))
    return platform


def current_receiver_guard_expectations(suite, expected, queries, declarations):
    import importlib.util
    spec = importlib.util.spec_from_file_location('receiver_guard_contract', ROOT / 'scripts/receiver-guard-oracle-contract.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.apply(ORACLE, suite, expected, queries, declarations)


def current_null_expectations(suite, expected, queries, declarations):
    import importlib.util
    spec = importlib.util.spec_from_file_location('null_oracle_contract', ROOT / 'scripts/null-oracle-contract.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.apply(ORACLE, suite, expected, queries, declarations)


def primitive_suite(jdk, native_binary, env, work, helper, empty, platform):
    """Source-only strict primitive selections, plus explicit unknown/invalid cases."""
    root = ORACLE / "primitive-v1"
    files = ("PrimitiveCalls.java", "Boundaries.java", "SingletonCalls.java")
    declarations, queries, requests = fixture_markers(root / "source", files)
    expected = {}
    for query, status, target, candidates in rows(root / "expected.tsv", 4):
        assert query not in expected and target.split("@")[0] in declarations
        expected[query] = (status, target, candidates)
    assert len(expected) == len(queries) == 32
    assert sum(value[0] == "Resolved" for value in expected.values()) == 12
    # Keep the original inputs and golden history intact. These three previously
    # refused calls now consume independently proven primitive scalar results.
    updates = rows(ORACLE / "return-v1/primitive-updates.tsv", 5)
    assert len(updates) == 3 and {row[0] for row in updates} == {
        "nested_outer_refusal", "call_expression_refusal", "single_result_refusal"}
    for query, before, after, selected, candidates in updates:
        assert expected[query][0] == before and expected[query][1] == selected
        assert after == "Resolved" and selected == candidates
        expected[query] = (after, selected, candidates)
    expected = current_receiver_guard_expectations("primitive-v1", expected, queries, declarations)
    request = work / "primitive-requests.tsv"
    request.write_text(requests)
    javac = records(run([jdk / "bin/java", "-cp", helper, "BindingOracle",
                         root / "source", request, empty], env).stdout)
    native = records(run([native_binary, root / "source", request, platform], {"PATH": "/nonexistent"}).stdout)

    # Analyze each invalid fixture independently. Compiler errors must not be
    # converted into successful empty binding comparisons.
    invalid = rows(root / "invalid.tsv", 4)
    assert len(invalid) == 16 and len({row[0] for row in invalid}) == len(invalid)
    assert sorted(p.name for p in (root / "invalid").glob("*.java")) == sorted(row[0] for row in invalid)
    for file, query, status, candidates in invalid:
        source = (root / "invalid" / file).read_bytes()
        matches = list(MARKER.finditer(source))
        assert source.count(b"/*D:") + source.count(b"/*Q:") == len(matches)
        ds, qs = {}, {}
        for match in matches:
            kind, identity, name = (part.decode() for part in match.groups())
            destination = ds if kind == "D" else qs
            assert identity not in destination
            destination[identity] = (file, match.start(3), match.end(3), name)
        assert set(qs) == {query}
        request.write_text(f"F\t{file}\nQ\t{query}\t{file}\t{qs[query][1]}\n")
        rejected = run([jdk / "bin/java", "-cp", helper, "BindingOracle",
                        root / "invalid", request, empty], env, False)
        assert rejected.returncode == 2 and "compiler.err." in rejected.stderr and not rejected.stdout, \
            f"invalid primitive fixture accepted: {file}"
        actual = records(run([native_binary, root / "invalid", request, platform],
                             {"PATH": "/nonexistent"}).stdout)
        assert set(actual) == {query}
        check_candidates(query, status, candidates, ds, actual[query])
        if status == "CallNotResolved":
            omitted = records(run([native_binary, root / "invalid", request],
                                  {"PATH": "/nonexistent"}).stdout)
            assert set(omitted) == {query}
            check_candidates(query, status, candidates, ds, omitted[query])
    print("PASS: 16 rejected compiler fixtures with exact native refusal candidates; "
          "includes three formerly Resolved singleton invalid calls")
    compare_primitive(expected, queries, declarations, javac, native)

    # Same-name overload mutations must be caught by source identity, not name.
    wrong = dict(expected)
    wrong["float_array"] = ("Resolved", "pick_double@oracle.PrimitiveCalls", "pick_double@oracle.PrimitiveCalls")
    must_reject(lambda: compare_primitive(wrong, queries, declarations, javac, native), "wrong primitive overload")
    corrupted = {key: list(value) for key, value in javac.items()}
    corrupted["float_array"][-2:] = ["0", "1"]
    must_reject(lambda: compare_primitive(expected, queries, declarations, corrupted, native), "corrupted compiler range")
    for label in ("owner", "range", "duplicate", "missing", "extra", "refusal_candidate"):
        changed = {key: list(value) for key, value in native.items()}
        if label == "owner":
            changed["float_array"][2] = changed["float_array"][2].replace("oracle.PrimitiveCalls", "oracle.Wrong")
        elif label == "range":
            parts = changed["float_array"][2].split("|")
            parts[-2:] = ["0", "1"]
            changed["float_array"][2] = "|".join(parts)
        elif label == "duplicate":
            changed["float_array"][1] = "2"
            changed["float_array"].append(changed["float_array"][2])
        elif label == "missing":
            del changed["float_array"]
        elif label == "extra":
            changed["not_requested"] = changed["float_array"]
        else:
            changed["varargs_refusal"][1] = "1"
            changed["varargs_refusal"].pop()
        must_reject(lambda: compare_primitive(expected, queries, declarations, javac, changed), label)
    request.write_text(requests)
    omitted = records(run([native_binary, root / "source", request], {"PATH": "/nonexistent"}).stdout)
    newly_supported = [query for query, row in expected.items()
                       if row[0] == "Resolved" and not query.startswith("single_")]
    assert len(newly_supported) == 12 and set(omitted) == set(expected)
    assert all(omitted[q][0] != "Resolved" for q in newly_supported), "missing Object did not block new selections"
    must_reject(lambda: compare_primitive(expected, queries, declarations, javac, omitted), "omitted Object input")
    print("PASS: 15 strict primitive source bindings; 17 conservative refusals; "
          "three reviewed scalar-return updates to the unchanged 32-query source corpus; "
          "exact candidates, target mutations, omitted Object and query-completeness controls verified")


def return_suite(jdk, native_binary, env, work, helper, empty, platform):
    """Declared scalar results of recursively proven calls, never legacy navigation."""
    root = ORACLE / "return-v1"
    files = ("ReturnCalls.java", "ReturnBoundaries.java", "ReturnBudget.java")
    declarations, queries, requests = fixture_markers(root / "source", files)
    expected = {}
    for query, status, target, candidates in rows(root / "expected.tsv", 4):
        assert query not in expected and target.split("@")[0] in declarations
        expected[query] = (status, target, candidates)
    assert len(expected) == len(queries) == 29
    assert sum(row[0] == "Resolved" for row in expected.values()) == 14
    request = work / "return-requests.tsv"
    request.write_text(requests)
    javac = records(run([jdk / "bin/java", "-cp", helper, "BindingOracle",
                         root / "source", request, empty], env).stdout)
    native = records(run([native_binary, root / "source", request, platform], {"PATH": "/nonexistent"}).stdout)
    compare_primitive(expected, queries, declarations, javac, native)

    invalid = rows(root / "invalid.tsv", 4)
    assert len(invalid) == 12 and len({row[0] for row in invalid}) == len(invalid)
    assert sorted(p.name for p in (root / "invalid").glob("*.java")) == sorted(row[0] for row in invalid)
    for file, query, status, candidates in invalid:
        source = (root / "invalid" / file).read_bytes()
        matches = list(MARKER.finditer(source))
        assert source.count(b"/*D:") + source.count(b"/*Q:") == len(matches)
        ds, qs = {}, {}
        for match in matches:
            kind, identity, name = (part.decode() for part in match.groups())
            destination = ds if kind == "D" else qs
            assert identity not in destination
            destination[identity] = (file, match.start(3), match.end(3), name)
        assert set(qs) == {query}
        request.write_text(f"F\t{file}\nQ\t{query}\t{file}\t{qs[query][1]}\n")
        rejected = run([jdk / "bin/java", "-cp", helper, "BindingOracle",
                        root / "invalid", request, empty], env, False)
        assert rejected.returncode == 2 and "compiler.err." in rejected.stderr and not rejected.stdout, \
            f"invalid return fixture accepted: {file}"
        actual = records(run([native_binary, root / "invalid", request, platform],
                             {"PATH": "/nonexistent"}).stdout)
        assert set(actual) == {query}
        check_candidates(query, status, candidates, ds, actual[query])

    # Swapping inner parameter/return types must not make a return type select
    # the inner overload. The enclosing call really needs the double overload.
    wrong = dict(expected)
    wrong["return_swapped_type"] = ("Resolved", "return_pick_float@oracle.ReturnCalls",
                                    "return_pick_float@oracle.ReturnCalls")
    must_reject(lambda: compare_primitive(wrong, queries, declarations, javac, native), "return-driven inner overload")
    corrupted = {key: list(value) for key, value in javac.items()}
    corrupted["return_swapped_type"][-2:] = ["0", "1"]
    must_reject(lambda: compare_primitive(expected, queries, declarations, corrupted, native), "corrupted return compiler range")
    for label in ("owner", "range", "duplicate", "missing", "extra", "refusal_candidate"):
        changed = {key: list(value) for key, value in native.items()}
        if label == "owner":
            changed["return_swapped_type"][2] = changed["return_swapped_type"][2].replace("oracle.ReturnCalls", "oracle.Wrong")
        elif label == "range":
            parts = changed["return_swapped_type"][2].split("|")
            parts[-2:] = ["0", "1"]
            changed["return_swapped_type"][2] = "|".join(parts)
        elif label == "duplicate":
            changed["return_swapped_type"][1] = "2"
            changed["return_swapped_type"].append(changed["return_swapped_type"][2])
        elif label == "missing":
            del changed["return_swapped_type"]
        elif label == "extra":
            changed["not_requested"] = changed["return_swapped_type"]
        else:
            changed["return_generic_refusal"][1] = "1"
            changed["return_generic_refusal"].pop()
        must_reject(lambda: compare_primitive(expected, queries, declarations, javac, changed), f"return {label}")
    request.write_text(requests)
    omitted = records(run([native_binary, root / "source", request], {"PATH": "/nonexistent"}).stdout)
    assert set(omitted) == set(expected)
    # The singleton may retain legacy name navigation without Object. It must
    # never become evidence for an enclosing call's result type.
    selections = [query for query, row in expected.items()
                  if row[0] == "Resolved" and query != "return_singleton"]
    assert len(selections) == 13
    assert all(omitted[query][0] != "Resolved" for query in selections), "missing Object did not block return selections"
    must_reject(lambda: compare_primitive(expected, queries, declarations, javac, omitted), "omitted return Object input")
    print("PASS: 14 scalar-result source bindings (12 newly selectable, two controls); "
          "15 valid-source refusals; 12 compiler-invalid refusals; "
          "exact candidates, target mutations, omitted Object and query-completeness controls verified")


REFERENCE_FILES = (
    "model/Root.java", "model/Mid.java", "model/Leaf.java", "model/Other.java",
    "model/Marker.java", "model/Branch.java", "model/Target.java",
    "model/GenericReceiver.java", "model/Parent.java", "model/InheritedReceiver.java",
    "model/GenericMethods.java", "model/BinaryReceiver.java", "app/Use.java",
)
REFERENCE_DECLARATIONS = frozenset((
    "pick_root", "pick_mid", "pick_other", "exact_leaf", "exact_other", "only_root",
    "empty", "arity_one", "arity_two", "binary_object", "binary_other",
    "marker_interface", "marker_other", "cross_root", "cross_mid", "iterable_root",
    "iterable_mid", "parent_root", "inherited_mid", "generic_t", "generic_leaf", "binary_leaf",
))
REFERENCE_INVALID_CASES = frozenset((
    "wrong_arity", "class_instance", "wrong_reference", "protected_crosspackage",
    "inaccessible_receiver", "ambiguous", "missing_parent", "cycle",
    "duplicate_parent", "missing_argument_type", "inaccessible_argument_type",
))


def reference_target(declarations, candidate, binaries):
    """Explicit source-name identity or one inventoried binary declaration."""
    if candidate.startswith("class:"):
        path, member = candidate.removeprefix("class:").split("@")
        assert path == "dep/BinaryParent.class", "unexpected reference binary candidate"
        owner, name = member.split("#")
        assert (owner, name) == ("dep.BinaryParent", "pick")
        return f"{binaries / path}|Method|{owner}|{name}|0|0"
    kind = "Class" if candidate.startswith("type:") else "Method"
    marker, owner = candidate.removeprefix("type:").split("@")
    path, start, end, name = declarations[marker]
    return f"{path}|{kind}|{owner}|{name}|{start}|{end}"


def check_reference_candidates(query, status, candidates, declarations, actual, binaries):
    assert status in {"Resolved", "CallNotResolved", "OverloadNotResolved",
                      "SignatureNotCompared", "Unresolved", "ReceiverNotBound", "Cycle", "Ambiguous"}
    actual_status, count, *targets = actual
    assert actual_status == status, f"{query}: native status {actual_status}, expected {status}"
    assert int(count) == len(targets), f"{query}: incomplete reference native records"
    wanted = [] if candidates == "-" else [
        reference_target(declarations, item, binaries) for item in candidates.split(",")]
    assert len(set(wanted)) == len(wanted), f"{query}: duplicate reference golden candidate"
    assert len(set(targets)) == len(targets), f"{query}: duplicate reference native candidate"
    assert sorted(targets) == sorted(wanted), f"{query}: exact reference candidates differ: {targets}"
    if status in {"Resolved", "CallNotResolved"}:
        assert len(wanted) == 1, f"{query}: singleton status needs one candidate"
    elif status in {"Unresolved", "ReceiverNotBound", "Cycle"}:
        assert not wanted, f"{query}: unsupported receiver/cycle published target"
    else:
        assert len(wanted) >= 2, f"{query}: refusal/ambiguity needs multiple candidates"


def compare_reference(expected, queries, declarations, javac, native, binaries):
    """Strict independent declaration identity for the reference-call suite."""
    assert set(expected) == set(queries) == set(javac) == set(native), "missing/extra reference query evidence"
    for query, (status, selected, candidates) in expected.items():
        marker, owner = selected.split("@")
        path, start, end, name = declarations[marker]
        jpath, jkind, jowner, jname, jstart, jend = javac[query]
        assert (jpath, jkind, jowner, jname) == (path, "METHOD", owner, name), \
            f"{query}: reference compiler declaration differs"
        assert int(jstart) <= start < end <= int(jend), f"{query}: reference compiler range differs"
        contained = [key for key, (p, s, e, n) in declarations.items()
                     if p == jpath and n == jname and int(jstart) <= s < e <= int(jend)]
        assert contained == [marker], f"{query}: non-unique reference compiler declaration identity"
        if status == "Resolved":
            assert candidates == selected, f"{query}: selected source differs from compiler target"
        check_reference_candidates(query, status, candidates, declarations, native[query], binaries)


def compare_with_reference_updates(expected, queries, declarations, javac, native):
    """Preserve old goldens; separately audit one reviewed missing-Object refusal."""
    assert set(expected) == set(queries) == set(javac) == set(native), "missing/extra original query evidence"
    updates = rows(ORACLE / "reference-v1/navigation-updates.tsv", 5)
    assert updates == [["receiver_read_call", "Resolved", "CallNotResolved", "read_method", "lib.Value"]]
    changed = set()
    for query, before, after, marker, owner in updates:
        assert expected[query] == (before, marker), "original navigation golden changed"
        selected = f"{marker}@{owner}"
        compare_reference({query: (after, selected, selected)}, {query: queries[query]},
                          declarations, {query: javac[query]}, {query: native[query]}, Path("/unused"))
        changed.add(query)
    # The unchanged comparator still verifies every other original expectation.
    compare({q: row for q, row in expected.items() if q not in changed},
            {q: row for q, row in queries.items() if q not in changed}, declarations,
            {q: row for q, row in javac.items() if q not in changed},
            {q: row for q, row in native.items() if q not in changed})


def reference_fixture():
    root = ORACLE / "reference-v1"
    assert (root / "files.tsv").read_text().splitlines() == list(REFERENCE_FILES)
    declarations, queries, requests = fixture_markers(root / "source", REFERENCE_FILES)
    assert set(declarations) == REFERENCE_DECLARATIONS, "missing/extra reference declaration marker"
    expected = {}
    for query, status, selected, candidates in rows(root / "expected.tsv", 4):
        assert query not in expected and selected.split("@")[0] in declarations
        expected[query] = (status, selected, candidates)
    assert len(expected) == len(queries) == 24 and set(expected) == set(queries), \
        "reference query inventory differs"
    assert sum(row[0] == "Resolved" for row in expected.values()) == 15
    return declarations, queries, requests, expected


def reference_invalid_fixtures():
    root = ORACLE / "reference-v1/invalid"
    cases = rows(root.parent / "invalid.tsv", 5)
    assert len(cases) == 11 and {row[0] for row in cases} == REFERENCE_INVALID_CASES
    assert sorted(p.relative_to(root).as_posix() for p in root.rglob("*.java")) == sorted(
        f"{case}/{file}" for case, _, _, _, inventory in cases for file in inventory.split(",")), \
        "invalid reference file inventory differs"
    result = []
    for case, query, status, candidates, inventory in cases:
        ds, qs, requests = fixture_markers(root / case, inventory.split(","))
        assert set(qs) == {query} and query == f"reference_invalid_{case}"
        expected_markers = {"invalid_a", "invalid_b"} if case == "ambiguous" else {"invalid_only"}
        if case == "duplicate_parent":
            expected_markers |= {"invalid_parent_one", "invalid_parent_two"}
        assert set(ds) == expected_markers, f"{case}: missing/extra invalid declaration marker"
        assert status != "Resolved", f"{case}: compiler-invalid fixture cannot be a success"
        result.append((case, query, status, candidates, ds, requests))
    return result


def reference_suite(jdk, native_binary, env, work, helper, empty, platform):
    """Optional javac comparison, never fixture execution or production dependency."""
    root = ORACLE / "reference-v1"
    declarations, queries, requests, expected = reference_fixture()
    expected = current_receiver_guard_expectations("reference-v1", expected, queries, declarations)
    expected = current_null_expectations("reference-v1", expected, queries, declarations)
    expected = current_inherited_expectations("reference-v1", expected, queries, declarations)
    binaries = work / "reference-classes"
    binaries.mkdir()
    dependency = root / "dependency"
    assert sorted(p.relative_to(dependency).as_posix() for p in dependency.rglob("*.java")) == ["dep/BinaryParent.java"]
    run([jdk / "bin/javac", "--release", "21", "-proc:none", "-implicit:none",
         "-encoding", "UTF-8", "-classpath", empty, "-sourcepath", empty,
         "-d", binaries, dependency / "dep/BinaryParent.java"], env)
    assert sorted(p.relative_to(binaries).as_posix() for p in binaries.rglob("*.*")) == ["dep/BinaryParent.class"]
    object_path = binaries / "java/lang/Object.class"
    object_path.parent.mkdir(parents=True)
    object_path.write_bytes((platform / "java/lang/Object.class").read_bytes())
    # pinned_object has already verified this JMOD's exact checksum. The two
    # platform classes are input data, not executions of java/jmod/jar tools.
    iterable_path = binaries / "java/lang/Iterable.class"
    with zipfile.ZipFile(jdk / "jmods/java.base.jmod") as archive:
        entry = archive.getinfo("classes/java/lang/Iterable.class")
        assert 0 < entry.file_size < 1024 * 1024
        iterable_path.write_bytes(archive.read(entry))
    assert sorted(p.relative_to(binaries).as_posix() for p in binaries.rglob("*.*")) == [
        "dep/BinaryParent.class", "java/lang/Iterable.class", "java/lang/Object.class"]
    request = work / "reference-requests.tsv"
    request.write_text(requests)
    javac = records(run([jdk / "bin/java", "-cp", helper, "BindingOracle",
                         root / "source", request, binaries, empty], env).stdout)
    native = records(run([native_binary, root / "source", request, binaries], {"PATH": "/nonexistent"}).stdout)
    for query in expected:
        print(query + "\tjavac=" + "|".join(javac[query]) + "\tnative=" + "|".join(native[query]))
    compare_reference(expected, queries, declarations, javac, native, binaries)

    for case, query, status, candidates, ds, contents in reference_invalid_fixtures():
        request.write_text(contents)
        rejected = run([jdk / "bin/java", "-cp", helper, "BindingOracle",
                        root / "invalid" / case, request, empty, empty], env, False)
        assert rejected.returncode == 2 and "compiler.err." in rejected.stderr and not rejected.stdout, \
            f"invalid reference fixture accepted: {case}"
        actual = records(run([native_binary, root / "invalid" / case, request, binaries],
                             {"PATH": "/nonexistent"}).stdout)
        assert set(actual) == {query}, f"{case}: missing/extra invalid reference query"
        check_reference_candidates(query, status, candidates, ds, actual[query], binaries)
        print(query + "\tjavac=compiler-error\tnative=" + "|".join(actual[query]))
        false_success = {query: ["Resolved", *actual[query][1:]]}
        must_reject(lambda: check_reference_candidates(query, status, candidates, ds,
                    false_success[query], binaries), f"{case}: false Resolved")

    wrong = dict(expected)
    wrong["reference_specificity"] = ("Resolved", "pick_root@model.Target", "pick_root@model.Target")
    must_reject(lambda: compare_reference(wrong, queries, declarations, javac, native, binaries),
                "wrong reference overload")
    for label in ("path", "kind", "owner", "name", "range", "overlapping_range", "missing", "extra"):
        changed = {key: list(value) for key, value in javac.items()}
        row = changed["reference_specificity"]
        if label == "missing":
            del changed["reference_specificity"]
        elif label == "extra":
            changed["not_requested"] = row
        elif label == "range":
            row[-2:] = ["0", "1"]
        elif label == "overlapping_range":
            row[-2:] = ["0", "100000"]
        else:
            row[{"path": 0, "kind": 1, "owner": 2, "name": 3}[label]] = "wrong"
        must_reject(lambda: compare_reference(expected, queries, declarations, changed, native, binaries),
                    "reference compiler " + label)
    for label in ("path", "kind", "owner", "name", "range", "duplicate", "count", "missing", "extra", "binary_candidate"):
        changed = {key: list(value) for key, value in native.items()}
        row = changed["reference_specificity"]
        if label == "missing":
            del changed["reference_specificity"]
        elif label == "extra":
            changed["not_requested"] = row
        elif label == "duplicate":
            row[1] = "2"
            row.append(row[2])
        elif label == "count":
            row[1] = "2"
        elif label == "binary_candidate":
            changed["reference_binary_refusal"] = changed["reference_binary_refusal"][:3]
            changed["reference_binary_refusal"][1] = "1"
        else:
            parts = row[2].split("|")
            if label == "range":
                parts[-2:] = ["0", "1"]
            else:
                parts[{"path": 0, "kind": 1, "owner": 2, "name": 3}[label]] = "wrong"
            row[2] = "|".join(parts)
        must_reject(lambda: compare_reference(expected, queries, declarations, javac, changed, binaries),
                    "reference native " + label)
    request.write_text(requests)
    for removed, affected in ((object_path, [q for q, row in expected.items() if row[0] == "Resolved"]),
                              (iterable_path, ["reference_generic_ancestor"])):
        content = removed.read_bytes()
        removed.unlink()
        try:
            omitted = records(run([native_binary, root / "source", request, binaries],
                                  {"PATH": "/nonexistent"}).stdout)
            assert set(omitted) == set(expected)
            assert all(omitted[q][0] != "Resolved" for q in affected), \
                f"omitted {removed.name}: incomplete ancestor inventory still selected a call"
            for query in affected:
                print(query + "\tomitted=" + removed.name + "\tnative=" + "|".join(omitted[query]))
            must_reject(lambda: compare_reference(expected, queries, declarations, javac, omitted, binaries),
                        "omitted reference " + removed.name)
        finally:
            removed.write_bytes(content)
    print("PASS: 15 reference source bindings (13 new selections and two singleton controls); "
          "9 compiler-valid refusals; 11 separate compiler-invalid refusals; "
          "exact source/binary candidate inventories, compiler/native mutations, "
          "and omitted Object/Iterable controls verified")


def real_primitive_suite(jdk, native_binary, env, work, helper, empty, platform, sources_jar):
    """Optional eight source-selected queries over pinned unmodified upstream bytes."""
    pins = json.loads((ROOT / "scripts/acceptance-inputs.json").read_text())
    with sources_jar.open("rb") as source:
        assert hashlib.file_digest(source, "sha256").hexdigest() == pins["sources"]["sha256"], \
            "real oracle sources checksum differs"
    source_root = work / "real-primitive-source"
    files = []
    with zipfile.ZipFile(sources_jar) as archive:
        entries = [entry for entry in archive.infolist() if entry.filename.endswith(".java")]
        assert len(entries) == 249 and sum(entry.file_size for entry in entries) == 3676819, "real source inventory differs"
        seen = set()
        for entry in entries:
            relative = PurePosixPath(entry.filename)
            assert not relative.is_absolute() and ".." not in relative.parts and entry.filename not in seen, \
                "unsafe or duplicate real source path"
            seen.add(entry.filename)
            target = source_root.joinpath(*relative.parts)
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(archive.read(entry))
            files.append(entry.filename)
    expected = rows(ORACLE / "primitive-v1/real-expected.tsv", 7)
    assert len(expected) == 4 and {row[0] for row in expected} == {"q28", "q29", "q30", "q31"}
    returns = rows(ORACLE / "return-v1/real-expected.tsv", 7)
    assert len(returns) == 4 and {row[0] for row in returns} == {
        "return_real_max_double", "return_real_max_float", "return_real_min_double", "return_real_min_float"}
    expected += returns
    request = work / "real-primitive-requests.tsv"
    requests = [f"F\t{path}" for path in sorted(files)]
    requests.extend(f"Q\t{query}\t{path}\t{use}" for query, path, use, *_ in expected)
    request.write_text("\n".join(requests) + "\n")
    javac = records(run([jdk / "bin/java", "-cp", helper, "BindingOracle", source_root, request, empty], env).stdout)
    native = records(run([native_binary, source_root, request, platform], {"PATH": "/nonexistent"}).stdout)

    def compare_real(actual):
        assert {row[0] for row in expected} == set(javac) == set(actual), "missing/extra real oracle query"
        for query, path, use, name, primitive, start, end in expected:
            start, end = int(start), int(end)
            source = (source_root / path).read_bytes()
            owner = "org.apache.commons.lang3.math.IEEE754rUtils"
            jpath, jkind, jowner, jname, jstart, jend = javac[query]
            assert (jpath, jkind, jowner, jname) == (path, "METHOD", owner, name)
            assert int(jstart) <= start < end <= int(jend), f"{query}: compiler range differs"
            assert source[int(use):int(use) + len(name)] == source[start:end] == name.encode()
            if query.startswith("return_real_"):
                assert source[int(use) - 7:int(use)] == b"return "
                expression = f"{name}({name}(a, b), c);".encode()
                assert source[int(use):int(use) + len(expression)] == expression, \
                    f"{query}: real outer-call source differs"
            prefix = f"public static {primitive} {name}(final {primitive} a, final {primitive} b)".encode()
            assert source[int(jstart):int(jend)].startswith(prefix), f"{query}: compiler-selected signature differs"
            assert start == int(jstart) + len(f"public static {primitive} ".encode())
            target = f"{path}|Method|{owner}|{name}|{start}|{end}"
            assert actual[query] == ["Resolved", "1", target], f"{query}: real native target differs: {actual[query]}"
    compare_real(native)
    wrong = {key: list(value) for key, value in native.items()}
    wrong["q28"] = list(native["q29"])
    must_reject(lambda: compare_real(wrong), "wrong same-name real overload")
    wrong = {key: list(value) for key, value in native.items()}
    wrong["return_real_max_double"] = list(native["return_real_max_float"])
    must_reject(lambda: compare_real(wrong), "wrong same-name real return overload")
    omitted = records(run([native_binary, source_root, request], {"PATH": "/nonexistent"}).stdout)
    assert all(omitted[query][0] != "Resolved" for query, *_ in expected), "missing Object did not block real selections"
    must_reject(lambda: compare_real(omitted), "omitted real Object input")
    print("PASS: 8 independently compiler-selected real primitive source calls (four array-element and four scalar-result calls); "
          "249 unchanged source files analyzed; no upstream class generation/execution")


def inherited_source_fixture():
    """Frozen additive contract; never rewrites historical inherited refusals."""
    root = ORACLE / "inherited-source-v1"
    data = (root / "fixture-manifest.json").read_bytes()
    assert hashlib.sha256(data).hexdigest() == "609376e0c8fdda7ac437ba5e78fcd7dfe1fcc75d7fdf7e5d09b8ca04859f0e97", "inherited source fixture pin"
    pinned = json.loads(data)["files"]
    revision_data = (root / "prior-inherited-revision.json").read_bytes()
    assert hashlib.sha256(revision_data).hexdigest() == "63cf804783563664165363d5d1b1a71881583c7673095ab26e814defdd813c1d", "inherited revision pin"
    revision = json.loads(revision_data)
    assert revision["original_forecast_sha256"] == pinned["prior-inherited-updates.json"]
    for name, digest in revision["evidence"].items():
        assert hashlib.sha256((root / name).read_bytes()).hexdigest() == digest
    assert sorted(p.relative_to(root).as_posix() for p in root.rglob("*")
                  if p.is_file() and p.name not in ("README.md", "fixture-manifest.json")) == sorted(list(pinned) + ["prior-inherited-revision.json"] + list(revision["evidence"]))
    for name, digest in pinned.items():
        assert hashlib.sha256((root / name).read_bytes()).hexdigest() == digest, name
    files = (root / "files.tsv").read_text().splitlines()
    ds, qs, req = fixture_markers(root / "source", files)
    contracts = []
    for filename in ("expected.tsv", "baseline.tsv"):
        expected = {}
        for q, status, selected, candidates in rows(root / filename, 4):
            assert q not in expected
            expected[q] = status, selected, candidates
        assert set(expected) == set(qs) and len(qs) == 9
        contracts.append(expected)
    assert sum(v[0] == "Resolved" for v in contracts[0].values()) == 6
    return root, ds, qs, req, contracts[0], contracts[1]


def current_inherited_expectations(suite, expected, queries, declarations):
    """Exact v2 current overlay; old capture modes must not invoke it."""
    root, _, _, _, _, _ = inherited_source_fixture()
    plan = json.loads((root / "prior-inherited-updates.json").read_text())
    revision = json.loads((root / "prior-inherited-revision.json").read_text())
    assert suite in revision["active_suites"], "not an activated inherited overlay"
    for name, digest in plan["artifacts"].items():
        assert hashlib.sha256((ORACLE / name).read_bytes()).hexdigest() == digest, name
    update = plan["updates"][suite]
    assert {k: list(v) for k, v in queries.items()} == update["queries"], "complete query metadata"
    assert {k: list(v) for k, v in declarations.items()} == update["declarations"], "complete declaration metadata"
    assert {k: list(v) for k, v in expected.items()} == update["previous_current"], "complete previous stage"
    q = update["query"]
    assert list(expected[q]) == update["before"]
    assert update["after"] == ["Resolved", expected[q][1], expected[q][1]]
    result = dict(expected); result[q] = tuple(update["after"])
    return result


def inherited_source_suite(jdk, native_binary, env, work, helper, empty, platform, baseline=False):
    """Source superclass candidates; complete inherited sets, no override support."""
    assert type(baseline) is bool
    root, ds, qs, req, current, old = inherited_source_fixture()
    expected = old if baseline else current_source_override_expectations(current, qs, ds)
    request = work / "inherited-source-requests.tsv"; request.write_text(req)
    compiler = records(run([jdk / "bin/java", "-cp", helper, "BindingOracle", root / "source", request, empty], env).stdout)
    native = records(run([native_binary, root / "source", request, platform], {"PATH": "/nonexistent"}).stdout)
    for q in sorted(expected):
        print(q + "\tjavac=" + "|".join(compiler[q]) + "\tnative=" + "|".join(native[q]), flush=True)
    compare_reference(expected, qs, ds, compiler, native, platform)
    # Whole compiler declaration spans, in addition to exact native name spans.
    for q, (_, selected, _) in expected.items():
        path, start, _, _ = ds[selected.split("@")[0]]
        data = (root / "source" / path).read_bytes()
        begin = data.rfind(b"\n", 0, start) + 1; end = data.find(b"\n", start)
        begin += len(data[begin:end]) - len(data[begin:end].lstrip())
        # Single-line nested-class methods begin after the opening class brace.
        if b"class " in data[begin:start] or b"interface " in data[begin:start]:
            begin = data.rfind(b"{", begin, start) + 1
            while data[begin:begin+1].isspace(): begin += 1
            end = data.index(b"}", start) + 1
        assert tuple(map(int, compiler[q][4:])) == (begin, end), q + ": compiler whole range"
    if baseline:
        must_reject(lambda: compare_reference(current, qs, ds, compiler, native, platform), "inherited baseline must fail new selections")
    invalid = rows(root / "invalid.tsv", 5)
    assert {row[0] for row in invalid} == {"missing", "unknown"}
    for case, q, status, names, diagnostic in invalid:
        source = root / "invalid" / case
        ids, queries, req = fixture_markers(source, ("Use.java",))
        assert set(queries) == {q}; request.write_text(req)
        bad = run([jdk / "bin/java", "-cp", helper, "BindingOracle", source, request, empty], env, False)
        assert bad.returncode == 2 and diagnostic in bad.stderr and not bad.stdout
        actual = records(run([native_binary, source, request, platform], {"PATH": "/nonexistent"}).stdout)
        assert set(actual) == {q}
        check_reference_candidates(q, status, names, ids, actual[q], platform)
    print("PASS: inherited source candidates with explicit current override overlay; selected=" + str(sum(row[0] == "Resolved" for row in expected.values())) + "; two compiler-invalid refusals; baseline=" + str(baseline), flush=True)


def source_override_fixture():
    root = ORACLE / "source-override-v1"
    data = (root / "fixture-manifest.json").read_bytes()
    assert hashlib.sha256(data).hexdigest() == "397dd951e39b1e0fd77a41c053c68f2d7989fdab1a9c07595f056d994ab5e772", "source override fixture pin"
    pinned = json.loads(data)["files"]
    assert sorted(p.relative_to(root).as_posix() for p in root.rglob("*")
                  if p.is_file() and p.name not in ("README.md", "fixture-manifest.json")) == sorted(pinned)
    for name, digest in pinned.items(): assert hashlib.sha256((root / name).read_bytes()).hexdigest() == digest, name
    ds, qs, req = fixture_markers(root / "source", (root / "files.tsv").read_text().splitlines())
    contracts = []
    for filename in ("expected.tsv", "baseline.tsv"):
        expected = {}
        for q, status, selected, candidates in rows(root / filename, 4):
            assert q not in expected; expected[q] = status, selected, candidates
        assert set(expected) == set(qs) and len(qs) == 12
        contracts.append(expected)
    assert sum(v[0] == "Resolved" for v in contracts[0].values()) == 7
    return root, ds, qs, req, contracts[0], contracts[1]


def current_source_override_expectations(expected, queries, declarations):
    root, _, _, _, _, _ = source_override_fixture()
    update = json.loads((root / "prior-override-update.json").read_text())
    for name, digest in update["artifacts"].items():
        assert hashlib.sha256((ORACLE / name).read_bytes()).hexdigest() == digest, name
    assert {k: list(v) for k, v in queries.items()} == update["queries"]
    assert {k: list(v) for k, v in declarations.items()} == update["declarations"]
    assert {k: list(v) for k, v in expected.items()} == update["previous_current"]
    q = update["query"]; assert list(expected[q]) == update["before"]
    assert update["after"] == ["Resolved", expected[q][1], expected[q][1]]
    result = dict(expected); result[q] = tuple(update["after"])
    return result


def source_override_suite(jdk, native_binary, env, work, helper, empty, platform, baseline=False):
    """Static source override targets; never guesses a runtime receiver class."""
    assert type(baseline) is bool
    root, ds, qs, req, current, old = source_override_fixture()
    expected = old if baseline else current
    request = work / "source-override-requests.tsv"; request.write_text(req)
    compiler = records(run([jdk / "bin/java", "-cp", helper, "BindingOracle", root / "source", request, empty], env).stdout)
    native = records(run([native_binary, root / "source", request, platform], {"PATH": "/nonexistent"}).stdout)
    for q in sorted(expected): print(q + "\tjavac=" + "|".join(compiler[q]) + "\tnative=" + "|".join(native[q]), flush=True)
    compare_reference(expected, qs, ds, compiler, native, platform)
    for q, (_, selected, _) in expected.items():
        path, start, _, _ = ds[selected.split("@")[0]]; data = (root / "source" / path).read_bytes()
        begin = data.rfind(b"\n", 0, start) + 1; end = data.find(b"\n", start)
        begin += len(data[begin:end]) - len(data[begin:end].lstrip())
        if b"class " in data[begin:start] or b"interface " in data[begin:start]:
            begin = data.rfind(b"{", begin, start) + 1
            while data[begin:begin+1].isspace(): begin += 1
            end = data.index(b"}", start) + 1 if b"}" in data[start:end] else data.index(b";", start) + 1
        assert tuple(map(int, compiler[q][4:])) == (begin, end), q + ": whole compiler range"
    if baseline:
        must_reject(lambda: compare_reference(current, qs, ds, compiler, native, platform), "override baseline must fail new selections")
    invalid = rows(root / "invalid.tsv", 4)
    assert {row[0] for row in invalid} == {"final", "return", "static", "visibility", "unknown", "missing"}
    for case, q, status, candidates in invalid:
        source = root / "invalid" / case; ids, queries, req = fixture_markers(source, ("Use.java",))
        assert set(queries) == {q}; request.write_text(req)
        rejected = run([jdk / "bin/java", "-cp", helper, "BindingOracle", source, request, empty], env, False)
        assert rejected.returncode == 2 and "compiler.err." in rejected.stderr and not rejected.stdout
        result = records(run([native_binary, source, request, platform], {"PATH": "/nonexistent"}).stdout)
        assert set(result) == {q}; check_reference_candidates(q, status, candidates, ids, result[q], platform)
    print("PASS: source override; seven static targets, five valid refusals, six invalid refusals; baseline=" + str(baseline), flush=True)


def source_invocation_fixture():
    root = ORACLE / "source-invocation-v1"
    data = (root / "fixture-manifest.json").read_bytes()
    assert hashlib.sha256(data).hexdigest() == "91e25ac0101162feb81639784fa38fddf5b975b3be23a36555978fa5a550b957", "source invocation fixture pin"
    pinned = json.loads(data)["files"]
    assert sorted(p.relative_to(root).as_posix() for p in root.rglob("*")
                  if p.is_file() and p.name not in ("README.md", "fixture-manifest.json")) == sorted(pinned)
    for name, digest in pinned.items(): assert hashlib.sha256((root / name).read_bytes()).hexdigest() == digest, name
    ds, qs, req = fixture_markers(root / "source", (root / "files.tsv").read_text().splitlines())
    contracts = []
    for filename in ("expected.tsv", "baseline.tsv"):
        expected = {}
        for q, status, selected, candidates in rows(root / filename, 4):
            assert q not in expected; expected[q] = status, selected, candidates
        assert set(expected) == set(qs) and len(qs) == 14
        contracts.append(expected)
    assert sum(v[0] == "Resolved" for v in contracts[0].values()) == 9
    return root, ds, qs, req, contracts[0], contracts[1]


def current_source_invocation_expectations(suite, expected, queries, declarations):
    root, _, _, _, _, _ = source_invocation_fixture()
    plan = json.loads((root / "prior-invocation-updates.json").read_text())
    assert suite in plan["suites"]
    for name, digest in plan["artifacts"].items():
        assert hashlib.sha256((ORACLE / name).read_bytes()).hexdigest() == digest, name
    update = plan["suites"][suite]
    assert {k: list(v) for k, v in queries.items()} == update["queries"]
    assert {k: list(v) for k, v in declarations.items()} == update["declarations"]
    assert {k: list(v) for k, v in expected.items()} == update["previous_current"]
    result = dict(expected)
    for q, after in update["changes"].items():
        assert after == ["Resolved", expected[q][1], expected[q][1]]
        result[q] = tuple(after)
    return result


def source_invocation_suite(jdk, native_binary, env, work, helper, empty, platform, baseline=False):
    """Bounded source call types compose as actuals/receivers, never runtime types."""
    assert type(baseline) is bool
    root, ds, qs, req, current, old = source_invocation_fixture()
    expected = old if baseline else current
    request = work / "source-invocation-requests.tsv"; request.write_text(req)
    compiler = records(run([jdk / "bin/java", "-cp", helper, "BindingOracle", root / "source", request, empty], env).stdout)
    native = records(run([native_binary, root / "source", request, platform], {"PATH": "/nonexistent"}).stdout)
    for q in sorted(expected): print(q + "\tjavac=" + "|".join(compiler[q]) + "\tnative=" + "|".join(native[q]), flush=True)
    compare_reference(expected, qs, ds, compiler, native, platform)
    for q, (_, selected, _) in expected.items():
        path, start, _, _ = ds[selected.split("@")[0]]; data = (root / "source" / path).read_bytes()
        begin = data.rfind(b"\n", 0, start) + 1; end = data.find(b"\n", start)
        begin += len(data[begin:end]) - len(data[begin:end].lstrip())
        assert tuple(map(int, compiler[q][4:])) == (begin, end), q + ": whole compiler range"
    if baseline:
        must_reject(lambda: compare_reference(current, qs, ds, compiler, native, platform), "invocation baseline must fail new selections")
    # Three distinct large source owners avoid attributing cached graph re-use
    # to repeated cost. Exact exhaustion is unit-tested in Rust, not inferred
    # solely from this conservative end-to-end refusal.
    budget = root / "budget"; ids, queries, req = fixture_markers(budget, ("Work.java",))
    assert set(queries) == {"work_refusal"}; request.write_text(req)
    budget_expected = {q: (st, sel, cs) for q, st, sel, cs in rows(budget / "expected.tsv", 4)}
    compiled = records(run([jdk / "bin/java", "-cp", helper, "BindingOracle", budget, request, empty], env).stdout)
    actual = records(run([native_binary, budget, request, platform], {"PATH": "/nonexistent"}).stdout)
    compare_reference(budget_expected, queries, ids, compiled, actual, platform)
    invalid = rows(root / "invalid.tsv", 4)
    assert {row[0] for row in invalid} == {"unknown", "void"}
    for case, q, status, candidates in invalid:
        source = root / "invalid" / case; ids, queries, req = fixture_markers(source, ("Use.java",))
        assert set(queries) == {q}; request.write_text(req)
        rejected = run([jdk / "bin/java", "-cp", helper, "BindingOracle", source, request, empty], env, False)
        assert rejected.returncode == 2 and "compiler.err." in rejected.stderr and not rejected.stdout
        result = records(run([native_binary, source, request, platform], {"PATH": "/nonexistent"}).stdout)
        assert set(result) == {q}; check_reference_candidates(q, status, candidates, ids, result[q], platform)
    print("PASS: source invocation; eight new/nine total static targets, five valid expression refusals, one large-work refusal, two invalid refusals; baseline=" + str(baseline), flush=True)


def binary_instance_class_members(data):
    """Independent small classfile reader for handwritten javac fixture evidence."""
    import struct
    assert data[:4] == b"\xca\xfe\xba\xbe"
    at = 8
    def u2():
        nonlocal at
        value = struct.unpack_from(">H", data, at)[0]; at += 2; return value
    def u4():
        nonlocal at
        value = struct.unpack_from(">I", data, at)[0]; at += 4; return value
    count = u2(); pool = [None] * count; i = 1
    while i < count:
        tag = data[at]; at += 1
        if tag == 1:
            size = u2(); pool[i] = data[at:at+size].decode('utf-8'); at += size
        elif tag in (7, 8, 16, 19, 20): pool[i] = u2()
        elif tag in (3, 4, 9, 10, 11, 12, 17, 18): at += 4
        elif tag in (5, 6): at += 8; i += 1
        elif tag == 15: at += 3
        else: raise AssertionError('unexpected classfile pool tag ' + str(tag))
        i += 1
    class_flags = u2(); this = u2(); parent = u2()
    binary_name = pool[pool[this]]; super_name = pool[pool[parent]] if parent else None
    interfaces = [pool[pool[u2()]] for _ in range(u2())]
    def attributes():
        nonlocal at
        result = {}
        for _ in range(u2()):
            name = pool[u2()]; size = u4(); value = data[at:at+size]; at += size
            assert name not in result; result[name] = value
        return result
    members = {}
    for kind in ('fields', 'methods'):
        result = []
        for _ in range(u2()):
            flags_at = at; flags = u2(); name = pool[u2()]; descriptor = pool[u2()]; attrs = attributes()
            signature = pool[struct.unpack('>H', attrs['Signature'])[0]] if 'Signature' in attrs else None
            result.append({'name': name, 'descriptor': descriptor, 'raw_flags': flags, 'flags_offset': flags_at,
                           'signature': signature})
        members[kind] = result
    attrs = attributes(); assert at == len(data), 'complete classfile bytes'
    return {'binary_name': binary_name, 'super_name': super_name, 'interfaces': interfaces,
            'raw_flags': class_flags, 'generic': 'Signature' in attrs, **members}


def binary_instance_javap_calls(output):
    """Read selected bytecode call descriptors, independently of engine output."""
    current = None; result = {}
    for line in output.splitlines():
        method = re.match(r"  public [^;]+? ([A-Za-z_$][A-Za-z0-9_$]*)\([^;]*\);$", line)
        if method:
            current = method.group(1); assert current not in result; result[current] = []
        elif current:
            call = re.search(r"\b(invokevirtual|invokestatic|invokeinterface|invokespecial)\s+#\d+.*// (?:InterfaceMethod|Method) ([^:]+):(\S+)", line)
            if call:
                owner_name = call.group(2)
                assert '.' in owner_name, 'explicit binary declaring owner required'
                owner, name = owner_name.rsplit('.', 1)
                result[current].append([call.group(1), owner, name, call.group(3)])
    return result


def binary_instance_default_projection(target):
    result = dict(target)
    if result.get('origin') == 'binary' and result.get('kind') == 'Method':
        assert 'descriptor' in result and isinstance(result['descriptor'], str)
        assert result['identity_precision'] == 'origin_binary_name_descriptor_linking_context'
        assert isinstance(result['linking_context'], dict)
        del result['descriptor']
        result['linking_context'] = None
        result['identity_precision'] = 'origin_kind_owner_name_only'
    return result


def binary_instance_compare_expected(actual, wanted):
    assert json.dumps(actual, sort_keys=True) == json.dumps(wanted, sort_keys=True), 'complete binary target/query contract'


def binary_instance_compare_modes(default, detailed):
    """Default schema-v1 target projection stays exact; do not deduplicate."""
    assert default['schema_version'] == detailed['schema_version'] == 1
    plain, rich = default['result'], detailed['result']
    assert 'target_details_version' not in plain
    assert type(rich['target_details_version']) is int and rich['target_details_version'] == 1
    assert set(rich) == set(plain) | {'target_details_version'}
    projected = {k: v for k, v in rich.items() if k != 'target_details_version'}
    projected['queries'] = [dict(row, targets=[binary_instance_default_projection(t) for t in row['targets']]) for row in rich['queries']]
    assert json.dumps(projected, sort_keys=True) == json.dumps(plain, sort_keys=True), 'exact v1 projection including multiplicity'


def binary_instance_prepare(jdk, work):
    """Compile only handwritten fixture bytes; javap reads, never runs, probes."""
    import shutil
    root = ORACLE / 'binary-instance-v1'; env = {'PATH': '/nonexistent'}
    classes, app, empty = work / 'dependency', work / 'probe-classes', work / 'empty'
    for path in (classes, app, empty): path.mkdir()
    platform = pinned_object(jdk, work)
    common = [jdk / 'bin/javac', '--release', '21', '-g:none', '-proc:none', '-implicit:none', '-encoding', 'UTF-8', '-sourcepath', empty]
    dependency = sorted((root / 'dependency').rglob('*.java'))
    run(common + ['-classpath', empty, '-d', classes, *dependency], env)
    class_data = {p.relative_to(classes).as_posix()[:-6]: binary_instance_class_members(p.read_bytes()) for p in classes.rglob('*.class')}
    expected_flags = json.loads((root / 'flags-expected.json').read_text())
    for owner, expected in expected_flags.items():
        actual = {m['name'] + ':' + m['descriptor']: m['raw_flags'] for m in class_data[owner]['methods'] if m['name'] != '<init>'}
        assert actual == expected, owner + ': exact handwritten compiler flags'
    source_files = sorted((root / 'source').rglob('*.java'))
    run(common + ['-classpath', classes, '-d', app, *source_files], env)
    compiler_expected = json.loads((root / 'compiler-expected.json').read_text())
    compiler = {}
    for owner, key in [('app.Probe', 'probe_calls'), ('app.Navigation', 'navigation_calls')]:
        result = run([jdk / 'bin/javap', '-classpath', str(app) + ':' + str(classes), '-c', '-s', '-p', owner], env).stdout
        compiler[key] = binary_instance_javap_calls(result)
        assert compiler[key] == compiler_expected[key], (owner, compiler[key], compiler_expected[key])
    invalid = sorted((root / 'invalid').iterdir())
    assert [p.name for p in invalid] == ['private', 'protected', 'wrong_arity', 'wrong_type']
    for directory in invalid:
        rejected = run(common + ['-XDrawDiagnostics', '-classpath', classes, '-d', app, directory / 'app/Probe.java'], env, False)
        assert rejected.returncode != 0 and 'compiler.err.' in rejected.stderr, directory.name
    malformed = work / 'malformed'; shutil.copytree(classes, malformed)
    method = next(m for m in class_data['dep/Methods']['methods'] if m['name'] == 'zero')
    data = bytearray((malformed / 'dep/Methods.class').read_bytes())
    data[method['flags_offset']:method['flags_offset']+2] = (0x0411).to_bytes(2, 'big')
    (malformed / 'dep/Methods.class').write_bytes(data)
    assert next(m for m in binary_instance_class_members(data)['methods'] if m['name'] == 'zero')['raw_flags'] == 0x0411
    batches = []; source_roots = {}
    for label, source, cp, only in [('valid', root / 'source', classes, None),
                                  *[(p.name, p, classes, None) for p in invalid],
                                  ('malformed_flags', root / 'source', malformed, {'zero'})]:
        files = sorted(p.relative_to(source).as_posix() for p in source.rglob('*.java'))
        _, queries, _ = fixture_markers(source, files)
        if only is not None: queries = {q: v for q, v in queries.items() if q in only}
        scopes = [{'project': name, 'source_set': 'main'} for name in ('first', 'second')]
        copies = {}
        for scope in scopes:
            copied = work / (label + '-source') / scope['project']; shutil.copytree(source, copied)
            copies[scope['project']] = copied
            source_roots[str(copied)] = '@SOURCE@/' + label + '/' + scope['project']
        manifest = work / (label + '-manifest.json')
        manifest.write_text(json.dumps({'schema_version': 1, 'compilations': [
            {'id': scope, 'source_roots': [str(copies[scope['project']])], 'classpath': [{'kind': 'directory', 'path': str(cp)}, {'kind': 'directory', 'path': str(platform)}]} for scope in scopes]}))
        requests = work / (label + '-requests.json')
        identities = []; rows = []
        for scope in scopes:
            for q, (path, at, _, _) in queries.items():
                identities.append([scope['project'], q])
                rows.append({'compilation': scope, 'file': str(copies[scope['project']] / path), 'position': {'encoding': 'utf8', 'offset': at}})
        requests.write_text(json.dumps({'schema_version': 1, 'queries': rows}))
        batches.append({'label': label, 'manifest': manifest, 'requests': requests, 'identities': identities})
    return {'root': root, 'classes': classes, 'malformed': malformed, 'platform': platform,
            'class_data': class_data, 'compiler': compiler, 'batches': batches, 'source_roots': source_roots,
            'class_hashes': {p.relative_to(classes).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest() for p in classes.rglob('*.class')}}


def binary_instance_normalize(value, prepared):
    if isinstance(value, list): return [binary_instance_normalize(v, prepared) for v in value]
    if isinstance(value, dict): return {k: binary_instance_normalize(v, prepared) for k, v in value.items()}
    if isinstance(value, str):
        for source, marker in prepared['source_roots'].items(): value = value.replace(source, marker)
        for key, marker in [('root', '@FIXTURE@'), ('classes', '@CLASSES@'), ('malformed', '@MALFORMED@'), ('platform', '@PLATFORM@')]:
            value = value.replace(str(prepared[key]), marker)
    return value


def binary_instance_query(native_cli, batch, details=False, success=True):
    command = [native_cli, 'declaration', '--manifest', batch['manifest'], '--requests', batch['requests']]
    if details: command.append('--target-details')
    completed = run(command, {'PATH': '/nonexistent'}, success)
    if not success: return completed
    response = json.loads(completed.stdout)
    assert type(response['schema_version']) is int and response['schema_version'] == 1 and response['command'] == 'declaration'
    assert response['capabilities']['runtime'] == 'native_rust_no_jvm'
    assert response['snapshot']['manifest'] == str(batch['manifest'])
    return response


def binary_instance_target(owner, name, descriptor, scope, malformed=False):
    # Handwritten top-level names retain literal '$'; nested fixture metadata
    # uses its independently frozen compiler target rather than guessing scope.
    dotted = owner.replace('/', '.')
    if owner == 'dep/Outer$Inner': dotted = 'dep.Outer.Inner'
    return {'kind': 'Method', 'name': name, 'owner': dotted,
            'origin': 'binary', 'source_available': False,
            'path': ('@MALFORMED@/' if malformed else '@CLASSES@/') + owner + '.class',
            'binary_name': owner,
            'loaded_contexts': [{'project': value, 'source_set': 'main'} for value in ('first', 'second')],
            'linking_context': {'project': scope, 'source_set': 'main'},
            'identity_precision': 'origin_binary_name_descriptor_linking_context', 'descriptor': descriptor}


def binary_instance_predict(baseline, compiler_expected):
    """Freeze predictions before candidate execution, using independent descriptors."""
    import copy
    result = {}
    positives = compiler_expected['positive_targets']
    for label, batch in baseline['batches'].items():
        wanted = copy.deepcopy(batch['default']); wanted['target_details_version'] = 1
        for (scope, query), row in zip(batch['identities'], wanted['queries']):
            if label == 'valid' and query in positives:
                owner, name, descriptor = positives[query]
                row['status'] = 'Resolved'; row['targets'] = [binary_instance_target(owner, name, descriptor, scope)]
            elif query not in ('binary_class', 'binary_field'):
                if row['status'] == 'Resolved': row['status'] = 'CallNotResolved'
                targets = []
                by_owner_name = {}
                for target in row['targets']:
                    assert target['origin'] == 'binary' and target['kind'] == 'Method'
                    key = target['binary_name'], target['name']
                    by_owner_name.setdefault(key, []).append(target)
                for (owner, name), old_targets in by_owner_name.items():
                    members = [m for m in baseline['class_data'][owner]['methods'] if m['name'] == name]
                    assert len(members) == len(old_targets), 'complete legacy candidate multiplicity must match independent descriptors'
                    for method in members:
                        targets.append(binary_instance_target(owner, name, method['descriptor'], scope, label == 'malformed_flags'))
                row['targets'] = targets
        result[label] = wanted
    return result


def binary_instance_fixture():
    root = ORACLE / 'binary-instance-v1'
    data = (root / 'fixture-manifest.json').read_bytes()
    assert hashlib.sha256(data).hexdigest() == '9ae7787ee4ea63c596c07cab07b4cc62511b2588eaf08b996756443ca39d5202', 'frozen binary instance inputs'
    pinned = json.loads(data)['files']
    assert sorted(p.relative_to(root).as_posix() for p in root.rglob('*')
                  if p.is_file() and p.name not in ('fixture-manifest.json', 'README.md')) == sorted(pinned)
    for name, digest in pinned.items(): assert hashlib.sha256((root / name).read_bytes()).hexdigest() == digest, name
    preservation = json.loads((root / 'prior-preservation.json').read_text())
    for name, digest in preservation['artifacts'].items():
        assert hashlib.sha256((ORACLE / name).read_bytes()).hexdigest() == digest, 'unchanged prior oracle: ' + name
    contract = json.loads((root / 'contract.json').read_text())
    assert contract['schema_version'] == 2
    for key, name in [('scope_revision_sha256', 'scope-revision.json'), ('topology_revision_sha256', 'topology-revision.json')]:
        assert contract[key] == hashlib.sha256((root / name).read_bytes()).hexdigest()
    scope = json.loads((root / 'scope-revision.json').read_text())
    assert scope['supersedes_sha256'] == hashlib.sha256((root / 'history/v2-scope-revision.json').read_bytes()).hexdigest()
    for name, digest in scope['history'].items(): assert hashlib.sha256((root / name).read_bytes()).hexdigest() == digest
    topology = json.loads((root / 'topology-revision.json').read_text())
    assert topology['original_prediction_sha256'] == hashlib.sha256((root / 'history/pre-candidate-topology-v1.json').read_bytes()).hexdigest()
    assert topology['mechanism_sha256'] == hashlib.sha256((root / 'topology-mechanism.json').read_bytes()).hexdigest()
    return root, contract


def binary_instance_suite(jdk, native_cli, env, work, baseline=False):
    """Two real linking contexts, source javap oracle, exact v1/opt-in contracts."""
    assert type(baseline) is bool
    root, contract = binary_instance_fixture()
    local = work / 'binary-instance'; local.mkdir()
    prepared = binary_instance_prepare(jdk, local)
    assert prepared['class_hashes'] == contract['baseline']['class_hashes'], 'fixed compiler class bytes'
    assert prepared['class_data'] == contract['baseline']['class_data'], 'independent complete classfile metadata'
    assert prepared['compiler'] == contract['baseline']['compiler'], 'independent compiler calls'
    assert set(contract['current']) == {b['label'] for b in prepared['batches']}
    total = 0
    for batch in prepared['batches']:
        label = batch['label']; historical = contract['baseline']['batches'][label]
        assert batch['identities'] == historical['identities']
        default = binary_instance_query(native_cli, batch)
        normalized = binary_instance_normalize(default['result'], prepared)
        if baseline:
            binary_instance_compare_expected(normalized, historical['default'])
        else:
            detailed = binary_instance_query(native_cli, batch, details=True)
            binary_instance_compare_modes(default, detailed)
            rich = binary_instance_normalize(detailed['result'], prepared)
            binary_instance_compare_expected(rich, contract['current'][label])
        total += len(normalized['queries'])
        print('binary-instance ' + label + ' contexts=2 queries=' + str(len(normalized['queries'])) + ' baseline=' + str(baseline), flush=True)
    if baseline:
        rejected = binary_instance_query(native_cli, prepared['batches'][0], details=True, success=False)
        assert rejected.returncode != 0 and 'target-details' in rejected.stdout + rejected.stderr
        current = dict(contract['current']['valid']); del current['target_details_version']
        current['queries'] = [dict(row, targets=[binary_instance_default_projection(t) for t in row['targets']]) for row in current['queries']]
        must_reject(lambda: binary_instance_compare_expected(contract['baseline']['batches']['valid']['default'], current),
                    'legacy status false positives/overloads must fail strict current contract')
    print('PASS: binary instance strict; 2 linking contexts; ' + str(total) + ' complete queries; descriptor/raw-flags javap evidence; v1/opt-in exact projections; baseline=' + str(baseline), flush=True)


def main():
    if sys.flags.optimize:
        raise RuntimeError("oracle validation requires Python without -O/PYTHONOPTIMIZE")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--jdk", required=True, type=Path, help="checksum-verified Temurin 21.0.13+9 root")
    parser.add_argument("--native", type=Path, default=ROOT / "target/debug/examples/oracle_bindings")
    parser.add_argument("--native-cli", required=True, type=Path, help="explicit matching-profile java-analyzer CLI")
    parser.add_argument("--jdk-archive", required=True, type=Path, help="pinned JDK archive for installed-byte verification")
    parser.add_argument("--sources-jar", type=Path, help="optional checksum-pinned Commons Lang sources for eight real overload queries")
    args = parser.parse_args()
    jdk = args.jdk.resolve()
    import importlib.util
    verifier_spec = importlib.util.spec_from_file_location('semantic_jdk_verifier', ROOT / 'scripts/check-strict-reference-oracle.py')
    verifier = importlib.util.module_from_spec(verifier_spec); verifier_spec.loader.exec_module(verifier)
    print('verified-jdk=' + verifier.verify_jdk(jdk, args.jdk_archive.resolve()), flush=True)
    native_cli = args.native_cli.resolve(strict=True)
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
        compare_with_reference_updates(expected, queries, declarations, javac, native)
        platform = pinned_object(jdk, work)
        closed_native = records(run([args.native.resolve(), fixture, request, platform],
                                    {"PATH": "/nonexistent"}).stdout)
        compare(expected, queries, declarations, javac, closed_native)
        print("PASS: original 63-query goldens unchanged with explicit Object input "
              "(59 supported bindings, four refusals); historical no-platform input "
              "separately requires the reviewed exact singleton refusal")
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
                compare_with_reference_updates(perturbed, queries, declarations, javac, native)
            except AssertionError:
                pass
            else:
                raise AssertionError(f"oracle accepted deliberately wrong declaration: {query}")
        corrupted = {key: list(value) for key, value in javac.items()}
        corrupted["parameter"][4] = "0"
        corrupted["parameter"][5] = "1"
        try:
            compare_with_reference_updates(expected, queries, declarations, corrupted, native)
        except AssertionError:
            pass
        else:
            raise AssertionError("oracle accepted corrupted compiler declaration range")
        local_type_suite(jdk, args.native.resolve(), env, work, classes, empty)
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
        print(f"PASS: {sum(s == 'Resolved' for s, _ in expected.values()) - 1} supported declaration bindings; "
              f"{sum(s != 'Resolved' for s, _ in expected.values()) + 1} explicit unsupported boundaries; "
              "including one reviewed missing-Object reference-call refusal; "
              "wrong-target detection, 3 invalid/incomplete compiler failures, "
              "and 5 no-callable refusals verified")
        for query in expected:
            print(query + "\tjavac=" + "|".join(javac[query]) + "\tnative=" + "|".join(native[query]))
        primitive_suite(jdk, args.native.resolve(), env, work, classes, empty, platform)
        return_suite(jdk, args.native.resolve(), env, work, classes, empty, platform)
        reference_suite(jdk, args.native.resolve(), env, work, classes, empty, platform)
        inherited_source_suite(jdk, args.native.resolve(), env, work, classes, empty, platform)
        source_override_suite(jdk, args.native.resolve(), env, work, classes, empty, platform)
        source_invocation_suite(jdk, args.native.resolve(), env, work, classes, empty, platform)
        binary_instance_suite(jdk, native_cli, env, work)
        if args.sources_jar is not None:
            real_primitive_suite(jdk, args.native.resolve(), env, work, classes, empty, platform,
                                 args.sources_jar.resolve(strict=True))


if __name__ == "__main__":
    main()
