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
    assert len(newly_supported) == 10 and set(omitted) == set(expected)
    assert all(omitted[q][0] != "Resolved" for q in newly_supported), "missing Object did not block new selections"
    must_reject(lambda: compare_primitive(expected, queries, declarations, javac, omitted), "omitted Object input")
    print("PASS: 12 strict primitive source bindings; 20 conservative refusals; "
          "exact candidates, target mutations, omitted Object and query-completeness controls verified")


def real_primitive_suite(jdk, native_binary, env, work, helper, empty, platform, sources_jar):
    """Optional four source-selected queries over pinned unmodified upstream bytes."""
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
            prefix = f"public static {primitive} {name}(final {primitive} a, final {primitive} b)".encode()
            assert source[int(jstart):int(jend)].startswith(prefix), f"{query}: compiler-selected signature differs"
            assert start == int(jstart) + len(f"public static {primitive} ".encode())
            target = f"{path}|Method|{owner}|{name}|{start}|{end}"
            assert actual[query] == ["Resolved", "1", target], f"{query}: real native target differs: {actual[query]}"
    compare_real(native)
    wrong = {key: list(value) for key, value in native.items()}
    wrong["q28"] = list(native["q29"])
    must_reject(lambda: compare_real(wrong), "wrong same-name real overload")
    omitted = records(run([native_binary, source_root, request], {"PATH": "/nonexistent"}).stdout)
    assert all(omitted[query][0] != "Resolved" for query, *_ in expected), "missing Object did not block real selections"
    must_reject(lambda: compare_real(omitted), "omitted real Object input")
    print("PASS: 4 independently compiler-selected real primitive source methods; "
          "249 unchanged source files analyzed; no upstream class generation/execution")


def main():
    if sys.flags.optimize:
        raise RuntimeError("oracle validation requires Python without -O/PYTHONOPTIMIZE")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--jdk", required=True, type=Path, help="checksum-verified Temurin 21.0.13+9 root")
    parser.add_argument("--native", type=Path, default=ROOT / "target/debug/examples/oracle_bindings")
    parser.add_argument("--sources-jar", type=Path, help="optional checksum-pinned Commons Lang sources for four real overload queries")
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
        platform = pinned_object(jdk, work)
        primitive_suite(jdk, args.native.resolve(), env, work, classes, empty, platform)
        if args.sources_jar is not None:
            real_primitive_suite(jdk, args.native.resolve(), env, work, classes, empty, platform,
                                 args.sources_jar.resolve(strict=True))


if __name__ == "__main__":
    main()
