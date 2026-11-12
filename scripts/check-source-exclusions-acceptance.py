#!/usr/bin/env python3
"""Independent, offline Gson whole-root selection acceptance. Never executes Java.

The historical narrowed-root checkers/goldens are unchanged. The only manual
model change is gson/main's ordinary java root plus literal module-info.java
exclusion. Upstream POM is inspected as pinned data, never executed or imported.
Every attempted invocation has an exclusive-create receipt before validation.
"""
import argparse
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unicodedata
import xml.etree.ElementTree as ET
import zipfile

ROOT = Path(__file__).resolve().parent.parent


def load(name):
    spec = importlib.util.spec_from_file_location('source_exclusions_' + name.replace('-', '_'),
                                                ROOT / 'scripts' / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


configure_gate = load('check-configure-acceptance')
second = configure_gate.second
strict = load('check-strict-reference-acceptance')
lsp = load('check-lsp-project')
require, AcceptanceError = second.require, second.AcceptanceError
canonical, strict_json = configure_gate.canonical, configure_gate.strict_json
write_new, tree_state = configure_gate.write_new, configure_gate.tree_state
WORKSPACE_LIMITS = configure_gate.WORKSPACE_LIMITS
MAX_MANIFEST = 1024 * 1024
MAX_OUTPUT = 20 * 1024 * 1024
WHOLE_ROOT = 'gson/src/main/java'
DESCRIPTOR = WHOLE_ROOT + '/module-info.java'
DESCRIPTOR_SHA256 = 'c290e0ed30dc226f68cddeb5af1a05693bf2215b347d77eac9e61f23f29aca30'
POM = 'gson/pom.xml'
# The immutable archive authenticates every POM byte; this extra member pin is
# filled from independent archive inspection, never candidate analyzer output.
POM_SHA256 = 'd2b115634f5c085db4b9c9ffc2658e89e231fdbfbe2242121a1cd95d4d948dd7'
SOURCE_ARCHIVE_SHA256 = '3678276a90e914bfabeb069c2c7ff99460f5de951baf65e764a2c9731946da13'
MAX_RULES, MAX_RULE_BYTES = 1024, 4096


def same(actual, expected, label):
    if type(actual) is bytes or type(expected) is bytes:
        require(type(actual) is type(expected) and actual == expected, label + ': exact bytes differ')
    else:
        configure_gate.same(actual, expected, label)


def rules(value):
    """Independent literal rule contract, including Cc rather than isprintable."""
    require(type(value) is list and len(value) <= MAX_RULES, 'exclusions must be an array of at most 1024 rules')
    seen = set()
    for rule in value:
        require(type(rule) is str and 0 < len(rule.encode('utf-8')) <= MAX_RULE_BYTES,
                'rule must contain 1..4096 UTF-8 bytes')
        require(rule not in seen, 'duplicate exclusion rule')
        seen.add(rule)
        require(not any(char in rule for char in '\\:*?[]{}'), 'nonliteral exclusion rule')
        require(not any(unicodedata.category(char) == 'Cc' for char in rule), 'control in exclusion rule')
        parts = rule.split('/')
        require(all(part not in ('', '.', '..') for part in parts), 'invalid relative rule component')
    return list(value)


def matches(relative, exclusions):
    return any(relative == rule or relative.startswith(rule + '/')
               for rule in rules(exclusions))


def effective_manifest(value, base, require_absolute=False):
    """Add one validated field without weakening the historical normalizer."""
    require(type(value) is dict and type(value.get('compilations')) is list, 'manifest object/compilations')
    original = copy.deepcopy(value)
    selections = []
    for compilation in original['compilations']:
        require(type(compilation) is dict, 'compilation object')
        selections.append(rules(compilation.pop('source_exclusions', [])))
    result = configure_gate.effective_manifest(original, base, require_absolute)
    for compilation, exclusions in zip(result['compilations'], selections):
        require(not exclusions or compilation['source_roots'], 'nonempty exclusions require source roots')
        compilation['source_exclusions'] = exclusions
    return result


def compare_manifests(generated, manual, base):
    actual = effective_manifest(generated, base, True)
    for compilation in generated['compilations']:
        require('source_exclusions' not in compilation or compilation['source_exclusions'],
                'configure omits empty exclusion fields')
    same(actual, effective_manifest(manual, base), 'independent manual/generated effective manifest')
    return actual


def gson_main(model):
    found = [c for c in model['compilations'] if c['id'] == {'project': 'gson', 'source_set': 'main'}]
    same(len(found), 1, 'exactly one gson/main compilation ID')
    return found[0]


def selected_model(fixture):
    model = copy.deepcopy(fixture.manifest_data)
    main = gson_main(model)
    same(main['source_roots'], ['sources/' + WHOLE_ROOT + '/com'], 'unchanged historical narrowed root')
    main['source_roots'] = ['sources/' + WHOLE_ROOT]
    main['source_exclusions'] = ['module-info.java']
    return model


def arguments(fixture, cwd, exclude=True):
    """Independently authored flags; insert into the group identified by its ID."""
    args = configure_gate.gson_arguments(fixture, cwd)
    start = next(i for i, arg in enumerate(args) if arg == '--compilation' and args[i+1:i+3] == ['gson', 'main'])
    end = next((i for i in range(start+3, len(args)) if args[i] == '--compilation'), len(args))
    group = args[start:end]
    same(group[:4], ['--compilation', 'gson', 'main', '--source-root'], 'independent gson/main flag group')
    group[4] = os.path.relpath(fixture.source_root / WHOLE_ROOT, cwd)
    if exclude:
        group[5:5] = ['--exclude-source', 'module-info.java']
    return args[:start] + group + args[end:]


def pom_semantics(data):
    require(b'<!DOCTYPE' not in data.upper() and b'<!ENTITY' not in data.upper(), 'POM external entities forbidden')
    root = ET.fromstring(data)
    ns = {'m': 'http://maven.apache.org/POM/4.0.0'}
    same(root.tag, '{http://maven.apache.org/POM/4.0.0}project', 'namespaced Maven POM')
    plugins = [p for p in root.findall('m:build/m:plugins/m:plugin', ns)
               if p.findtext('m:artifactId', namespaces=ns) == 'maven-compiler-plugin']
    same(len(plugins), 1, 'one direct build compiler plugin, not profile/pluginManagement')
    executions = [e for e in plugins[0].findall('m:executions/m:execution', ns)
                  if e.findtext('m:id', namespaces=ns) == 'default-compile']
    same(len(executions), 1, 'one compiler default-compile execution')
    same([e.text for e in executions[0].findall('m:id', ns)], ['default-compile'], 'unique execution ID')
    exclusions = executions[0].findall('m:configuration/m:excludes/m:exclude', ns)
    same([entry.text for entry in exclusions], ['module-info.java'], 'default-compile exact descriptor exclusion')
    return {'plugin': 'maven-compiler-plugin', 'execution': 'default-compile',
            'excludes': ['module-info.java'], 'interpretation': 'Explicit reviewed input; not automatic Maven import'}


def verify_member(data, expected, label):
    actual = hashlib.sha256(data).hexdigest()
    same(actual, expected, label + ' SHA-256')
    return actual


def archive_evidence(fixture):
    spec = strict_json(second.PINS.read_bytes())['artifacts']['sources']
    archive = fixture.inputs['sources']
    same(spec['sha256'], SOURCE_ARCHIVE_SHA256, 'immutable source archive pin')
    same(archive.stat().st_size, spec['size'], 'pinned source archive size')
    verify_member(archive.read_bytes(), SOURCE_ARCHIVE_SHA256, 'pinned source archive')
    with zipfile.ZipFile(archive) as source:
        names = source.namelist()
        same(len(names), len(set(names)), 'no duplicate source archive members')
        prefix = spec['root'] + '/'
        pom, descriptor = source.read(prefix + POM), source.read(prefix + DESCRIPTOR)
    descriptor_hash = verify_member(descriptor, DESCRIPTOR_SHA256, 'pinned module descriptor')
    same((fixture.source_root / DESCRIPTOR).read_bytes(), descriptor, 'descriptor original extracted bytes')
    pom_hash = hashlib.sha256(pom).hexdigest()
    verify_member(pom, POM_SHA256, 'pinned Gson POM')
    semantics = pom_semantics(pom)
    return {'source_archive_sha256': SOURCE_ARCHIVE_SHA256, 'pom_path': POM,
            'pom_sha256': pom_hash, 'descriptor_path': DESCRIPTOR,
            'descriptor_sha256': descriptor_hash, 'default_compile': semantics}


def check_inventory(fixture):
    actual = second.all_source_hashes(fixture)
    expected = strict_json((second.CORPUS / 'local-type-comparison.json').read_bytes())['all_226_source_sha256']
    same(actual, expected, 'all 226 original Java paths and SHA-256 values')
    selected = {}
    for key, (root, count, size) in second.ROOTS.items():
        root = WHOLE_ROOT if key == 'gson/main' else root
        exclusion = ['module-info.java'] if key == 'gson/main' else []
        owned = sorted(name for name in actual if name.startswith(root + '/')
                       and not matches(name[len(root)+1:], exclusion))
        same(owned, sorted(p for p in fixture.sources if fixture.owner[p] == key), key + ' independently selected paths')
        same(len(owned), count, key + ' independent owned count')
        same(sum((fixture.source_root / name).stat().st_size for name in owned), size, key + ' owned byte count')
        selected[key] = owned
    same(sum(map(len, selected.values())), 211, 'whole-root selected source count')
    require(DESCRIPTOR in actual and all(DESCRIPTOR not in rows for rows in selected.values()), 'descriptor exists but is excluded')
    return actual, selected


class Runner:
    """Save every bounded stdout/stderr outcome before inspecting success/failure."""
    def __init__(self, native, receipts, fixture):
        self.native, self.receipts, self.fixture = native, receipts, fixture
        receipts.mkdir(parents=True, exist_ok=True)
        require(not any(receipts.iterdir()), 'receipt directory must be fresh')
        self.number = 0

    def invoke(self, label, args, cwd, limit=MAX_OUTPUT):
        self.number += 1
        name = f'{self.number:03d}-{label}'
        before = tree_state(self.fixture.source_root)
        command = [self.native, *args]
        receipt = {'command': list(map(str, command)), 'cwd': str(cwd), 'stdout_limit': limit,
                   'environment': {'PATH': '/nonexistent'}, 'source_tree_before': before}
        try:
            code, stdout, stderr = configure_gate.commons.run_bounded(command, cwd, 90, limit)
        except Exception as error:
            receipt['exception'] = type(error).__name__ + ': ' + str(error)
            receipt['source_tree_after'] = tree_state(self.fixture.source_root)
            write_new(self.receipts / (name + '.json'), receipt)
            raise
        write_new(self.receipts / (name + '.stdout'), stdout)
        write_new(self.receipts / (name + '.stderr'), stderr)
        after = tree_state(self.fixture.source_root)
        receipt.update(exit_code=code, stdout_sha256=hashlib.sha256(stdout).hexdigest(),
                       stderr_sha256=hashlib.sha256(stderr).hexdigest(), source_tree_after=after)
        write_new(self.receipts / (name + '.json'), receipt)
        same(after, before, label + ': no source paths, bytes, modes or mtimes changed')
        return code, stdout, stderr

    def success(self, label, command, fixture, extra=()):
        code, stdout, stderr = self.invoke(label, [command, '--manifest', fixture.manifest, *extra], fixture.workdir.parent)
        same(code, 0, label + ' exit; stderr=' + stderr[:2000].decode('utf-8', 'replace'))
        same(stderr, b'', label + ' stderr')
        response = strict_json(stdout)
        second.check_envelope(command, response, fixture)
        same(response['schema_version'], 1, 'integer response schema')
        return response

    def failure(self, label, args, cwd, expected_code, text=None):
        result = self.invoke(label, args, cwd)
        error = configure_gate.check_failure(*result)
        same(error['error']['code'], expected_code, label + ' exact refusal code')
        if text:
            require(text in canonical(error), label + ' precise offending input')
        return error


def run_configure(runner, fixture, model, cwd, stage):
    before = tree_state(stage)
    code, stdout, stderr = runner.invoke('configure', ['configure', *arguments(fixture, cwd)], cwd, MAX_MANIFEST)
    # Receipts are outside stage, so any other writes are unauthorized.
    same(tree_state(stage), before, 'configure did not write product, config or cwd files')
    same(code, 0, 'configure exit; stderr=' + stderr[:2000].decode('utf-8', 'replace'))
    same(stderr, b'', 'configure stderr')
    require(len(stdout) <= MAX_MANIFEST and stdout.endswith(b'\n'), 'bounded newline-terminated configure output')
    normalized = compare_manifests(strict_json(stdout), model, fixture.manifest.parent)
    return stdout, normalized


def check_inspection(response, fixture):
    second.check_envelope('inspect', response, fixture)
    same(response['schema_version'], 1, 'integer inspect schema')
    result = response['result']
    same(result['detail_mode'], 'summary', 'summary inspection')
    same(result['workspace_limits'], WORKSPACE_LIMITS, 'unchanged workspace budgets')
    same([c['id'] for c in result['compilations']], [second.compilation(key) for key in sorted(second.ROOTS)], 'four ordered scopes')
    expected = effective_manifest(fixture.manifest_data, fixture.manifest.parent)
    for record, config in zip(result['compilations'], expected['compilations']):
        key = second.key_of(record['id'])
        same(record['source_paths'], fixture.owned[key], key + ' exact owned paths')
        same(record['visible_source_paths'], fixture.visible[key], key + ' exact dependency visibility')
        same(record['class_count'], second.CLASS_COUNTS[key], key + ' exact class count')
        same(record['shadowed_classes'], [], key + ' shadowed classes')
        require('class_origins' not in record, 'summary contains no full origins')
        actual = effective_manifest({'schema_version': 1, 'compilations': [record['configuration']]}, fixture.manifest.parent)
        same(actual['compilations'][0], config, key + ' exact effective configuration')
    return result


def compare_inspection(actual, original, actual_fixture, original_fixture, historical=False):
    check_inspection(actual, actual_fixture)
    check_inspection(original, original_fixture)
    left, right = copy.deepcopy(actual['result']), copy.deepcopy(original['result'])
    for records, fixture in [(left['compilations'], actual_fixture), (right['compilations'], original_fixture)]:
        for record in records:
            record['configuration'] = effective_manifest({'schema_version': 1, 'compilations': [record['configuration']]},
                                                         fixture.manifest.parent)['compilations'][0]
            if historical and record['id'] == second.compilation('gson/main'):
                record['configuration']['source_roots'] = [str(fixture.source_root / WHOLE_ROOT)]
                record['configuration']['source_exclusions'] = ['module-info.java']
    same(actual['capabilities'], original['capabilities'], 'same capabilities')
    same(left, right, 'entire inspection except explicit reviewed historical root/rule change')


def binary_inventory(runner, historical, selected, generated, stage):
    inventories = {}
    for key in sorted(second.ROOTS):
        results = []
        for label, fixture in [('historical', historical), ('manual', selected), ('generated', generated)]:
            config = copy.deepcopy(next(c for c in fixture.manifest_data['compilations'] if c['id'] == second.compilation(key)))
            config.update(source_roots=[], dependencies=[])
            # This is explicitly binary-only: remove only the now-inapplicable selection rules.
            config.pop('source_exclusions', None)
            model = effective_manifest({'schema_version': 1, 'compilations': [config]}, fixture.manifest.parent)
            manifest = stage / (key.replace('/', '-') + '-' + label + '.json')
            write_new(manifest, model)
            projection = copy.copy(fixture)
            projection.manifest = manifest
            response = runner.success('binary-' + key.replace('/', '-') + '-' + label, 'inspect', projection, ['--details'])
            record = response['result']['compilations'][0]
            same(record['source_paths'], [], 'projection no sources')
            same(record['visible_source_paths'], [], 'projection no source graph')
            same(record['class_count'], second.CLASS_COUNTS[key], key + ' binary count')
            same(len(record['class_origins']), second.CLASS_COUNTS[key], key + ' complete origin count')
            results.append(response)
        for result in results[1:]:
            same(result['result'], results[0]['result'], key + ' all ordered binary inventory/configuration fields')
            same(result['capabilities'], results[0]['capabilities'], key + ' binary capabilities')
        origins = results[0]['result']['compilations'][0]['class_origins']
        inventories[key] = {'count': len(origins), 'ordered_origins_sha256': hashlib.sha256(canonical(origins).encode()).hexdigest()}
    return inventories


# Current overlays retain the prior 40 records and explicitly add the non-null binary-argument query.
CURRENT_PROBE_GROUPS = (('original_16', 16), ('reference_3', 3), ('local_5', 5),
                        ('string_8', 8), ('binary_argument_9', 9))
CURRENT_QUERY_COUNT = 41

def probe_groups(fixture):
    original_expectations = second.current_original_expectations(fixture)
    same(len(fixture.plan), 16, 'complete original query inventory before pairing')
    same(len(original_expectations), 16, 'complete original expected inventory before pairing')
    originals = [{**query, 'expected': expected} for query, expected in
                 zip(fixture.plan, original_expectations)]
    reference, historical, parser_digest = second.reference_call_probes(fixture)
    reference = second.qualified_reference_probes(fixture, reference)
    groups = [('original_16', originals), ('reference_3', reference),
              ('local_5', second.local_type_probes(fixture)),
              ('string_8', strict.string.probes(fixture)), ('binary_argument_9', strict.current_probes(fixture))]
    same(tuple((name, len(probes)) for name, probes in groups), CURRENT_PROBE_GROUPS,
         'exact current frozen group identity, ordering and cardinality')
    same(sum(len(probes) for _, probes in groups), CURRENT_QUERY_COUNT,
         '41 current frozen navigation/refusal records including exact binary-argument recoveries')
    return groups, historical, parser_digest


def preservation(runner, fixture, label):
    groups, historical, parser_digest = probe_groups(fixture)
    report = {}
    for name, probes in groups:
        request = fixture.workdir / (label + '-' + name + '.json')
        write_new(request, {'schema_version': 1, 'queries': [
            {'compilation': q['compilation'], 'file': 'sources/' + q['file'],
             'position': {'encoding': 'utf8', 'offset': q['byte']}} for q in probes]})
        response = runner.success(label + '-' + name, 'declaration', fixture, ['--requests', request])
        second.check_reference_call_response(response, probes, fixture)
        report[name] = second.portable_queries(response, fixture)
    configure_gate.missing_generated(report['original_16'], fixture)
    parser = runner.success(label + '-parser', 'parser-report', fixture)['result']
    second.check_reference_parser(parser, fixture, historical, parser_digest)
    portable = copy.deepcopy(parser)
    for file in portable['files']:
        file['path'] = Path(file['path']).relative_to(fixture.source_root).as_posix()
    report['parser'] = portable
    same(portable['report_count'], 166, 'integer parser count')
    same(portable['files_with_reports'], 44, 'integer parser affected count')
    return report


def failure_controls(runner, fixture, selected, stage):
    results = {'no-configure-rule': runner.failure('no-configure-rule',
        ['configure', *arguments(fixture, stage, exclude=False)], stage, 'load_failed', 'module-info.java')}
    for label, excluded in [('removed-rule', None), ('wrong-rule', ['not-module-info.java'])]:
        model = copy.deepcopy(selected.manifest_data)
        main = gson_main(model)
        if excluded is None:
            main.pop('source_exclusions')
        else:
            main['source_exclusions'] = excluded
        # These models retain fixture-relative paths, so preserve their base.
        path = fixture.manifest.parent / (label + '.json')
        write_new(path, model)
        results[label] = runner.failure(label, ['inspect', '--manifest', path], stage, 'load_failed', 'module-info.java')
    request = stage / 'excluded-requests.json'
    write_new(request, {'schema_version': 1, 'queries': [
        {'compilation': second.compilation('gson/main'), 'file': str(fixture.source_root / DESCRIPTOR),
         'position': {'encoding': 'utf8', 'offset': 0}}]})
    results['excluded-query'] = runner.failure('excluded-query', ['declaration', '--manifest', selected.manifest,
        '--requests', request], stage, 'invisible_source', 'module-info.java')
    overlay = stage / 'excluded-overlay.json'
    write_new(overlay, {'schema_version': 1, 'revision': 1, 'files': [
        {'compilation': second.compilation('gson/main'), 'file': str(fixture.source_root / DESCRIPTOR),
         'version': 1, 'text': (fixture.source_root / DESCRIPTOR).read_text()}]})
    results['excluded-overlay'] = runner.failure('excluded-overlay', ['parser-report', '--manifest', selected.manifest,
        '--overlays', overlay], stage, 'invalid_overlay', 'module-info.java')
    return results


def lsp_sessions(runner, historical, selected, generated, stage):
    results = {}
    for label, fixture in [('historical', historical), ('whole_root', selected), ('generated', generated)]:
        before = tree_state(fixture.source_root)
        receipt = {'command': [str(runner.native), 'lsp', '--manifest', str(fixture.manifest)],
                   'environment': {'PATH': ''}}
        try:
            with lsp.Session([runner.native, 'lsp', '--manifest', fixture.manifest], stage) as client:
                result = lsp.exercise(client, lsp.reviewed_case(fixture))
            same(len(result), 9, 'complete nine-stage LSP session')
            receipt['stages'] = result
            results[label] = result
        except Exception as error:
            receipt['exception'] = type(error).__name__ + ': ' + str(error)
            raise
        finally:
            receipt['source_tree_unchanged'] = tree_state(fixture.source_root) == before
            write_new(runner.receipts / ('lsp-' + label + '.json'), receipt)
            same(tree_state(fixture.source_root), before, label + ' LSP no source writes')
    for label in ['whole_root', 'generated']:
        same(results[label], results['historical'], label + ' actual LSP session preserves all nine wire stages')
    return results


def capture_baseline(runner, fixture, stage):
    model = selected_model(fixture)
    whole = copy.deepcopy(model)
    gson_main(whole).pop('source_exclusions')
    # The unchanged manual model uses paths relative to the fixture manifest.
    whole_path = fixture.manifest.with_name('baseline-whole-root.json')
    candidate_path = fixture.manifest.with_name('baseline-candidate.json')
    write_new(whole_path, whole)
    write_new(candidate_path, model)
    controls = {
        'whole_root_without_rule': runner.failure('baseline-whole-root', ['inspect', '--manifest', whole_path], stage,
                                                  'load_failed', 'module-info.java'),
        'candidate_field': runner.failure('baseline-candidate-field', ['inspect', '--manifest', candidate_path], stage,
                                          'invalid_json', 'source_exclusions'),
        'candidate_flag': runner.failure('baseline-candidate-flag', ['configure', *arguments(fixture, stage)], stage,
                                         'usage', '--exclude-source'),
    }
    return {'validation': 'baseline_red_verified_not_acceptance', 'controls': controls,
            'candidate_manual_manifest': model, 'candidate_configure_arguments': arguments(fixture, stage)}


def run(native, inputs, evidence, baseline=False):
    evidence.mkdir(parents=True, exist_ok=True)
    require(not any(evidence.iterdir()), 'evidence directory must be new and empty')
    report = {'validation': 'started', 'native_sha256': second.digest(native),
              'analysis_environment': {'PATH': '/nonexistent', 'java_executed': False},
              'automatic_build_import': False, 'generated_java': False}
    try:
        with tempfile.TemporaryDirectory(prefix='source-exclusions-') as temporary:
            stage = Path(temporary).resolve()
            fixture = second.prepare_fixture(inputs, stage / 'fixture')
            before, selected_paths = check_inventory(fixture)
            report.update(upstream=archive_evidence(fixture), source_sha256_before=before,
                          selected_source_paths=selected_paths)
            runner = Runner(native, evidence / 'receipts', fixture)
            if baseline:
                report.update(capture_baseline(runner, fixture, stage))
            else:
                selected = copy.copy(fixture)
                selected.manifest_data = selected_model(fixture)
                selected.manifest = fixture.manifest.with_name('whole-root.json')
                write_new(selected.manifest, selected.manifest_data)
                cwd = stage / 'invocation 中 space'
                cwd.mkdir()
                raw, normalized = run_configure(runner, fixture, selected.manifest_data, cwd, stage)
                relocated_path = stage / 'relocated/deeper/generated.json'
                probe_path = fixture.manifest.with_name('generated.json')
                write_new(relocated_path, raw)
                write_new(probe_path, raw)
                same(relocated_path.read_bytes(), probe_path.read_bytes(), 'relocated/probe manifest identical bytes')
                generated = copy.copy(selected)
                generated.manifest, generated.manifest_data = probe_path, strict_json(raw)
                relocated = copy.copy(generated)
                relocated.manifest = relocated_path
                original = runner.success('historical-inspect', 'inspect', fixture)
                manual = runner.success('whole-root-inspect', 'inspect', selected)
                actual = runner.success('relocated-inspect', 'inspect', relocated)
                compare_inspection(manual, original, selected, fixture, historical=True)
                compare_inspection(actual, manual, relocated, selected)
                details = {}
                for label, context in [('historical', fixture), ('manual', selected), ('generated', generated)]:
                    details[label] = runner.failure(label + '-details-limit', ['inspect', '--manifest', context.manifest,
                        '--details'], cwd, 'output_limit')
                same(details['generated'], details['historical'], 'default detail budget remains exact')
                same(details['manual'], details['historical'], 'manual detail refusal remains exact')
                binaries = binary_inventory(runner, fixture, selected, generated, stage / 'binary-projections')
                preserved = {}
                for label, context in [('historical', fixture), ('manual', selected), ('generated', generated)]:
                    preserved[label] = preservation(runner, context, label)
                same(preserved['manual'], preserved['historical'], 'all 41 current navigation/refusal rows and complete parser records')
                same(preserved['generated'], preserved['historical'], 'generated preserves all exact results')
                controls = failure_controls(runner, fixture, selected, stage)
                sessions = lsp_sessions(runner, fixture, selected, generated, stage)
                report.update(validation='passed', manual_manifest=selected.manifest_data,
                              generated_manifest=strict_json(raw), effective_manifest=normalized,
                              configure_arguments=arguments(fixture, cwd), configure_stdout_limit=MAX_MANIFEST,
                              identical_relocated_probe_manifest_bytes=True,
                              manifest_sha256=hashlib.sha256(raw).hexdigest(),
                              binary_only_projections=binaries, detail_refusals=details,
                              preservation=preserved['generated'], exact_historical_manual_generated_equality=True,
                              query_count=CURRENT_QUERY_COUNT, parser_report_count=166, parser_files_with_reports=44,
                              controls=controls, lsp_sessions=sessions)
            after, _ = check_inventory(fixture)
            same(after, before, 'all original source hashes after entire run')
            report.update(source_sha256_after=after, selected_source_count=211, extracted_source_count=226)
            same(second.digest(native), report['native_sha256'], 'frozen executable unchanged during run')
    except Exception as error:
        report.update(validation='failed', failure=type(error).__name__ + ': ' + str(error))
        write_new(evidence / 'failed.json', report)
        raise
    write_new(evidence / 'report.json', report)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('inputs_dir', type=Path)
    parser.add_argument('--native', type=Path, required=True)
    parser.add_argument('--evidence-dir', type=Path, required=True, help='new empty persistent receipts/report directory')
    parser.add_argument('--capture-baseline', action='store_true', help='strict baseline RED evidence, never acceptance')
    args = parser.parse_args()
    try:
        native = args.native.resolve(strict=True)
        require(native.is_file() and os.access(native, os.X_OK), 'native executable file')
        second.reject_symlinks(args.evidence_dir.absolute())
        report = run(native, args.inputs_dir.absolute(), args.evidence_dir.absolute(), args.capture_baseline)
        print(json.dumps(report, indent=2))
    except (ValueError, OSError, KeyError, TypeError, IndexError, ET.ParseError,
            zipfile.BadZipFile, RuntimeError, EOFError, subprocess.SubprocessError) as error:
        parser.exit(1, f'SOURCE EXCLUSIONS ACCEPTANCE FAILED: {error}\n')


if __name__ == '__main__':
    main()
