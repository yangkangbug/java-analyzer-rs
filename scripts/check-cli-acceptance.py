#!/usr/bin/env python3
"""Offline acceptance of the shipped native CLI, not an alternate Rust harness.

Requires Python 3.11+ and POSIX waitid(WNOWAIT), tested on Linux. Verifies fixed
external inputs, never fetches,
builds, or invokes Java, and bounds each product subprocess's time and output.
"""

import argparse
import copy
from collections import Counter
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import selectors
import signal
import subprocess
import tempfile
import time
import zipfile
import zlib


ROOT = Path(__file__).resolve().parent.parent
CORPUS = ROOT / "examples" / "real_source"
COMPILATION = {"project": "commons-lang", "source_set": "main"}
SOURCE_COUNT = 249
SOURCE_BYTES = 3_676_819
PLATFORM_CLASSES = 7_615


class AcceptanceError(ValueError):
    pass


def require(condition, message):
    # Do not use assert: Python -O must not disable acceptance checks.
    if not condition:
        raise AcceptanceError(message)


def same(actual, expected, label):
    require(actual == expected, f"{label}: expected {expected!r}, got {actual!r}")


def input_helpers():
    spec = importlib.util.spec_from_file_location(
        "acceptance_input_security", ROOT / "scripts" / "fetch-acceptance-inputs.py"
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def extract_sources(path, specs, destination, notices):
    """Reuse the bounded archive validator, then extract Java bytes exclusively.

    The helper validates every entry, duplicate/name/type/expansion budgets and
    CRCs, and retains LICENSE/NOTICE. No archive method creates filesystem paths.
    The destination is a new private temporary directory with no symlinks.
    """
    security = input_helpers()
    security.verify(path, specs)
    security.check_sources(path, specs, notices)
    sources = {}
    with zipfile.ZipFile(path) as archive:
        for entry in archive.infolist():
            if entry.is_dir() or not entry.filename.endswith(".java"):
                continue
            name = security.safe_name(entry.filename)
            with archive.open(entry) as stream:
                data = stream.read(specs["max_file"] + 1)
            same(len(data), entry.file_size, f"source size: {name}")
            require(len(data) <= specs["max_file"], f"source size budget: {name}")
            data.decode("utf-8")
            target = destination.joinpath(*name.split("/"))
            target.parent.mkdir(parents=True, exist_ok=True)
            with target.open("xb") as stream:
                stream.write(data)
            sources[name] = data
    same(len(sources), SOURCE_COUNT, "source inventory")
    same(sum(map(len, sources.values())), SOURCE_BYTES, "source byte inventory")
    require((notices / "LICENSE.txt").stat().st_size > 0, "source license is empty")
    require((notices / "NOTICE.txt").stat().st_size > 0, "source notice is empty")
    return sources


def run_bounded(command, cwd, timeout, max_output):
    """Bound wall time and combined stdout/stderr, including inherited pipes.

    A new process group allows timeout/output failures to kill descendants too.
    These are harness containment checks, not a production CLI timeout promise.
    """
    require(os.name == "posix" and hasattr(os, "waitid") and hasattr(os, "WNOWAIT"),
            "bounded CLI acceptance requires POSIX waitid(WNOWAIT) for safe process-group cleanup")
    deadline = time.monotonic() + timeout
    output = {"stdout": bytearray(), "stderr": bytearray()}
    process = subprocess.Popen(
        [str(part) for part in command], cwd=cwd,
        env={"PATH": "/nonexistent"}, stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True,
    )
    try:
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ, "stdout")
            selector.register(process.stderr, selectors.EVENT_READ, "stderr")
            total = 0
            while selector.get_map():
                remaining = deadline - time.monotonic()
                require(remaining > 0, f"CLI timeout after {timeout:g}s: {command[1]}")
                for key, _ in selector.select(remaining):
                    block = os.read(key.fileobj.fileno(), 65_536)
                    if not block:
                        selector.unregister(key.fileobj)
                        continue
                    total += len(block)
                    require(total <= max_output, f"CLI output exceeded {max_output} bytes")
                    output[key.data].extend(block)
            remaining = deadline - time.monotonic()
            require(remaining > 0, f"CLI timeout after {timeout:g}s: {command[1]}")
            # Observe exit without releasing the leader's PID/PGID. Reaping
            # before killpg would allow reuse and could signal an unrelated group.
            while os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT) is None:
                remaining = deadline - time.monotonic()
                require(remaining > 0, f"CLI timeout after {timeout:g}s: {command[1]}")
                time.sleep(min(0.005, remaining))
    finally:
        # Also clean up a descendant retaining a pipe after its parent exits.
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        code = process.wait(timeout=5)
        process.stdout.close()
        process.stderr.close()
    return code, bytes(output["stdout"]), bytes(output["stderr"])


def run_cli(native, cwd, manifest, command, timeout, max_output, requests=None, *, target_details=False):
    arguments = [native, command, "--manifest", manifest]
    if target_details:
        require(command == "declaration", "target details only apply to declarations")
        arguments.append("--target-details")
    if command == "inspect":
        arguments.append("--details")
    if requests is not None:
        arguments.extend(["--requests", requests])
    started = time.monotonic()
    code, stdout, stderr = run_bounded(arguments, cwd, timeout, max_output)
    elapsed_ms = (time.monotonic() - started) * 1000
    require(code == 0, f"{command} exited {code}: {stderr[:4000].decode('utf-8', 'replace')}")
    same(stderr, b"", f"{command} successful stderr")
    result = json.loads(stdout)
    same(result["schema_version"], 1, f"{command} response version")
    same(result["command"], command, "response command")
    snapshot = result["snapshot"]
    same(snapshot["identity"], "invocation_local", "snapshot identity boundary")
    require(isinstance(snapshot["id"], str) and snapshot["id"], "missing snapshot ID")
    same(snapshot["manifest"], str(manifest.resolve()), "snapshot manifest")
    label = "inspect --details" if command == "inspect" else command
    print(f"cli_invocation={label} manifest={manifest.name} elapsed_ms={elapsed_ms:.2f} "
          f"stdout_bytes={len(stdout)}", flush=True)
    return result


def rows(path, width):
    result = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if line and not line.startswith("#"):
            fields = line.split("\t")
            same(len(fields), width, f"fixture row: {path.name}")
            result.append(fields)
    return result


def load_goldens(sources):
    queries = rows(CORPUS / "queries.tsv", 6)
    targets = rows(CORPUS / "targets.tsv", 7)
    same(len(queries), 45, "fixed query count")
    same(len(targets), 70, "fixed candidate count")
    same(len({q[0] for q in queries}), 45, "unique query IDs")
    require({t[0] for t in targets} <= {q[0] for q in queries}, "orphan target fixture")
    for query in queries:
        identifier, path, offset, name, _, count = query
        data = sources[path]
        require(data[int(offset):].startswith(name.encode("utf-8")),
                f"{identifier}: query spelling is absent from pinned source")
        expected = [t for t in targets if t[0] == identifier]
        same(len(expected), int(count), f"{identifier}: fixture candidate count")
        for target in expected:
            if not target[1].startswith("!"):
                same(sources[target[1]][int(target[5]):int(target[6])],
                     target[4].encode("utf-8"), f"{identifier}: exact declaration spelling")
    return queries, targets


def requests_for(queries):
    return {"schema_version": 1, "queries": [
        {"compilation": COMPILATION, "file": "../sources/" + row[1],
         "position": {"encoding": "utf8", "offset": int(row[2])}}
        for row in queries
    ]}


def utf16(data, byte):
    return len(data[:byte].decode("utf-8").encode("utf-16-le")) // 2


def check_inspection(response, source_root, sources, expected_classes, platform):
    same(response["result"]["detail_mode"], "full", "explicit full inspection mode")
    compilations = response["result"]["compilations"]
    same(len(compilations), 1, "inspected compilation count")
    compilation = compilations[0]
    same(compilation["id"], COMPILATION, "inspected compilation")
    paths = sorted(str(source_root / name) for name in sources)
    same(compilation["source_paths"], paths, "exact owned source inventory")
    same(compilation["visible_source_paths"], paths, "exact visible source inventory")
    origins = compilation["class_origins"]
    same(compilation["class_count"], expected_classes, "binary class summary count")
    same(len(origins), expected_classes, "binary class inventory")
    same(len(set(origins)), expected_classes, "unique binary origins")
    require(all(path.startswith(str(platform) + "!/classes/") and path.endswith(".class")
                for path in origins), "unexpected binary artifact provenance")
    same(compilation["shadowed_classes"], [], "no duplicate binary classes")
    configuration = compilation["configuration"]
    same(configuration["source_roots"], ["../sources"], "manifest-relative source configuration")
    same(configuration["classpath"], [], "no duplicate Commons Lang binary input")
    same(configuration["target_release"], 21, "explicit target JAR release")
    same(len(configuration["platform"]), int(expected_classes != 0), "explicit platform configuration")
    print(f"inspect_sources={len(paths)} source_bytes={SOURCE_BYTES} platform_classes={len(origins)}")


def check_parser_report(response, inspection, source_root, sources, print_inventory=True):
    result = response["result"]
    classification = "parser_limit_or_invalid_syntax"
    same(result["classification"], classification, "parser report classification")
    same(result["java_semantic_diagnostics"], False, "parser reports are not Java semantic diagnostics")
    same(result["scope"], "owned_sources_only", "parser report source scope")
    files = result["files"]
    paths = inspection["result"]["compilations"][0]["source_paths"]
    same([file["path"] for file in files], paths, "parser report includes every inspected source once")
    counts = {}
    for file in files:
        same(file["compilation"], COMPILATION, "parser report compilation")
        relative = Path(file["path"]).relative_to(source_root).as_posix()
        data = sources[relative]
        if file["reports"]:
            counts[relative] = len(file["reports"])
        for gap in file["reports"]:
            same(gap["classification"], classification, f"{relative}: gap classification")
            require(isinstance(gap["message"], str) and gap["message"], f"{relative}: missing parser message")
            extent = gap["range"]
            start, end = extent["start_byte"], extent["end_byte"]
            require(0 <= start <= end <= len(data), f"{relative}: parser range outside source")
            same(extent["start_utf16"], utf16(data, start), f"{relative}: parser range UTF-16 start")
            same(extent["end_utf16"], utf16(data, end), f"{relative}: parser range UTF-16 end")
    same(result["source_count"], SOURCE_COUNT, "parser source count")
    same(result["files_with_reports"], len(counts), "parser affected-file summary")
    same(result["report_count"], sum(counts.values()), "parser gap summary")
    baseline = {path: int(count) for path, count in rows(CORPUS / "parser-errors.tsv", 2)}
    if print_inventory:
        print(f"parser_report_files={len(counts)} parser_reports={sum(counts.values())} "
              f"classification={classification}")
        # Parser coverage evolves. Keep the existing inventory-only policy;
        # inconsistent/missing records fail, but genuine baseline changes do not.
        changes = {path: {"baseline": baseline.get(path, 0), "actual": counts.get(path, 0)}
                   for path in sorted(set(baseline) | set(counts))
                   if baseline.get(path, 0) != counts.get(path, 0)}
        print("parser_baseline_changed=" + str(bool(changes)).lower())
        if changes:
            print("parser_baseline_changes=" + json.dumps(changes, sort_keys=True))


def parser_controls(response, checker):
    missing_file = copy.deepcopy(response)
    missing_file["result"]["files"].pop()
    false_semantics = copy.deepcopy(response)
    false_semantics["result"]["java_semantic_diagnostics"] = True
    wrong_summary = copy.deepcopy(response)
    wrong_summary["result"]["report_count"] += 1
    for changed in (missing_file, false_semantics, wrong_summary):
        try:
            checker(changed)
        except AcceptanceError:
            continue
        raise AcceptanceError("parser report negative control passed incorrectly")
    print("parser_controls=3 rejected (omission, semantic label, inconsistent count)")


def check_declarations(response, queries, targets, source_root, platform, sources, *, binary_baseline=False):
    result = response["result"]
    same(result["index_builds"], 1, "one prepared index per request batch")
    same(len(result["queries"]), len(queries), "returned query count")
    for index, (actual, query) in enumerate(zip(result["queries"], queries)):
        identifier, path, offset, _, status, count = query
        if identifier == "q06" and not binary_baseline:
            same(query, ["q06", "org/apache/commons/lang3/StringUtils.java", "15520", "length", "Resolved", "1"], "frozen q06 migration")
            same([t for t in targets if t[0] == identifier],
                 [["q06", "!/classes/java/lang/String.class", "Method", "java.lang.String", "length", "0", "0"]],
                 "frozen q06 original target")
            expected = copy.deepcopy(binary_method_plan()['queries'][8]['expected'])
            expected['query_index'] = index
            changed = copy.deepcopy(actual)
            changed['file'] = Path(changed['file']).relative_to(source_root).as_posix()
            for target in changed['targets']:
                target['path'] = target['path'].replace(str(platform), '@PLATFORM@')
            same(changed, expected, "q06 exact current full-record ancestor-hints migration")
            continue
        same(actual["query_index"], index, f"{identifier}: query order")
        same(actual["compilation"], COMPILATION, f"{identifier}: query compilation")
        same(actual["source_compilation"], COMPILATION, f"{identifier}: source ownership")
        same(actual["file"], str(source_root / path), f"{identifier}: canonical query path")
        same(actual["position"], {"encoding": "utf8", "offset": int(offset)},
             f"{identifier}: echoed position")
        same(actual["byte_offset"], int(offset), f"{identifier}: byte offset")
        same(actual["status"], status, f"{identifier}: status")
        same(len(actual["targets"]), int(count), f"{identifier}: candidate count")
        expected_targets = [target for target in targets if target[0] == identifier]
        for candidate, expected in zip(actual["targets"], expected_targets):
            _, origin, kind, owner, name, start, end = expected
            same(candidate["kind"], kind, f"{identifier}: target kind")
            same(candidate["owner"], "" if owner == "-" else owner, f"{identifier}: target owner")
            same(candidate["name"], name, f"{identifier}: target name")
            if origin.startswith("!"):
                same(candidate["origin"], "binary", f"{identifier}: binary provenance")
                same(candidate["source_available"], False, f"{identifier}: binary source unavailable")
                require("range" not in candidate, f"{identifier}: binary must not fabricate a source range")
                same(candidate["path"], str(platform) + origin, f"{identifier}: exact JMOD origin")
                same(candidate["binary_name"], origin.removeprefix("!/classes/").removesuffix(".class"),
                     f"{identifier}: binary identity")
                same(candidate["linking_context"], None, f"{identifier}: unknown binary linking context")
                same(candidate["loaded_contexts"], [COMPILATION], f"{identifier}: loaded binary contexts")
                same(candidate["identity_precision"], "origin_kind_owner_name_only",
                     f"{identifier}: binary identity boundary")
            else:
                same(candidate["origin"], "source", f"{identifier}: source provenance")
                same(candidate["source_available"], True, f"{identifier}: source available")
                same(candidate["path"], str(source_root / origin), f"{identifier}: exact source origin")
                same(candidate["compilation"], COMPILATION, f"{identifier}: declaration compilation")
                same(candidate["range"], {
                    "start_byte": int(start), "end_byte": int(end),
                    "start_utf16": utf16(sources[origin], int(start)),
                    "end_utf16": utf16(sources[origin], int(end)),
                }, f"{identifier}: exact declaration range")


BINARY_METHOD_ORIGINAL_SHA256 = "b7f49b891fc1dc7aa0cf1ecced9ba9ef8011c0b1928bbe17f3aa69c326ca5f74"
BINARY_METHOD_V2_PLAN_SHA256 = "fa35998d44456385aa134ee0bca4dad9bb172a105664b0e749d36ebc527e5bcf"
BINARY_METHOD_PLAN_SHA256 = "d9aba6446867da62fe779bb2b5251abe931357b3fb73bb3791a1f980d036f56f"
BINARY_METHOD_DEFERRAL_SHA256 = "53a8fdb62cebd1742ca418e53ee81210c80e54e7099b7f384e24e445659c5cd3"
BINARY_METHOD_TOPOLOGY_SHA256 = "56e42689d4bd2cf6b4ae99223d95f4625568e9a2334f0a43a7144334755375fa"
BINARY_METHOD_FIRST_FAILURE_SHA256 = "0dd492fde1ca342af02c6fc82ea1d546a4b6e2e9fdf9c741bcc68f71a3a2e6d5"


def binary_method_plan_v2(path=None):
    path = path or CORPUS / "binary-method-probes-v2.json"
    original = CORPUS / 'binary-method-probes.json'
    evidence_path = CORPUS / 'binary-method-topology-evidence.json'
    for file in (path, original, evidence_path):
        input_helpers().reject_symlink_path(file)
    same(sha256(original), BINARY_METHOD_ORIGINAL_SHA256, 'original forecast preserved')
    same(sha256(evidence_path), BINARY_METHOD_TOPOLOGY_SHA256, 'independent topology evidence')
    same(sha256(path), BINARY_METHOD_V2_PLAN_SHA256, "binary method v2 plan hash")
    old = json.loads(original.read_text())
    evidence = json.loads(evidence_path.read_text())
    for code in evidence['code']:
        same(hashlib.sha256(code['text'].encode()).hexdigest(), code['sha256'], 'exact mechanism excerpt')
    same([c['binary_name'] for c in evidence['classes']], ['java/lang/String','java/lang/CharSequence'], 'independent owners')
    require('java/lang/CharSequence' in evidence['classes'][0]['interfaces'], 'declared interface evidence')
    for c in evidence['classes']:
        same({m['name']:m['descriptor'] for m in c['methods']}, {'length':'()I','charAt':'(I)C'}, 'same erased signatures')
    rebuilt = copy.deepcopy(old)
    changes = []
    for i in (3,4,8):
        for key in ('expected','expected_details'):
            row = rebuilt['queries'][i][key]
            row['status'] = 'Ambiguous'
            target = copy.deepcopy(row['targets'][0])
            target['owner'] = 'java.lang.CharSequence'
            target['binary_name'] = 'java/lang/CharSequence'
            target['path'] = target['path'].replace('java/lang/String.class','java/lang/CharSequence.class')
            row['targets'].append(target)
        changes.append({'index':i, 'original_expected':old['queries'][i]['expected'],
                        'revised_expected':rebuilt['queries'][i]['expected'],
                        'original_expected_details':old['queries'][i]['expected_details'],
                        'revised_expected_details':rebuilt['queries'][i]['expected_details']})
    rebuilt['scope_revision'] = {'supersedes_sha256':BINARY_METHOD_ORIGINAL_SHA256,
        'evidence_sha256':BINARY_METHOD_TOPOLOGY_SHA256, 'first_failure_sha256':BINARY_METHOD_FIRST_FAILURE_SHA256,
        'reason':evidence['reason'], 'changes':changes}
    plan = json.loads(path.read_text())
    same(plan, rebuilt, 'only three explicit full-record topology revisions; no positive changed')
    same(len(plan['queries']), 10, "complete real binary method inventory")
    same([q['baseline']['byte_offset'] for q in plan['queries']],
         [17502,27716,17430,27662,11304,30719,33251,36948,15520,253882], "exact binary method sites")
    same(sum(q['classification'] == 'new_selection' for q in plan['queries']), 6, "six new selections")
    return plan


def binary_method_plan(path=None):
    previous = binary_method_plan_v2()
    path = path or CORPUS / 'binary-method-probes-v3.json'
    evidence_path = CORPUS / 'binary-method-header-deferral.json'
    for file in (path,evidence_path):
        input_helpers().reject_symlink_path(file)
    same(sha256(path), BINARY_METHOD_PLAN_SHA256, 'binary method v3 plan hash')
    same(sha256(evidence_path), BINARY_METHOD_DEFERRAL_SHA256, 'exact header deferral evidence')
    evidence = json.loads(evidence_path.read_text())
    same(evidence['source_sha256'], previous['source_sha256'][evidence['source_path']], 'unchanged original source')
    same(evidence['source_range'][0], 5865, 'exact unsupported method header')
    same(evidence['source_excerpt'], 'public static Comparator<Class<?>> comparator()', 'generic header not literal blocker')
    for item in evidence['trace_logs'] + [evidence['trace_code']]:
        same(hashlib.sha256(item['text'].encode()).hexdigest(), item['sha256'], 'exact raw diagnosis text')
    logs = {item['name']:item['text'] for item in evidence['trace_logs']}
    require('INVENTORY method=Some("comparator") offset=5865' in logs['inventory-header-trace.log'], 'exact method trace')
    require('INV owner=Some("ClassUtils") start=1313 line=3496' in logs['inventory3-raw.log'], 'exact class inventory trace')
    for offset in (17502,27716,17430):
        require(f'TRACE {offset} receiver=false' in logs['diagnostic-stages.log'], 'failure before actual classification')
    same(evidence['independent_string_methods']['jmod_sha256'],previous['platform_sha256'],'descriptor evidence platform')
    rebuilt = copy.deepcopy(previous)
    changes = []
    for i in (0,1,2):
        row = rebuilt['queries'][i]
        row['classification'] = 'deferred_enclosing_generic_header'
        row['expected'] = copy.deepcopy(row['baseline'])
        detail = copy.deepcopy(row['baseline'])
        methods = [m for m in evidence['independent_string_methods']['methods'] if m['name'] == detail['targets'][0]['name']]
        same(len(methods), len(detail['targets']), 'complete direct method descriptor inventory')
        for target,method in zip(detail['targets'],methods):
            target.update(descriptor=method['descriptor'], linking_context=COMPILATION,
                          identity_precision='origin_binary_name_descriptor_linking_context')
        row['expected_details'] = detail
        changes.append({'index':i, 'original':previous['queries'][i], 'revised':row})
    rebuilt['deferral_revision'] = {'supersedes_sha256':BINARY_METHOD_V2_PLAN_SHA256,
        'evidence_sha256':BINARY_METHOD_DEFERRAL_SHA256, 'first_failure_sha256':BINARY_METHOD_FIRST_FAILURE_SHA256,
        'reason':evidence['reason'], 'changes':changes}
    plan = json.loads(path.read_text())
    same(plan,rebuilt,'only three explicit header deferrals; all other v2 records unchanged')
    same(sum(q['classification']=='new_selection' for q in plan['queries']),3,'three remaining prospective real improvements')
    return plan



HEADER_INVENTORY_PLAN_SHA256 = 'fcc6271919de7ce2fd271e17fdb157c58cb093384a02a7d3f5b25978001d3cca'


def header_inventory_prediction(path=None):
    previous = binary_method_plan()
    path = path or CORPUS / 'header-inventory-probes.json'
    input_helpers().reject_symlink_path(path)
    same(sha256(path), HEADER_INVENTORY_PLAN_SHA256, 'frozen header inventory forecast')
    original = json.loads((CORPUS / 'binary-method-probes.json').read_text())
    expected = {
        'schema_version': 1, 'feature': 'return_header_inventory',
        'baseline_revision': '0697877ba11de2eb8735029540cafd07805e6d7b',
        'baseline_native_sha256': '26b9c79b86b413eabe378992cfefd5cb184f5bd460352e92ee24744e7a3fd60c',
        'previous_plan_sha256': sha256(CORPUS / 'binary-method-probes-v3.json'),
        'initial_forecast_sha256': sha256(CORPUS / 'binary-method-probes.json'), 'changes': [],
    }
    for i in (0, 1, 2):
        before, forecast = previous['queries'][i], original['queries'][i]
        same(before['id'], forecast['id'], 'same independent original query')
        same(before['classification'], 'deferred_enclosing_generic_header', 'only deferred header queries')
        expected['changes'].append({
            'id': before['id'], 'baseline': before['expected'], 'baseline_details': before['expected_details'],
            'expected': forecast['expected'], 'expected_details': forecast['expected_details'],
        })
    plan = json.loads(path.read_text())
    same(plan, expected, 'complete old records and original independently frozen targets')
    return plan


def header_inventory_plan():
    result = copy.deepcopy(binary_method_plan())
    prediction = header_inventory_prediction()
    for row, change in zip(result['queries'][:3], prediction['changes']):
        same(row['id'], change['id'], 'fixed header query order')
        row['classification'] = 'new_header_inventory_selection'
        row['expected'] = copy.deepcopy(change['expected'])
        row['expected_details'] = copy.deepcopy(change['expected_details'])
    return result


def check_binary_methods(response, plan, source_root, platform, *, baseline=False, details=False):
    require(not (baseline and details), "old binary has no details protocol")
    same(response['result']['index_builds'], 1, "one binary proof index")
    require("target_details_version" not in response, "details version belongs only in result")
    if details:
        same(response['result'].get('target_details_version'), 1, "explicit target details version")
    else:
        require('target_details_version' not in response['result'], "default protocol must not expose details version")
    rows = copy.deepcopy(response['result']['queries'])
    for row in rows:
        row['file'] = Path(row['file']).relative_to(source_root).as_posix()
        for target in row['targets']:
            target['path'] = target['path'].replace(str(platform), '@PLATFORM@')
    key = 'baseline' if baseline else 'expected_details' if details else 'expected'
    same(rows, [q[key] for q in plan['queries']], 'exact binary method rows including descriptor/context and multiplicity')


INHERITED_PRESERVATION_SHA256 = '573cf8d3b94ac18a86bd9ffa5d69a988fee499ade0606662b4490995bfde3300'


def inherited_preservation_plan(source_root, platform, path=None):
    path = CORPUS / 'inherited-source-preservation.json' if path is None else path
    input_helpers().reject_symlink_path(path)
    same(sha256(path), INHERITED_PRESERVATION_SHA256, 'complete Commons inherited preservation plan')
    plan = json.loads(path.read_text())
    specs = json.loads((ROOT / 'scripts/acceptance-inputs.json').read_text())
    same(plan['source_archive_sha256'], specs['sources']['sha256'], 'Commons inherited source archive')
    same(plan['platform_sha256'], sha256(platform), 'Commons inherited platform')
    for name, checksum in plan['source_sha256'].items():
        same(sha256(source_root / name), checksum, 'Commons inherited source ' + name)
    same(len(plan['queries']), 4, 'four preserved Commons inherited getters')
    return plan


def check_inherited_preservation(response, plan, source_root):
    same(response['result']['index_builds'], 1, 'Commons inherited prepared index')
    rows = copy.deepcopy(response['result']['queries'])
    same(len(rows), 4, 'exact Commons inherited preservation row count')
    for row in rows:
        row['file'] = Path(row['file']).relative_to(source_root).as_posix()
        for target in row['targets']:
            target['path'] = Path(target['path']).relative_to(source_root).as_posix()
    same(rows, plan['queries'], 'complete Commons inherited source records, already resolved before this slice')


def comparator_controls(response, checker):
    """A missing result or a plausible wrong declaration must fail the gate."""
    missing_query = copy.deepcopy(response)
    missing_query["result"]["queries"].pop()
    wrong_target = copy.deepcopy(response)
    wrong_target["result"]["queries"][0]["targets"][0]["range"]["start_byte"] += 1
    missing_candidate = copy.deepcopy(response)
    missing_candidate["result"]["queries"][6]["targets"].pop()
    invented_resolution = copy.deepcopy(response)
    invented_resolution["result"]["queries"][11]["status"] = "Resolved"
    for name, changed in (("missing_query", missing_query), ("wrong_target", wrong_target),
                          ("missing_candidate", missing_candidate),
                          ("invented_resolution", invented_resolution)):
        try:
            checker(changed)
        except AcceptanceError:
            continue
        raise AcceptanceError(f"negative control passed incorrectly: {name}")
    print("comparator_controls=4 rejected (omission, wrong range, lost candidate, invented resolution)")


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("sources_jar", type=Path)
    parser.add_argument("java_base_jmod", type=Path)
    parser.add_argument("--native", type=Path, default=ROOT / "target/release/java-analyzer",
                        help="already-built product CLI (default: target/release/java-analyzer)")
    parser.add_argument("--timeout-seconds", type=float, default=60)
    parser.add_argument("--max-output-bytes", type=int, default=16 * 1024 * 1024)
    capture = parser.add_mutually_exclusive_group()
    capture.add_argument("--capture-binary-method-baseline", action="store_true", help="verify exact frozen 650565ff binary without future target-details")
    capture.add_argument("--capture-header-inventory-baseline", action="store_true", help="verify exact frozen 0697877b default and detailed records before header inventory changes")
    args = parser.parse_args()
    if not 0 < args.timeout_seconds <= 600:
        parser.error("timeout-seconds must be greater than zero and at most 600")
    if not 1024 <= args.max_output_bytes <= 64 * 1024 * 1024:
        parser.error("max-output-bytes must be between 1024 and 67108864")
    try:
        specs = json.loads((ROOT / "scripts/acceptance-inputs.json").read_text(encoding="utf-8"))
        security = input_helpers()
        security.reject_symlink_path(args.sources_jar)
        security.reject_symlink_path(args.java_base_jmod)
        source = args.sources_jar.resolve(strict=True)
        platform = args.java_base_jmod.resolve(strict=True)
        require(platform.is_file(), "platform input must be a regular file")
        require(platform.stat().st_size <= specs["jdk"]["max_file"], "platform input size budget exceeded")
        same(sha256(platform), specs["jdk"]["jmod_sha256"], "platform SHA-256")
        native = args.native.resolve(strict=True)
        require(native.is_file() and os.access(native, os.X_OK), "--native must name an executable product CLI")
        historical = args.capture_binary_method_baseline or args.capture_header_inventory_baseline
        binary_plan = binary_method_plan() if historical else header_inventory_plan()
        if args.capture_header_inventory_baseline:
            same(sha256(native), header_inventory_prediction()['baseline_native_sha256'], 'exact header inventory baseline binary')
        same(binary_plan["source_archive_sha256"], specs["sources"]["sha256"], "binary source artifact")
        same(binary_plan["platform_sha256"], specs["jdk"]["jmod_sha256"], "binary platform artifact")
        if args.capture_binary_method_baseline:
            same(sha256(native), binary_plan["baseline_native_sha256"], "exact historical binary")
        with tempfile.TemporaryDirectory(prefix="java-analyzer-cli-acceptance-") as temporary:
            stage = Path(temporary).resolve()
            source_root, notices, cwd = stage / "sources", stage / "notices", stage / "unrelated-cwd"
            for directory in (source_root, notices, cwd):
                directory.mkdir()
            sources = extract_sources(source, specs["sources"], source_root, notices)
            queries, targets = load_goldens(sources)
            manifest = stage / "config/project.json"
            requests = stage / "queries/requests.json"
            config = {"schema_version": 1, "compilations": [{
                "id": COMPILATION, "source_roots": ["../sources"], "dependencies": [],
                "classpath": [], "target_release": 21,
                "platform": [{"kind": "jmod", "path": os.path.relpath(platform, manifest.parent)}],
            }]}
            write_json(manifest, config)
            write_json(requests, requests_for(queries))
            def invoke(command, selected_manifest=manifest, selected_requests=None):
                return run_cli(native, cwd, selected_manifest, command, args.timeout_seconds,
                               args.max_output_bytes, selected_requests)
            print(f"native={native}\nnative_sha256={sha256(native)}", flush=True)
            print(f"source_sha256={specs['sources']['sha256']}\nplatform_sha256={specs['jdk']['jmod_sha256']}")
            print("environment=PATH=/nonexistent (no inherited variables); cwd=unrelated-cwd")
            inspection = invoke("inspect")
            check_inspection(inspection, source_root, sources, PLATFORM_CLASSES, platform)
            report = invoke("parser-report")
            check_parser_report(report, inspection, source_root, sources)
            parser_controls(report, lambda changed: check_parser_report(
                changed, inspection, source_root, sources, print_inventory=False))
            declarations = invoke("declaration", selected_requests=requests)
            def check(value):
                check_declarations(value, queries, targets, source_root, platform, sources, binary_baseline=args.capture_binary_method_baseline)
            check(declarations)
            comparator_controls(declarations, check)
            inheritance = inherited_preservation_plan(source_root, platform)
            inherited_requests = stage / 'queries/inherited-preservation.json'
            write_json(inherited_requests, {'schema_version':1, 'queries':[
                {'compilation':q['compilation'], 'file':str(source_root/q['file']), 'position':q['position']}
                for q in inheritance['queries']]})
            check_inherited_preservation(invoke('declaration', selected_requests=inherited_requests), inheritance, source_root)
            print('inherited_preservation=4 previously resolved Commons getters unchanged; no new-success claim')
            for path, digest in binary_plan['source_sha256'].items():
                same(hashlib.sha256(sources[path]).hexdigest(), digest, 'binary method exact source')
            binary_requests = stage / 'queries/binary-methods.json'
            write_json(binary_requests, {'schema_version': 1, 'queries': [
                {'compilation': q['baseline']['compilation'], 'file': str(source_root / q['baseline']['file']),
                 'position': q['baseline']['position']} for q in binary_plan['queries']]})
            check_binary_methods(invoke('declaration', selected_requests=binary_requests), binary_plan,
                                 source_root, platform, baseline=args.capture_binary_method_baseline)
            if not args.capture_binary_method_baseline:
                detailed = run_cli(native, cwd, manifest, 'declaration', args.timeout_seconds,
                                   args.max_output_bytes, binary_requests, target_details=True)
                check_binary_methods(detailed, binary_plan, source_root, platform, details=True)
            print('binary_methods=10 exact complete records; header_inventory_new_predictions=' +
                  ('0' if historical else '3') + '; stage=' +
                  ('baseline_verified_not_acceptance' if historical else 'current'))
            print("declarations=45 candidates=" + str(sum(len(q["targets"]) for q in declarations["result"]["queries"])))
            print("statuses=" + json.dumps(dict(sorted(Counter(q["status"] for q in declarations["result"]["queries"]).items()))))
            # Prove the JMOD changes real binding, not that absent Java means absent platform data.
            config["compilations"][0]["platform"] = []
            without_platform = stage / "config/without-platform.json"
            write_json(without_platform, config)
            missing_inspection = invoke("inspect", without_platform)
            check_inspection(missing_inspection, source_root, sources, 0, platform)
            control_requests = stage / "queries/without-platform.json"
            write_json(control_requests, requests_for([queries[0], queries[2]]))
            missing = invoke("declaration", without_platform, control_requests)
            source_query, binary_query = missing["result"]["queries"]
            same(source_query["status"], "Resolved", "missing-platform source control status")
            same(source_query["targets"], declarations["result"]["queries"][0]["targets"],
                 "missing-platform source target unchanged")
            same(binary_query["status"], "Unresolved", "missing-platform binary control status")
            same(binary_query["targets"], [], "missing-platform binary candidates absent")
            print("missing_platform_control=source target unchanged; Character Unresolved; binary count 0")
            print("PASS shipped CLI fixed-corpus acceptance; no Java/build/download invoked.")
            print("Bounded fixture evidence only, not Java conformance, full project support, or runtime timeout safety.")
    except (AcceptanceError, OSError, ValueError, KeyError, TypeError, IndexError,
            zipfile.BadZipFile, RuntimeError, EOFError, zlib.error, subprocess.SubprocessError) as error:
        parser.exit(1, f"CLI ACCEPTANCE FAILED: {error}\n")


if __name__ == "__main__":
    main()
