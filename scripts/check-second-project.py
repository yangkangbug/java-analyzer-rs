#!/usr/bin/env python3
"""Offline four-compilation Gson CLI acceptance. Never builds or executes Java.

Import only from trusted repository tooling. prepare_fixture(inputs_dir, workdir)
returns manifest/requests paths and portable metadata. check_response validates a
parsed actual product response outside any caller's measurement interval.
"""
import argparse
import copy
import hashlib
import importlib.util
import json
from pathlib import Path, PurePosixPath
import stat
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parent.parent
CORPUS = ROOT / 'examples' / 'second_project'
PINS = ROOT / 'scripts' / 'second-project-inputs.json'
SOURCE_COUNT = 211
SOURCE_BYTES = 1_484_012
QUERY_COUNT = 16
ROOTS = {
    'extras/main': ('extras/src/main/java', 8, 41_289),
    'extras/test': ('extras/src/test/java', 5, 27_736),
    'gson/main': ('gson/src/main/java/com', 79, 593_352),
    'gson/test': ('gson/src/test/java', 119, 821_635),
}
EDGES = {'extras/main': ['gson/main'], 'extras/test': ['extras/main'],
         'gson/main': [], 'gson/test': ['gson/main']}
BINARIES = {'extras/main': ['jsr250'], 'extras/test': ['jsr250', 'junit', 'hamcrest'],
            'gson/main': [], 'gson/test': ['junit', 'hamcrest']}
CLASS_COUNTS = {'extras/main': 7704, 'extras/test': 8099,
                'gson/main': 7693, 'gson/test': 8088}
PLATFORM = ['java_base', 'java_sql']


class AcceptanceError(ValueError):
    pass


def require(ok, message):
    if not ok:
        raise AcceptanceError(message)


def same(actual, expected, label):
    require(actual == expected, f'{label}: expected {expected!r}, got {actual!r}')


def compilation(key):
    project, source_set = key.split('/')
    return {'project': project, 'source_set': source_set}


def key_of(value):
    return value['project'] + '/' + value['source_set']


def utf16(data, byte):
    return len(data[:byte].decode('utf-8').encode('utf-16-le')) // 2


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def reject_symlinks(path):
    for parent in [path, *path.parents]:
        require(not parent.is_symlink(), f'symlink input/ancestor: {parent}')


def safe_name(name):
    require(name and not name.startswith('/') and '\\' not in name and ':' not in name,
            f'unsafe archive path: {name!r}')
    parts = name.rstrip('/').split('/')
    require(all(part not in ('', '.', '..') for part in parts),
            f'unsafe archive component: {name!r}')
    return '/'.join(parts)


class Fixture:
    """Owned temporary fixture; workdir lifetime is controlled by the caller."""
    pass


def extract_sources(archive_path, spec, destination):
    """Validate every archive entry; only Java data and LICENSE are materialized."""
    # Reuse the setup validator before writing any source: NUL names, type and
    # compression mismatches, collisions, all CRCs and notices are checked first.
    security_helpers().check_archive(archive_path, spec, destination.parent / 'notices')
    seen, sources, total = set(), {}, 0
    with zipfile.ZipFile(archive_path) as archive:
        require(len(archive.infolist()) <= spec['max_entries'], 'archive entry limit')
        for entry in archive.infolist():
            name = safe_name(entry.filename)
            require(name not in seen, f'duplicate archive entry: {name}')
            seen.add(name)
            require(name == spec['root'] or name.startswith(spec['root'] + '/'),
                    f'unexpected archive root: {name}')
            mode = entry.external_attr >> 16
            kind = stat.S_IFMT(mode)
            require(kind in (0, stat.S_IFREG, stat.S_IFDIR), f'special archive entry: {name}')
            require(not entry.flag_bits & 1, f'encrypted archive entry: {name}')
            require(entry.file_size <= spec['max_file'], f'archive member limit: {name}')
            total += entry.file_size
            require(total <= spec['max_expanded'], 'archive expansion limit')
            if entry.is_dir():
                require(entry.file_size == 0, f'nonempty archive directory: {name}')
                continue
            with archive.open(entry) as stream:
                data = stream.read(spec['max_file'] + 1)
            same(len(data), entry.file_size, f'archive size: {name}')
            relative = name[len(spec['root']) + 1:]
            if not (relative.endswith('.java') or relative == 'LICENSE'):
                continue
            data.decode('utf-8')
            target = destination.joinpath(*PurePosixPath(relative).parts)
            target.parent.mkdir(parents=True, exist_ok=True)
            with target.open('xb') as stream:
                stream.write(data)
            if relative.endswith('.java'):
                sources[relative] = data
    require((destination / 'LICENSE').is_file(), 'missing Gson LICENSE')
    same(len(sources), 226, 'complete upstream Java inventory, including exclusions')
    return sources


def security_helpers():
    spec = importlib.util.spec_from_file_location('second_project_input_security',
                                                ROOT / 'scripts' / 'fetch-second-project-inputs.py')
    helper = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helper)
    return helper


def prepare_fixture(inputs_dir, workdir):
    """Prepare only fixed verified local data. No fetch, Java or upstream scripts."""
    inputs_dir, workdir = Path(inputs_dir).absolute(), Path(workdir).absolute()
    reject_symlinks(inputs_dir)
    reject_symlinks(workdir)
    workdir.mkdir(parents=True, exist_ok=True)
    require(not any(workdir.iterdir()), f'fixture directory must be empty: {workdir}')
    spec = json.loads(PINS.read_text(encoding='utf-8'))
    security_helpers().validate_manifest(spec)
    same(spec['format_version'], 1, 'pin version')
    fixture = Fixture()
    fixture.workdir = workdir
    fixture.inputs = {}
    hashes = {}
    for name in ['sources', 'junit', 'hamcrest', 'jsr250', *PLATFORM]:
        pin = spec['artifacts'][name]
        path = inputs_dir / pin['filename']
        reject_symlinks(path)
        require(path.is_file(), f'input is not a regular file: {path}')
        same(path.stat().st_size, pin['size'], f'input size: {name}')
        checksum = digest(path)
        same(checksum, pin['sha256'], f'input SHA-256: {name}')
        fixture.inputs[name] = path.resolve()
        hashes[name] = {'filename': pin['filename'], 'size': pin['size'], 'sha256': checksum}
    fixture.source_root = workdir / 'sources'
    fixture.source_root.mkdir()
    all_sources = extract_sources(fixture.inputs['sources'], spec['artifacts']['sources'],
                                  fixture.source_root)
    fixture.sources, fixture.owned, fixture.owner = {}, {}, {}
    for key, (root, count, size) in ROOTS.items():
        selected = {name: data for name, data in all_sources.items()
                    if name.startswith(root + '/')}
        same(len(selected), count, f'{key} source count')
        same(sum(map(len, selected.values())), size, f'{key} source bytes')
        fixture.owned[key] = sorted(str(fixture.source_root / name) for name in selected)
        for name, data in selected.items():
            require(name not in fixture.sources, f'duplicate source ownership: {name}')
            fixture.sources[name], fixture.owner[name] = data, key
    same(len(fixture.sources), SOURCE_COUNT, 'selected source count')
    same(sum(map(len, fixture.sources.values())), SOURCE_BYTES, 'selected source bytes')
    fixture.visible = {}
    for key in ROOTS:
        visited, pending = set(), [key]
        while pending:
            item = pending.pop()
            if item not in visited:
                visited.add(item)
                pending.extend(EDGES[item])
        fixture.visible[key] = sorted(p for item in visited for p in fixture.owned[item])
    comps = []
    for key, (root, _, _) in ROOTS.items():
        comps.append({'id': compilation(key), 'source_roots': ['sources/' + root],
                      'dependencies': [compilation(item) for item in EDGES[key]],
                      'classpath': [{'kind': 'jar', 'path': str(fixture.inputs[item])}
                                    for item in BINARIES[key]],
                      'platform': [{'kind': 'jmod', 'path': str(fixture.inputs[item])}
                                   for item in PLATFORM], 'target_release': 21})
    fixture.manifest_data = {'schema_version': 1, 'compilations': comps}
    fixture.manifest = workdir / 'project.json'
    fixture.manifest.write_text(json.dumps(fixture.manifest_data, indent=2) + '\n')
    fixture.plan = json.loads((CORPUS / 'query-plan.json').read_text())['queries']
    fixture.expected = json.loads((CORPUS / 'expected.json').read_text())
    same(len(fixture.plan), QUERY_COUNT, 'fixed query plan count')
    same(len(fixture.expected), QUERY_COUNT, 'fixed expectations count')
    fixture.requests_data = {'schema_version': 1, 'queries': []}
    for query, expected in zip(fixture.plan, fixture.expected):
        data = fixture.sources[query['file']]
        same(data[query['byte']:query['byte'] + len(query['spelling'])],
             query['spelling'].encode(), f"{query['id']} query spelling")
        fixture.requests_data['queries'].append({
            'compilation': query['compilation'], 'file': 'sources/' + query['file'],
            'position': {'encoding': 'utf8', 'offset': query['byte']}})
        for target in expected['targets']:
            if target['origin'] == 'source':
                data = fixture.sources[target['path']]
                same(data[target['start']:target['end']], target['name'].encode(),
                     f"{query['id']} exact independently reviewed declaration")
    fixture.requests = workdir / 'requests.json'
    fixture.requests.write_text(json.dumps(fixture.requests_data, indent=2) + '\n')
    fixture.metadata = {
        'project': 'Gson core/extras 2.10.1 manual source-set model',
        'commit': '2ce6a6106c22bf0cce2e39f68b0da2f117d6e66b',
        'source_count': SOURCE_COUNT, 'source_bytes': SOURCE_BYTES,
        'query_count': QUERY_COUNT, 'inputs': hashes,
        'automatic_build_import': False, 'upstream_build_or_tests_executed': False,
        'missing_generated_source': 'com.google.gson.internal.GsonBuildConfig',
        'jpms_supported': False,
    }
    return fixture


def source_target(target, fixture):
    data = fixture.sources[target['path']]
    return {'origin': 'source', 'source_available': True,
            'path': str(fixture.source_root / target['path']),
            'compilation': compilation(fixture.owner[target['path']]),
            'kind': target['kind'], 'name': target['name'], 'owner': target['owner'],
            'range': {'start_byte': target['start'], 'end_byte': target['end'],
                      'start_utf16': utf16(data, target['start']),
                      'end_utf16': utf16(data, target['end'])}}


def expected_target(target, fixture):
    if target['origin'] == 'source':
        return source_target(target, fixture)
    artifact = target['artifact']
    contexts = [key for key in ROOTS if artifact in PLATFORM + BINARIES[key]]
    entry = ('classes/' if artifact in PLATFORM else '') + target['binary_name'] + '.class'
    return {'origin': 'binary', 'source_available': False,
            'path': str(fixture.inputs[artifact]) + '!/' + entry,
            'kind': target['kind'], 'name': target['name'], 'owner': target['owner'],
            'binary_name': target['binary_name'],
            'loaded_contexts': [compilation(key) for key in sorted(contexts)],
            'linking_context': None, 'identity_precision': 'origin_kind_owner_name_only'}


def check_envelope(command, response, fixture, manifest=None):
    same(response['schema_version'], 1, 'response schema')
    same(response['command'], command, 'response command')
    snapshot = response['snapshot']
    same(snapshot['identity'], 'invocation_local', 'snapshot identity boundary')
    require(isinstance(snapshot['id'], str) and snapshot['id'], 'snapshot ID')
    same(snapshot['manifest'], str((manifest or fixture.manifest).resolve()), 'snapshot manifest')
    caps = response['capabilities']
    same(caps['runtime'], 'native_rust_no_jvm', 'native runtime claim')
    same(caps['build_tool_import'], False, 'manual build model')
    same(caps['semantic_diagnostics'], False, 'semantic diagnostics unsupported')


def check_query_row(row, query, expected, fixture, index):
    same(row['query_index'], index, 'query index')
    same(row['compilation'], query['compilation'], 'requested compilation')
    same(row['source_compilation'], compilation(fixture.owner[query['file']]), 'source owner')
    same(row['file'], str(fixture.source_root / query['file']), 'query path')
    same(row['position'], {'encoding': 'utf8', 'offset': query['byte']}, 'query position')
    same(row['byte_offset'], query['byte'], 'query byte offset')
    same(row['status'], expected['status'], f"{query['id']} status")
    same(row['targets'], [expected_target(t, fixture) for t in expected['targets']],
         f"{query['id']} exact target/candidate records")


def check_parser(result, fixture):
    classification = 'parser_limit_or_invalid_syntax'
    same(result['classification'], classification, 'parser classification')
    same(result['java_semantic_diagnostics'], False, 'not semantic diagnostics')
    same(result['scope'], 'owned_sources_only', 'parser ownership scope')
    expected_paths = sorted((key, path) for key in ROOTS for path in fixture.owned[key])
    files = result['files']
    same([(key_of(f['compilation']), f['path']) for f in files], expected_paths,
         'every owned parser source exactly once')
    affected, count = 0, 0
    for file in files:
        relative = Path(file['path']).relative_to(fixture.source_root).as_posix()
        data = fixture.sources[relative]
        affected += bool(file['reports'])
        count += len(file['reports'])
        for report in file['reports']:
            same(report['classification'], classification, 'parser gap classification')
            require(isinstance(report['message'], str) and report['message'], 'parser message')
            extent = report['range']
            start, end = extent['start_byte'], extent['end_byte']
            require(0 <= start <= end <= len(data), f'parser range: {relative}')
            same(extent['start_utf16'], utf16(data, start), 'parser UTF16 start')
            same(extent['end_utf16'], utf16(data, end), 'parser UTF16 end')
    same(result['source_count'], SOURCE_COUNT, 'parser source count')
    same(result['files_with_reports'], affected, 'parser affected count')
    same(result['report_count'], count, 'parser total count')


def check_response(command, response, fixture):
    """Validate full actual response, not just an exit code or a resolution ratio."""
    check_envelope(command, response, fixture)
    result = response['result']
    if command == 'inspect':
        same(result['detail_mode'], 'summary', 'summary inspection')
        same([key_of(c['id']) for c in result['compilations']], sorted(ROOTS), 'compilation IDs')
        for record, configured in zip(result['compilations'], fixture.manifest_data['compilations']):
            key = key_of(record['id'])
            same(record['source_paths'], fixture.owned[key], key + ' owned sources')
            same(record['visible_source_paths'], fixture.visible[key], key + ' visible sources')
            same(record['class_count'], CLASS_COUNTS[key], key + ' binary count')
            same(record['shadowed_classes'], [], key + ' no duplicate binaries')
            require('class_origins' not in record, 'summary must not emit full origins')
            for field in ['id', 'source_roots', 'dependencies', 'classpath', 'platform', 'target_release']:
                same(record['configuration'][field], configured[field], key + ' config ' + field)
    elif command == 'declaration':
        same(result['index_builds'], 1, 'batch reuses one index')
        same(len(result['queries']), QUERY_COUNT, 'returned query count')
        for i, (row, query, expected) in enumerate(zip(result['queries'], fixture.plan, fixture.expected)):
            check_query_row(row, query, expected, fixture, i)
    elif command == 'parser-report':
        check_parser(result, fixture)
    else:
        raise AcceptanceError(f'unsupported checker command: {command}')


def bounded_helper():
    spec = importlib.util.spec_from_file_location('first_project_cli_acceptance',
                                                ROOT / 'scripts' / 'check-cli-acceptance.py')
    helper = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helper)
    return helper


def invoke(native, fixture, command, manifest=None, requests=None):
    args = [str(native), command, '--manifest', str(manifest or fixture.manifest)]
    if command == 'declaration':
        args += ['--requests', str(requests or fixture.requests)]
    # Parent is a private temporary directory different from the manifest directory.
    code, stdout, stderr = bounded_helper().run_bounded(args, fixture.workdir.parent, 40, 20*1024*1024)
    return code, stdout, stderr


def successful(native, fixture, command, manifest=None, requests=None):
    code, stdout, stderr = invoke(native, fixture, command, manifest, requests)
    same(code, 0, f'{command} exit; stderr={stderr[:2000]!r}')
    same(stderr, b'', command + ' stderr')
    response = json.loads(stdout)
    check_envelope(command, response, fixture, manifest)
    return response


def document(fixture, name, data):
    path = fixture.workdir / name
    path.write_text(json.dumps(data, indent=2) + '\n')
    return path


def expect_failure(native, fixture, command, code_name, manifest=None, requests=None):
    code, stdout, stderr = invoke(native, fixture, command, manifest, requests)
    require(code != 0, 'negative control unexpectedly succeeded')
    same(stdout, b'', 'failure has no partial stdout')
    error = json.loads(stderr)
    same(error['error']['code'], code_name, 'negative-control error code')
    return error


def run_controls(native, fixture):
    completed = []
    # File admission must respect source-set boundaries, before preparing queries.
    for name, context, index in [('main-test-isolation', 'gson/main', 3),
                                 ('reverse-module-isolation', 'gson/main', 0),
                                 ('test-helper-isolation', 'extras/test', 4)]:
        q = copy.deepcopy(fixture.requests_data['queries'][index])
        q['compilation'] = compilation(context)
        req = document(fixture, name + '.json', {'schema_version': 1, 'queries': [q]})
        expect_failure(native, fixture, 'declaration', 'invisible_source', requests=req)
        completed.append(name)
    # Removing each required origin cannot leak a declaration from other contexts.
    for name, changed_context, field, index in [
        ('missing-source-edge', 'extras/main', 'dependencies', 1),
        ('missing-junit', 'gson/test', 'junit', 8),
        ('missing-annotation', 'extras/main', 'jsr250', 9),
        ('missing-sql', 'gson/main', 'java_sql', 10),
    ]:
        model = copy.deepcopy(fixture.manifest_data)
        c = next(c for c in model['compilations'] if key_of(c['id']) == changed_context)
        if field == 'dependencies':
            c[field] = []
        else:
            which = 'platform' if field in PLATFORM else 'classpath'
            c[which] = [item for item in c[which] if item['path'] != str(fixture.inputs[field])]
        manifest = document(fixture, name + '-project.json', model)
        req = document(fixture, name + '-requests.json', {'schema_version': 1,
                       'queries': [fixture.requests_data['queries'][index]]})
        response = successful(native, fixture, 'declaration', manifest, req)
        same(response['result']['index_builds'], 1, name + ' index count')
        same(len(response['result']['queries']), 1, name + ' query count')
        check_query_row(response['result']['queries'][0], fixture.plan[index],
                        {'status': 'Unresolved', 'targets': []}, fixture, 0)
        completed.append(name)
    # Missing inputs and unsupported module-info must be whole-load failures.
    for name, mutate in [
        ('missing-root', lambda c: c.update(source_roots=['does-not-exist'])),
        ('missing-binary', lambda c: c.update(classpath=[{'kind': 'jar', 'path': 'absent.jar'}])),
        ('jpms-refusal', lambda c: c.update(source_roots=['sources/gson/src/main/java'])),
    ]:
        model = copy.deepcopy(fixture.manifest_data)
        c = next(c for c in model['compilations'] if key_of(c['id']) == 'gson/main')
        mutate(c)
        manifest = document(fixture, name + '.json', model)
        error = expect_failure(native, fixture, 'inspect', 'load_failed', manifest=manifest)
        text = json.dumps(error)
        require(('module-info' if name == 'jpms-refusal' else
                 'does-not-exist' if name == 'missing-root' else 'absent.jar') in text,
                name + ' offending path absent')
        completed.append(name)
    # Rename only a disposable source field and its references, retaining exact goldens.
    index = 6
    query = copy.deepcopy(fixture.plan[index])
    expected = copy.deepcopy(fixture.expected[index])
    path = fixture.source_root / query['file']
    original = path.read_bytes()
    renamed = original.replace(b'bread', b'loaf')
    old_sources = fixture.sources[query['file']]
    try:
        path.write_bytes(renamed)
        fixture.sources[query['file']] = renamed
        query['byte'] = renamed.index(b'this.loaf = loaf;') + len(b'this.')
        query['spelling'] = 'loaf'
        target = expected['targets'][0]
        target['name'] = 'loaf'
        target['start'] = renamed.index(b'public String loaf;') + len(b'public String ')
        target['end'] = target['start'] + 4
        req = document(fixture, 'renamed-requests.json', {'schema_version': 1, 'queries': [{
            'compilation': query['compilation'], 'file': 'sources/' + query['file'],
            'position': {'encoding': 'utf8', 'offset': query['byte']}}]})
        result = successful(native, fixture, 'declaration', requests=req)['result']
        same(result['index_builds'], 1, 'renamed index')
        same(len(result['queries']), 1, 'renamed query count')
        check_query_row(result['queries'][0], query, expected, fixture, 0)
        completed.append('source-rename-fresh-load')
        # Original test file has no gaps; deletion introduces an incomplete buffer.
        malformed = original[:-2]
        path.write_bytes(malformed)
        fixture.sources[query['file']] = malformed
        report = successful(native, fixture, 'parser-report')
        check_response('parser-report', report, fixture)
        selected = next(f for f in report['result']['files'] if f['path'] == str(path))
        require(selected['reports'], 'malformed source was not reported')
        completed.append('malformed-source-report')
    finally:
        path.write_bytes(original)
        fixture.sources[query['file']] = old_sources
    # Original success after failed loads and source restore, not persistent rollback.
    restored = successful(native, fixture, 'declaration')
    check_response('declaration', restored, fixture)
    completed.append('source-restore-and-failed-load-recovery')
    corrupted = copy.deepcopy(restored)
    corrupted['result']['queries'][0]['targets'][0]['range']['start_byte'] += 1
    try:
        check_response('declaration', corrupted, fixture)
    except AcceptanceError:
        completed.append('wrong-target-checker-control')
    else:
        raise AcceptanceError('checker accepted corrupted exact target')
    return completed


def run_acceptance(native, fixture):
    results = {}
    for command in ['inspect', 'declaration', 'parser-report']:
        response = successful(native, fixture, command)
        check_response(command, response, fixture)
        results[command] = response
    parser = results['parser-report']['result']
    inventory = {str(Path(f['path']).relative_to(fixture.source_root)): len(f['reports'])
                 for f in parser['files'] if f['reports']}
    baseline = {}
    for line in (CORPUS / 'parser-errors.tsv').read_text().splitlines():
        if line and not line.startswith('#'):
            path, count = line.split('\t')
            baseline[path] = int(count)
    # Parser changes are reported, not rejected as a fixed-count success metric.
    changes = {path: {'baseline': baseline.get(path, 0), 'actual': inventory.get(path, 0)}
               for path in sorted(set(baseline) | set(inventory))
               if baseline.get(path, 0) != inventory.get(path, 0)}
    controls = run_controls(native, fixture)
    return {'metadata': fixture.metadata, 'query_statuses': [q['status'] for q in
            results['declaration']['result']['queries']], 'parser_files_with_reports':
            parser['files_with_reports'], 'parser_report_count': parser['report_count'],
            'parser_inventory': inventory, 'parser_baseline_changes': changes,
            'controls': controls, 'automatic_build_import': False,
            'persistent_reload': False, 'upstream_tests_executed': False}


def local_type_probes(fixture):
    """Independent additive positions; never replace the original 16 goldens."""
    spec = json.loads((CORPUS / 'local-type-probes.json').read_text())
    probes = spec['queries']
    same(len(probes), 5, 'local-type probe count')
    same(len({q['id'] for q in probes}), 5, 'unique local-type probes')
    for query in probes:
        data = fixture.sources[query['file']]
        same(hashlib.sha256(data).hexdigest(), spec['source_sha256'], 'unaltered GsonTest bytes')
        same((fixture.source_root / query['file']).read_bytes(), data, 'extracted source bytes')
        same(data[query['byte']:query['byte'] + len(query['spelling'])],
             query['spelling'].encode(), 'local-type use spelling')
        for target in query['expected']['targets']:
            same(data[target['start']:target['end']], target['name'].encode(), 'local declaration token')
            same((target['kind'], target['owner']), ('Local', ''), 'local target convention')
    return probes


def check_local_type_response(response, probes, fixture):
    actual = response['result']['queries']
    same(len(actual), len(probes), 'local-type output completeness')
    for index, (row, query) in enumerate(zip(actual, probes)):
        check_query_row(row, query, query['expected'], fixture, index)


def run_local_type_acceptance(native, fixture, capture_baseline=False):
    probes = local_type_probes(fixture)
    request = document(fixture, 'local-type-requests.json', {'schema_version': 1, 'queries': [
        {'compilation': q['compilation'], 'file': 'sources/' + q['file'],
         'position': {'encoding': 'utf8', 'offset': q['byte']}} for q in probes]})
    response = successful(native, fixture, 'declaration', requests=request)
    parser = successful(native, fixture, 'parser-report')['result']
    check_parser(parser, fixture)
    failure = None
    try:
        check_local_type_response(response, probes, fixture)
    except AcceptanceError as error:
        if not capture_baseline:
            raise
        failure = str(error)
    # Hash every extracted source, including the 15 unselected paths. No source
    # is rewritten or generated and no upstream Java compilation is attempted.
    extracted = {p.relative_to(fixture.source_root).as_posix(): digest(p)
                 for p in fixture.source_root.rglob('*.java')}
    same(len(extracted), 226, 'complete extracted Java path inventory')
    portable_queries = json.loads(json.dumps(response['result']['queries']).replace(
        str(fixture.source_root) + '/', ''))
    if failure:
        failure = failure.replace(str(fixture.source_root) + '/', '')
    return {'validation': 'baseline_failure' if failure else 'passed', 'failure': failure,
            'queries': portable_queries, 'metadata': fixture.metadata,
            'source_sha256': extracted,
            'parser_inventory': {str(Path(f['path']).relative_to(fixture.source_root)): len(f['reports'])
                                 for f in parser['files']},
            'parser_report_count': parser['report_count'],
            'javac_real_gson_equivalence': False, 'upstream_code_executed': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('inputs_dir', type=Path)
    parser.add_argument('--native', type=Path, default=ROOT / 'target/release/java-analyzer')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--local-types', action='store_true', help='run independent additive local-type probes')
    parser.add_argument('--capture-baseline', action='store_true', help='retain failing local-type evidence without treating it as acceptance')
    args = parser.parse_args()
    native = args.native.resolve()
    require(native.is_file(), f'build the CLI first: {native}')
    with tempfile.TemporaryDirectory(prefix='second-project-') as tmp:
        fixture = prepare_fixture(args.inputs_dir, Path(tmp) / 'fixture')
        require(not args.capture_baseline or args.local_types, 'baseline capture requires --local-types')
        result = (run_local_type_acceptance(native, fixture, args.capture_baseline)
                  if args.local_types else run_acceptance(native, fixture))
        text = json.dumps(result, indent=2) + '\n'
        if args.output:
            reject_symlinks(args.output.absolute())
            # Refuse replacement, including symlink targets. No partial report is
            # written before every acceptance check has succeeded.
            with args.output.open('x', encoding='utf-8') as stream:
                stream.write(text)
        print(text, end='')


if __name__ == '__main__':
    main()
