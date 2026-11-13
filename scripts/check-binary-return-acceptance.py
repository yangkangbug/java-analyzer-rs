#!/usr/bin/env python3
"""Pinned Commons binary-return probes using the shipped CLI only; no Java/network/build."""
import argparse
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import tempfile

spec = importlib.util.spec_from_file_location('binary_return_commons_prior', Path(__file__).with_name('check-cli-acceptance.py'))
a = importlib.util.module_from_spec(spec)
spec.loader.exec_module(a)
ROOT = a.ROOT
PLAN = a.CORPUS / 'binary-return-probes.json'
PLAN_SHA256 = '4b576e50c25830588d1540d7e090ec527fb7a74d5e4f3b7afbdd45f3720e46c4'
BASELINE_SHA256 = 'af124cee9bfb662152b7ec32fa0b2d9939dee3d081a44b6d75893fdced4400a1'
BASELINE_REVISION = '5694f15fc9c4e82328d0713868f032ba9fff210c'
S = 'org/apache/commons/lang3/StringUtils.java'
E = 'org/apache/commons/lang3/text/ExtendedMessageFormat.java'
X = 'org/apache/commons/lang3/exception/ExceptionUtils.java'
# Reviewed source syntax and public String API identities, before candidate execution.
PROBES = [
    ('concat_receiver', S, 'padStr.substring(0, pads).concat(str)', 'concat', '(Ljava/lang/String;)Ljava/lang/String;', True),
    ('concat_actual', S, 'str.concat(padStr.substring(0, pads))', 'concat', '(Ljava/lang/String;)Ljava/lang/String;', True),
    ('substring_primitive_actual', S, 'str.substring(remove.length())', 'substring', '(I)Ljava/lang/String;', True),
    ('trim_receiver', E, 'desc.substring(0, i).trim()', 'trim', '()Ljava/lang/String;', True),
    ('trim_middle', X, 'token.substring(0, at).trim().isEmpty()', 'trim', '()Ljava/lang/String;', True),
    ('default_interface_refusal', X, 'token.substring(0, at).trim().isEmpty()', 'isEmpty', None, False),
    ('arithmetic_refusal', E, 'desc.substring(i + 1).trim()', 'trim', None, False),
    ('inner_preservation', S, 'padStr.substring(0, pads).concat(str)', 'substring', '(II)Ljava/lang/String;', False),
]


def exact(actual, expected, label):
    a.same(json.dumps(actual, sort_keys=True), json.dumps(expected, sort_keys=True), label)


def query_plan(sources):
    result = []
    for identity, path, expression, name, descriptor, positive in PROBES:
        data = sources[path]
        expression = expression.encode()
        a.same(data.count(expression), 1, identity + ' unique source expression')
        start = data.index(expression)
        selector = b'.' + name.encode() + b'('
        a.same(expression.count(selector), 1, identity + ' unique selector')
        offset = start + expression.index(selector) + 1
        a.same(data[offset:offset + len(name)], name.encode(), identity + ' selector bytes')
        result.append({'id': identity, 'file': path, 'byte_offset': offset, 'name': name,
                       'descriptor': descriptor, 'newly_supported': positive})
    return result


def normalize(value, source_root, platform):
    if isinstance(value, dict):
        return {key: normalize(v, source_root, platform) for key, v in value.items()}
    if isinstance(value, list):
        return [normalize(v, source_root, platform) for v in value]
    if isinstance(value, str):
        return value.replace(str(source_root) + '/', '@SOURCE@/').replace(str(platform), '@PLATFORM@')
    return value


def target(name, descriptor, detailed):
    result = {'kind': 'Method', 'name': name, 'owner': 'java.lang.String', 'origin': 'binary',
              'source_available': False, 'path': '@PLATFORM@!/classes/java/lang/String.class',
              'binary_name': 'java/lang/String', 'loaded_contexts': [a.COMPILATION],
              'linking_context': a.COMPILATION if detailed else None,
              'identity_precision': 'origin_binary_name_descriptor_linking_context' if detailed else 'origin_kind_owner_name_only'}
    if detailed:
        result['descriptor'] = descriptor
    return result


def forecast(baseline, queries, detailed):
    result = copy.deepcopy(baseline)
    a.same(len(result['queries']), len(queries), 'complete baseline query count')
    for metadata, row in zip(queries, result['queries']):
        if metadata['newly_supported']:
            a.require(row['status'] != 'Resolved', 'new prediction was not refused in baseline')
            row['status'] = 'Resolved'
            row['targets'] = [target(metadata['name'], metadata['descriptor'], detailed)]
        elif metadata['id'] == 'inner_preservation':
            a.same(row['status'], 'Resolved', 'existing inner selection')
            exact(row['targets'], [target(metadata['name'], metadata['descriptor'], detailed)], 'existing inner exact target')
        else:
            a.require(row['status'] != 'Resolved', 'conservative control baseline must refuse')
    return result


def load_plan():
    a.input_helpers().reject_symlink_path(PLAN)
    a.same(a.sha256(PLAN), PLAN_SHA256, 'frozen binary-return corpus plan')
    plan = json.loads(PLAN.read_text())
    a.same(plan['baseline_native_sha256'], BASELINE_SHA256, 'frozen baseline binary')
    a.same(plan['baseline_revision'], BASELINE_REVISION, 'frozen baseline revision')
    for key, detailed in [('default', False), ('details', True)]:
        exact(plan['candidate'][key], forecast(plan['baseline'][key], plan['queries'], detailed), 'only five supported projections')
    return plan


def controls(wanted):
    mutants = []
    def add(change):
        value = copy.deepcopy(wanted); change(value); mutants.append(value)
    add(lambda v: v['queries'].pop())
    add(lambda v: v['queries'][0]['targets'][0].update(binary_name='java/lang/Object'))
    add(lambda v: v['queries'][0]['targets'][0].update(descriptor='()I'))
    add(lambda v: v['queries'][0]['targets'][0]['linking_context'].update(project='wrong'))
    add(lambda v: v['queries'][5].update(status='Resolved'))
    add(lambda v: v['queries'][0].update(query_index=False))
    add(lambda v: v['queries'][0]['targets'].extend(copy.deepcopy(v['queries'][0]['targets'])))
    for mutation in mutants:
        try:
            exact(mutation, wanted, 'negative comparator control')
        except a.AcceptanceError:
            continue
        raise a.AcceptanceError('comparator accepted mutation')
    return len(mutants)


def run(args):
    security = a.input_helpers()
    specs = json.loads((ROOT / 'scripts/acceptance-inputs.json').read_text())
    for path in (args.sources_jar, args.java_base_jmod, args.native):
        security.reject_symlink_path(path)
    source, platform, native = [p.resolve(strict=True) for p in (args.sources_jar, args.java_base_jmod, args.native)]
    a.require(platform.is_file() and platform.stat().st_size <= specs['jdk']['max_file'], 'bounded platform file')
    a.same(a.sha256(platform), specs['jdk']['jmod_sha256'], 'platform hash')
    a.require(native.is_file() and os.access(native, os.X_OK), 'executable native CLI')
    if args.baseline or args.capture_baseline:
        a.same(a.sha256(native), BASELINE_SHA256, 'exact old executable')
    plan = None if args.capture_baseline else load_plan()
    with tempfile.TemporaryDirectory(prefix='binary-return-commons-') as temp:
        stage = Path(temp).resolve()
        root, notices, cwd = [stage / name for name in ('sources', 'notices', 'unrelated-cwd')]
        for p in (root, notices, cwd): p.mkdir()
        sources = a.extract_sources(source, specs['sources'], root, notices)
        hashes = {name: hashlib.sha256(data).hexdigest() for name, data in sorted(sources.items())}
        queries = query_plan(sources)
        manifest = stage / 'config/project.json'
        a.write_json(manifest, {'schema_version': 1, 'compilations': [{'id': a.COMPILATION,
            'source_roots': ['../sources'], 'dependencies': [], 'classpath': [], 'target_release': 21,
            'platform': [{'kind': 'jmod', 'path': os.path.relpath(platform, manifest.parent)}]}]})
        request = stage / 'queries/requests.json'
        a.write_json(request, {'schema_version': 1, 'queries': [{'compilation': a.COMPILATION,
            'file': '../sources/' + q['file'], 'position': {'encoding': 'utf8', 'offset': q['byte_offset']}} for q in queries]})
        def invoke(command, details=False):
            return a.run_cli(native, cwd, manifest, command, 120, 32*1024*1024,
                             request if command == 'declaration' else None, target_details=details)
        inspection = invoke('inspect'); a.check_inspection(inspection, root, sources, a.PLATFORM_CLASSES, platform)
        parser = invoke('parser-report'); a.check_parser_report(parser, inspection, root, sources, print_inventory=False)
        actual = {key: normalize(invoke('declaration', detailed)['result'], root, platform)
                  for key, detailed in [('default', False), ('details', True)]}
        metadata = {'schema_version': 1, 'feature': 'binary_return_composition',
                    'baseline_revision': BASELINE_REVISION, 'baseline_native_sha256': BASELINE_SHA256,
                    'source_archive_sha256': specs['sources']['sha256'], 'platform_sha256': specs['jdk']['jmod_sha256'],
                    'source_count': a.SOURCE_COUNT, 'source_bytes': a.SOURCE_BYTES,
                    'source_sha256': hashes, 'queries': queries}
        if args.capture_baseline:
            a.require(not PLAN.exists(), 'never overwrite a frozen corpus plan')
            candidate = {key: forecast(actual[key], queries, detailed) for key, detailed in [('default', False), ('details', True)]}
            plan = dict(metadata, baseline=actual, candidate=candidate)
            a.write_json(PLAN, plan)
        else:
            exact({k: plan[k] for k in metadata}, metadata, 'all corpus provenance and token positions')
            exact(actual, plan['baseline' if args.baseline else 'candidate'], 'complete default and detailed records')
        a.same({name: a.sha256(root / name) for name in sources}, hashes, 'all 249 source files unchanged')
        count = controls(plan['candidate']['details'])
        print('PASS: 8 complete Commons probes, 5 new selections, 2 refusals, 1 preserved inner target; '
              f'249 unchanged sources; {count} comparator controls; baseline={args.baseline}; capture={args.capture_baseline}')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('sources_jar', type=Path)
    p.add_argument('java_base_jmod', type=Path)
    p.add_argument('--native', type=Path, required=True)
    mode = p.add_mutually_exclusive_group()
    mode.add_argument('--baseline', action='store_true')
    mode.add_argument('--capture-baseline', action='store_true')
    run(p.parse_args())


if __name__ == '__main__':
    main()
