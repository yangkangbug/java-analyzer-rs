#!/usr/bin/env python3
"""Offline native configure acceptance on pinned Commons Lang and four-scope Gson.

No downloads, compiler, Java, build import or generated Java. Expected manifests
come from the pre-existing manual acceptance models, never configure output.
Every product subprocess runs with only PATH=/nonexistent in its environment.
"""
import argparse
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import stat
import subprocess
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parent.parent
MAX_MANIFEST = 1024 * 1024
GSON_TEMPLATE = 'gson/src/main/java-templates/com/google/gson/internal/GsonBuildConfig.java'
GSON_TEMPLATE_SHA256 = '43e03c2463b063f2383c7c445bdc1437b1d6b4ca1ea5c028584127f010236f54'
# Explicitly frozen public defaults, not learned from the candidate executable.
WORKSPACE_LIMITS = {'max_compilations': 128, 'max_dependency_edges': 1024,
                    'max_entries': 200000, 'max_input_bytes': 536870912,
                    'max_archive_expanded_bytes': 1073741824}
INPUT_LIMITS = {'max_entries': 100000, 'max_classpath_entries': 128,
                'max_file_bytes': 8388608, 'max_input_bytes': 268435456,
                'max_archive_expanded_bytes': 536870912, 'max_directory_depth': 64}
JAR_LIMITS = {'max_archive_bytes': 268435456, 'max_entries': 100000,
              'max_entry_bytes': 16777216, 'max_total_uncompressed_bytes': 536870912}
JMOD_LIMITS = {**JAR_LIMITS, 'max_entry_bytes': 67108864}


def load_tool(name):
    spec = importlib.util.spec_from_file_location('configure_' + name.replace('-', '_'),
                                                ROOT / 'scripts' / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


commons = load_tool('check-cli-acceptance')
second = load_tool('check-second-project')
strings = load_tool('check-string-call-acceptance')
AcceptanceError = second.AcceptanceError
require = second.require


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), allow_nan=False)


def same(actual, expected, label):
    # JSON type-sensitive equality: True must never stand in for integer 1.
    if type(actual) is set and type(expected) is set:
        actual, expected = sorted(actual), sorted(expected)
    require(canonical(actual) == canonical(expected),
            f'{label}: expected {str(expected)[:1200]!r}, got {str(actual)[:1200]!r}')


def strict_json(data):
    def nonfinite(value):
        raise AcceptanceError('non-finite JSON number: ' + value)
    return json.loads(data, object_pairs_hook=second.unique_json_object, parse_constant=nonfinite)


def effective_manifest(value, base, require_absolute=False):
    """Normalize path spelling only, never erase array order or duplicates."""
    require(type(value) is dict, 'manifest must be an object')
    required = {'schema_version', 'compilations'}
    require(set(value) == required or set(value) == required | {'limits'}, 'manifest fields')
    require(not require_absolute or 'limits' in value, 'explicit generated workspace defaults')
    require(type(value['schema_version']) is int and value['schema_version'] == 1,
            'manifest version must be integer 1')
    require(type(value['compilations']) is list and value['compilations'], 'manifest compilations')
    result = copy.deepcopy(value)
    result.setdefault('limits', copy.deepcopy(WORKSPACE_LIMITS))
    same(result['limits'], WORKSPACE_LIMITS, 'unchanged workspace defaults')
    fields = {'id', 'source_roots', 'dependencies', 'classpath', 'platform', 'target_release'}
    def identifier(item):
        require(type(item) is dict, 'compilation ID object')
        same(set(item), {'project', 'source_set'}, 'compilation ID fields')
        require(all(type(v) is str and v for v in item.values()), 'nonempty opaque ID strings')
    def path(value):
        require(type(value) is str and value and '\x00' not in value, 'path string')
        require(not require_absolute or Path(value).is_absolute(), 'configure path must be absolute')
        return os.path.abspath(os.path.join(str(base), value))
    for item in result['compilations']:
        require(type(item) is dict, 'compilation must be an object')
        defaults = {'limits': INPUT_LIMITS, 'jar_limits': JAR_LIMITS, 'jmod_limits': JMOD_LIMITS}
        require(fields <= set(item) <= fields | set(defaults), 'compilation fields')
        require(not require_absolute or set(item) == fields | set(defaults),
                'explicit generated compilation defaults')
        for field, expected in defaults.items():
            item.setdefault(field, copy.deepcopy(expected))
            same(item[field], expected, 'unchanged ' + field)
        identifier(item['id'])
        for field in ['source_roots', 'dependencies', 'classpath', 'platform']:
            require(type(item[field]) is list, field + ' must be an array')
        for dependency in item['dependencies']:
            identifier(dependency)
        item['source_roots'] = [path(p) for p in item['source_roots']]
        for field, kinds in [('classpath', {'jar', 'directory'}), ('platform', {'jmod', 'jar'})]:
            for entry in item[field]:
                require(type(entry) is dict, field + ' entry object')
                same(set(entry), {'kind', 'path'}, field + ' entry fields')
                require(entry['kind'] in kinds, field + ' artifact kind')
                entry['path'] = path(entry['path'])
        require(type(item['target_release']) is int and 0 < item['target_release'] <= 65535,
                'target_release integer')
    return result


def compare_manifests(generated, manual, manual_base):
    actual = effective_manifest(generated, manual_base, require_absolute=True)
    expected = effective_manifest(manual, manual_base)
    same(actual, expected, 'generated effective configuration versus independent manual model')
    return actual


def tree_state(root):
    """Observe source/config/cwd writes; reading atime is deliberately ignored."""
    result = {}
    for path in sorted(root.rglob('*')):
        info = path.lstat()
        require(not stat.S_ISLNK(info.st_mode), 'unexpected fixture symlink')
        result[path.relative_to(root).as_posix()] = [
            info.st_mode, info.st_size if path.is_file() else None,
            info.st_mtime_ns, second.digest(path) if path.is_file() else None]
    return result


def configure(native, arguments, cwd, protected_root, manual, manual_base):
    before = tree_state(protected_root)
    code, stdout, stderr = commons.run_bounded([native, 'configure', *arguments], cwd, 90, MAX_MANIFEST)
    same(tree_state(protected_root), before, 'configure wrote no fixture/product files')
    same(code, 0, 'configure exit; stderr=' + stderr[:2000].decode('utf-8', 'replace'))
    same(stderr.decode('utf-8'), '', 'configure successful stderr')
    require(len(stdout) <= MAX_MANIFEST, 'configure manifest output bound')
    require(stdout.endswith(b'\n'), 'configure stdout newline terminator')
    generated = strict_json(stdout)
    normalized = compare_manifests(generated, manual, manual_base)
    return stdout, normalized


def check_failure(code, stdout, stderr):
    same(code, 2, 'configure failure exit')
    same(stdout.decode('utf-8'), '', 'configure failure has no partial stdout')
    require(stderr.endswith(b'\n'), 'configure failure stderr newline terminator')
    error = strict_json(stderr)
    same(set(error), {'schema_version', 'error'}, 'error envelope fields')
    require(type(error['schema_version']) is int and error['schema_version'] == 1,
            'error schema integer')
    require(type(error['error']) is dict, 'structured error object')
    for key in ['code', 'message']:
        require(type(error['error'].get(key)) is str and error['error'][key], 'error ' + key)
    return error


def failure_controls(native, fixture, cwd, stage):
    root = os.path.relpath(fixture.source_root / 'gson/src/main/java/com', cwd)
    platform = os.path.relpath(fixture.inputs['java_base'], cwd)
    base = ['--compilation', 'gson', 'main', '--source-root', root, '--platform-jmod', platform]
    corrupt = stage / 'not-an-archive.jar'
    write_new(corrupt, b'not a ZIP archive\n')
    cases = {
        'missing_source_acknowledgment': ['--compilation', 'empty', 'main', '--no-platform'],
        'missing_platform_acknowledgment': ['--compilation', 'gson', 'main', '--source-root', root],
        'source_binary_only_conflict': [*base, '--binary-only'],
        'platform_no_platform_conflict': [*base, '--no-platform'],
        'missing_source_path': ['--compilation', 'missing', 'main', '--source-root',
                                'missing-source-root', '--no-platform'],
        'invalid_binary_archive': [*base, '--classpath-jar', os.path.relpath(corrupt, cwd)],
        'unknown_dependency': [*base, '--depends-on', 'missing', 'main'],
        'duplicate_compilation': [*base, *base],
    }
    results = {}
    before = tree_state(stage)
    for name, args in cases.items():
        code, stdout, stderr = commons.run_bounded([native, 'configure', *args], cwd, 90, MAX_MANIFEST)
        same(tree_state(stage), before, name + ': failure wrote no product files')
        results[name] = check_failure(code, stdout, stderr)
    return results


def inspect(native, cwd, manifest, details=True):
    args = [native, 'inspect', '--manifest', manifest]
    if details:
        args.append('--details')
    code, stdout, stderr = commons.run_bounded(args, cwd, 90, 20 * 1024 * 1024)
    same(code, 0, 'inspect exit; stderr=' + stderr[:2000].decode('utf-8', 'replace'))
    same(stderr.decode('utf-8'), '', 'inspect stderr')
    response = strict_json(stdout)
    same(response['schema_version'], 1, 'inspect response schema')
    same(response['command'], 'inspect', 'inspect command')
    same(response['snapshot']['manifest'], str(manifest.resolve()), 'inspect manifest identity')
    same(response['snapshot']['identity'], 'invocation_local', 'inspect snapshot identity boundary')
    require(type(response['snapshot']['id']) is str and response['snapshot']['id'], 'snapshot ID')
    return response


def detail_output_limit(native, cwd, manifest):
    result = commons.run_bounded([native, 'inspect', '--manifest', manifest, '--details'],
                                 cwd, 90, 20 * 1024 * 1024)
    error = check_failure(*result)
    same(error['error']['code'], 'output_limit', 'four-scope full detail budget refusal')
    return error


def binary_projection(manifest, index):
    projected = copy.deepcopy(manifest)
    projected['compilations'] = [projected['compilations'][index]]
    projected['compilations'][0]['source_roots'] = []
    projected['compilations'][0]['dependencies'] = []
    return projected


def compare_binary_projections(native, cwd, manual, generated, stage):
    # Full four-scope --details exceeds the unchanged 10000-item limit. These
    # explicitly labelled binary-only projections audit each scope's same binary
    # inputs/order separately. They are not a substitute full-graph source run.
    result = {}
    for index, compilation in enumerate(manual['compilations']):
        key = second.key_of(compilation['id'])
        manual_path = stage / (key.replace('/', '-') + '-manual.json')
        generated_path = stage / (key.replace('/', '-') + '-generated.json')
        write_new(manual_path, binary_projection(manual, index))
        write_new(generated_path, binary_projection(generated, index))
        original = inspect(native, cwd, manual_path)
        actual = inspect(native, cwd, generated_path)
        compare_inspection(actual, original, generated_path, manual_path)
        record = actual['result']['compilations'][0]
        same(record['source_paths'], [], 'binary projection excludes source inputs explicitly')
        same(record['visible_source_paths'], [], 'binary projection is not graph visibility evidence')
        same(record['class_count'], second.CLASS_COUNTS[key], key + ' fixed binary class inventory')
        same(len(record['class_origins']), second.CLASS_COUNTS[key], key + ' complete binary origins')
        result[key] = {'class_count': record['class_count'], 'exact_binary_origin_order_equal': True,
                       'class_origins_sha256': hashlib.sha256(canonical(record['class_origins']).encode()).hexdigest()}
    return result


def compare_inspection(generated, manual, generated_manifest, manual_manifest):
    """Compare every result field including exact binary origin order and edges."""
    def normalized(response, manifest):
        result = copy.deepcopy(response['result'])
        for record in result['compilations']:
            one = {'schema_version': 1, 'compilations': [record['configuration']]}
            record['configuration'] = effective_manifest(one, manifest.parent)['compilations'][0]
        return result
    same(generated['capabilities'], manual['capabilities'], 'unchanged inspect capabilities')
    same(normalized(generated, generated_manifest), normalized(manual, manual_manifest),
         'complete owned/visible inventories, binary order, edges and effective configuration')


def write_new(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('xb') as output:
        output.write(data if isinstance(data, bytes) else (json.dumps(data, indent=2) + '\n').encode())


def commons_arguments(source_root, platform, cwd):
    # Independently authored CLI input; never serialize the manual manifest into flags.
    return ['--compilation', 'commons-lang', 'main', '--source-root', os.path.relpath(source_root, cwd),
            '--platform-jmod', os.path.relpath(platform, cwd), '--target-release', '21']


def gson_arguments(fixture, cwd):
    def source(path):
        return os.path.relpath(fixture.source_root / path, cwd)
    def binary(name):
        return os.path.relpath(fixture.inputs[name], cwd)
    platform = ['--platform-jmod', binary('java_base'), '--platform-jmod', binary('java_sql'),
                '--target-release', '21']
    # Spell out the independently reviewed build model, not fixture.manifest_data.
    return [
        '--compilation', 'extras', 'main', '--source-root', source('extras/src/main/java'),
        '--depends-on', 'gson', 'main', '--classpath-jar', binary('jsr250'), *platform,
        '--compilation', 'extras', 'test', '--source-root', source('extras/src/test/java'),
        '--depends-on', 'extras', 'main', '--classpath-jar', binary('jsr250'),
        '--classpath-jar', binary('junit'), '--classpath-jar', binary('hamcrest'), *platform,
        '--compilation', 'gson', 'main', '--source-root', source('gson/src/main/java/com'), *platform,
        '--compilation', 'gson', 'test', '--source-root', source('gson/src/test/java'),
        '--depends-on', 'gson', 'main', '--classpath-jar', binary('junit'),
        '--classpath-jar', binary('hamcrest'), *platform,
    ]


def preserve_commons_parser(parser, source_root):
    # The historical helper reports inventory changes without failing. This gate
    # adds strict preservation for this configuration-only change.
    expected = {p: int(n) for p, n in commons.rows(commons.CORPUS / 'parser-errors.tsv', 2)}
    actual = {Path(row['path']).relative_to(source_root).as_posix(): len(row['reports'])
              for row in parser['files'] if row['reports']}
    same(actual, expected, 'unchanged historical Commons parser gap inventory')
    same(parser['report_count'], sum(expected.values()), 'unchanged Commons parser report count')
    same(parser['files_with_reports'], len(expected), 'unchanged Commons parser affected-file count')
    return actual


def run_commons(native, source, platform, stage):
    specs = json.loads((ROOT / 'scripts/acceptance-inputs.json').read_text())
    security = commons.input_helpers()
    security.reject_symlink_path(source)
    security.reject_symlink_path(platform)
    source, platform = source.resolve(strict=True), platform.resolve(strict=True)
    require(platform.is_file(), 'regular Commons platform input')
    same(commons.sha256(platform), specs['jdk']['jmod_sha256'], 'Commons platform SHA-256')
    source_root, cwd, notices = stage / 'sources', stage / 'invocation 中 space', stage / 'notices'
    for directory in (source_root, cwd, notices):
        directory.mkdir(parents=True)
    sources = commons.extract_sources(source, specs['sources'], source_root, notices)
    hashes = {p: hashlib.sha256(b).hexdigest() for p, b in sources.items()}
    queries, targets = commons.load_goldens(sources)
    manual_path = stage / 'config/manual.json'
    manual = {'schema_version': 1, 'compilations': [{
        'id': {'project': 'commons-lang', 'source_set': 'main'}, 'source_roots': ['../sources'],
        'dependencies': [], 'classpath': [], 'target_release': 21,
        'platform': [{'kind': 'jmod', 'path': os.path.relpath(platform, manual_path.parent)}],
    }]}
    write_new(manual_path, manual)
    request = stage / 'queries/requests.json'
    write_new(request, commons.requests_for(queries))
    arguments = commons_arguments(source_root, platform, cwd)
    raw, normalized = configure(native, arguments, cwd, stage, manual, manual_path.parent)
    generated_path = stage / 'relocated/deeper/generated.json'
    write_new(generated_path, raw)
    original = inspect(native, cwd, manual_path)
    commons.check_inspection(original, source_root, sources, commons.PLATFORM_CLASSES, platform)
    generated = inspect(native, cwd, generated_path)
    compare_inspection(generated, original, generated_path, manual_path)
    # Query files are manifest-relative in the historical contract. Relocation
    # is already proven above; use identical manifest bytes beside the manual
    # control so all original request documents and checker routines stay intact.
    probe_manifest = manual_path.with_name('generated.json')
    write_new(probe_manifest, raw)
    same(second.digest(probe_manifest), second.digest(generated_path), 'identical relocated manifest bytes')
    results = {}
    for name, manifest in [('manual', manual_path), ('generated', probe_manifest)]:
        parser = commons.run_cli(native, cwd, manifest, 'parser-report', 90, 20*1024*1024)
        commons.check_parser_report(parser, generated, source_root, sources)
        preserve_commons_parser(parser['result'], source_root)
        declarations = commons.run_cli(native, cwd, manifest, 'declaration', 90, 20*1024*1024, request)
        commons.check_declarations(declarations, queries, targets, source_root, platform, sources, effective_binary=True)
        results[name] = {'parser': parser['result'], 'declarations': declarations['result']}
    same(results['generated'], results['manual'], 'Commons exact parser and declaration preservation')
    same({p.relative_to(source_root).as_posix(): second.digest(p) for p in source_root.rglob('*.java')},
         hashes, 'all Commons source bytes unchanged')
    return {'manual_manifest': manual, 'generated_manifest': strict_json(raw),
            'effective_manifest': normalized, 'configure_arguments': arguments,
            'manifest_relocation_checked': True, 'relocated_and_probe_manifest_bytes_identical': True,
            'historical_request_documents_unchanged': True,
            'source_sha256_before': hashes, 'source_sha256_after': hashes,
            'source_count': len(sources), 'source_bytes': sum(map(len, sources.values())),
            'query_count': len(queries), 'target_count': len(targets),
            'parser_inventory': preserve_commons_parser(results['generated']['parser'], source_root),
            'parser_report_count': results['generated']['parser']['report_count'],
            'parser_files_with_reports': results['generated']['parser']['files_with_reports'],
            'result_sha256': hashlib.sha256(canonical(results['generated']).encode()).hexdigest(),
            'full_inspection_equal': True, 'exact_probe_results_equal': True,
            'source_archive_sha256': specs['sources']['sha256'],
            'platform_sha256': specs['jdk']['jmod_sha256']}


def missing_generated(rows, fixture):
    selected = []
    statuses = {'missing_generated_type': 'Unresolved', 'missing_generated_field': 'ReceiverNotBound'}
    for query, row in zip(fixture.plan, rows):
        if query['id'] in statuses:
            same(row['status'], statuses[query['id']], query['id'] + ' preserves exact refusal')
            same(row['targets'], [], query['id'] + ' has no invented targets')
            selected.append(query['id'])
    same(selected, ['missing_generated_type', 'missing_generated_field'], 'both generated-source gaps')
    # The pinned upstream archive includes a raw, excluded Java template. Keep
    # that original byte-for-byte; do not confuse it with generated source.
    found = {p.relative_to(fixture.source_root).as_posix()
             for p in fixture.source_root.rglob('GsonBuildConfig.java')}
    require(found <= {GSON_TEMPLATE}, 'must not add a generated GsonBuildConfig source')
    for path in found:
        same(second.digest(fixture.source_root / path), GSON_TEMPLATE_SHA256,
             'unchanged original excluded GsonBuildConfig template')
    require(not found.intersection(fixture.sources), 'GsonBuildConfig template must not be owned/loaded')


def run_gson(native, inputs, stage):
    fixture = second.prepare_fixture(inputs, stage / 'fixture')
    before = second.all_source_hashes(fixture)
    cwd = stage / 'invocation 中 space'
    cwd.mkdir()
    arguments = gson_arguments(fixture, cwd)
    raw, normalized = configure(native, arguments, cwd, stage, fixture.manifest_data, fixture.manifest.parent)
    generated_path = stage / 'relocated/deeper/generated.json'
    write_new(generated_path, raw)
    original_summary = second.successful(native, fixture, 'inspect', strict_json=True)
    second.check_response('inspect', original_summary, fixture)
    relocated_summary = inspect(native, cwd, generated_path, details=False)
    compare_inspection(relocated_summary, original_summary, generated_path, fixture.manifest)
    detail_refusal = detail_output_limit(native, cwd, fixture.manifest)
    same(detail_output_limit(native, cwd, generated_path), detail_refusal,
         'unchanged full four-scope detail budget refusal')
    binary_inventory = compare_binary_projections(native, cwd, fixture.manifest_data,
                                                   strict_json(raw), stage / 'binary-only-projections')
    probe_manifest = fixture.manifest.with_name('generated.json')
    write_new(probe_manifest, raw)
    same(second.digest(probe_manifest), second.digest(generated_path), 'identical relocated manifest bytes')
    generated_fixture = copy.copy(fixture)
    generated_fixture.manifest = probe_manifest
    # The expected config is derived from the prior manual model, never native output.
    generated_fixture.manifest_data = effective_manifest(fixture.manifest_data, fixture.manifest.parent)
    generated_summary = second.successful(native, generated_fixture, 'inspect', strict_json=True)
    # The existing checker expects manual config spelling. Normalize only paths
    # and public defaults before applying it; inventories remain untouched.
    normalized_summary = copy.deepcopy(generated_summary)
    for record in normalized_summary['result']['compilations']:
        one = {'schema_version': 1, 'compilations': [record['configuration']]}
        record['configuration'] = effective_manifest(one, generated_path.parent)['compilations'][0]
    second.check_response('inspect', normalized_summary, generated_fixture)
    # These unchanged routines run 8 string, 16 original, 3 reference, 5 local
    # probes plus the fixed complete parser record digest and constructor correction.
    original = strings.run(native, fixture)
    generated = strings.run(native, generated_fixture)
    same(generated, original, 'Gson complete original/reference/constructor/local/string preservation')
    missing_generated(generated['preservation']['original_16_queries'], fixture)
    after = second.all_source_hashes(fixture)
    same(after, before, 'all 226 extracted Gson Java hashes preserved')
    return {'manual_manifest': fixture.manifest_data, 'generated_manifest': strict_json(raw),
            'effective_manifest': normalized, 'configure_arguments': arguments,
            'manifest_relocation_checked': True, 'relocated_and_probe_manifest_bytes_identical': True,
            'historical_request_documents_unchanged': True,
            'full_graph_summary_equal': True, 'exact_probe_results_equal': True,
            'full_graph_details': {'status': 'output_limit', 'manual_generated_error_equal': True,
                                   'error': detail_refusal},
            'binary_only_projections': {'source_roots_and_dependencies_removed': True,
                                        'inventories': binary_inventory},
            'missing_generated_source': 'com.google.gson.internal.GsonBuildConfig',
            'missing_generated_outcomes': {'type': 'Unresolved', 'VERSION_field': 'ReceiverNotBound'},
            'source_sha256_before': before, 'source_sha256_after': after,
            'failure_controls': failure_controls(native, fixture, cwd, stage),
            'preservation': generated}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('sources_jar', type=Path)
    parser.add_argument('java_base_jmod', type=Path)
    parser.add_argument('gson_inputs_dir', type=Path)
    parser.add_argument('--native', type=Path, required=True, help='already built native product')
    parser.add_argument('--output', type=Path, help='new JSON report; never overwrite an old report')
    args = parser.parse_args()
    try:
        native = args.native.resolve(strict=True)
        require(native.is_file() and os.access(native, os.X_OK), 'native must be an executable file')
        if args.output:
            second.reject_symlinks(args.output.absolute())
            require(not args.output.exists(), 'output must not already exist')
        with tempfile.TemporaryDirectory(prefix='configure-acceptance-') as tmp:
            stage = Path(tmp).resolve()
            report = {'validation': 'passed', 'native_sha256': second.digest(native),
                      'analysis_environment': {'PATH': '/nonexistent', 'java_executed': False},
                      'build_import': False, 'generated_java': False,
                      'commons': run_commons(native, args.sources_jar.absolute(),
                                              args.java_base_jmod.absolute(), stage / 'commons'),
                      'gson': run_gson(native, args.gson_inputs_dir, stage / 'gson')}
            if args.output:
                write_new(args.output, report)
            print(json.dumps(report, indent=2))
            print('PASS native configure: pinned Commons + Gson; manual controls and all probe gaps preserved.')
    except (ValueError, OSError, KeyError, TypeError, IndexError, zipfile.BadZipFile,
            RuntimeError, EOFError, subprocess.SubprocessError) as error:
        parser.exit(1, f'CONFIGURE ACCEPTANCE FAILED: {error}\n')


if __name__ == '__main__':
    main()
