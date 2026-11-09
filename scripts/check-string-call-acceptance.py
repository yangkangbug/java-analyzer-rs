#!/usr/bin/env python3
"""Additive pinned Gson string-literal gate. Offline; never executes Java."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile

_spec = importlib.util.spec_from_file_location('string_second', Path(__file__).with_name('check-second-project.py'))
second = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(second)
PLAN = second.CORPUS / 'string-call-probes.json'
PLAN_SHA256 = '730dc287eda7cac024c0a32cf9f4251b40e091fbb6c29b60803a26894d52ac8e'


def load_plan(path=PLAN):
    second.reject_symlinks(path.absolute())
    data = path.read_bytes()
    second.same(hashlib.sha256(data).hexdigest(), PLAN_SHA256, 'frozen complete string plan digest')
    return json.loads(data, object_pairs_hook=second.unique_json_object)


def probes(fixture):
    plan = load_plan()
    second.same(plan['source_archive_sha256'], fixture.metadata['inputs']['sources']['sha256'],
                'string source archive pin')
    for name, checksum in plan['historical_artifact_sha256'].items():
        second.same(second.digest(second.CORPUS / name), checksum, 'string preserved artifact ' + name)
    for name, checksum in plan['source_sha256'].items():
        data = fixture.sources[name]
        second.same(hashlib.sha256(data).hexdigest(), checksum, 'string source digest ' + name)
        second.reject_symlinks(fixture.source_root / name)
        second.same((fixture.source_root / name).read_bytes(), data, 'string extracted bytes ' + name)
    queries = plan['queries']
    second.same(len(queries), 8, 'string complete fixed plan')
    second.same(len({q['id'] for q in queries}), 8, 'string unique query IDs')
    for query in queries:
        data = fixture.sources[query['file']]
        expression = query['expression'].encode()
        second.same(data[query['byte']:query['byte'] + len(expression)], expression,
                    query['id'] + ' exact argument expression')
        for stage in ('baseline', 'expected'):
            for target in query[stage]['targets']:
                data = fixture.sources[target['path']]
                second.same(data[target['start']:target['end']], target['name'].encode(),
                            query['id'] + ' exact declaration token')
    # These identities come from source inspection, not baseline candidates.
    source = fixture.sources['gson/src/main/java/com/google/gson/JsonObject.java']
    for offset, spelling in [(5914, b'get(String memberName)'),
                             (2758, b'remove(String property)'), (5601, b'has(String memberName)'),
                             (2345, b'add(String property, JsonElement value)')]:
        second.same(source[offset:offset + len(spelling)], spelling, 'independent String formal')
    return queries


def check_response(response, queries, fixture, capture_baseline=False):
    second.check_reference_call_response(response, queries, fixture,
                                         'baseline' if capture_baseline else 'expected')


def run(native, fixture, capture_baseline=False):
    queries = probes(fixture)
    before = second.all_source_hashes(fixture)
    request = second.document(fixture, 'string-call-requests.json', {
        'schema_version': 1, 'queries': [
            {'compilation': q['compilation'], 'file': 'sources/' + q['file'],
             'position': {'encoding': 'utf8', 'offset': q['byte']}} for q in queries]})
    response = second.successful(native, fixture, 'declaration', requests=request, strict_json=True)
    check_response(response, queries, fixture, capture_baseline)
    # aa3 already includes the constructor correction. Baseline capture here must
    # therefore use CURRENT preservation expectations, never the constructor-era
    # baseline mode that deliberately expects its earlier refusal.
    preservation = second.run_constructor_call_acceptance(native, fixture)
    after = second.all_source_hashes(fixture)
    second.same(before, after, 'all source bytes unchanged throughout string gate')
    second.same(after, preservation['source_sha256_after'], 'complete preservation inventory')
    return {
        'validation': 'baseline_verified_not_acceptance' if capture_baseline else 'passed',
        'comparison_stage': 'baseline' if capture_baseline else 'expected',
        'plan_sha256': PLAN_SHA256, 'native_sha256': second.digest(native),
        'queries': second.portable_queries(response, fixture),
        'preservation': preservation, 'source_sha256_before': before, 'source_sha256_after': after,
        'source_bytes_unchanged': True, 'javac_real_gson_equivalence': False,
        'upstream_code_executed': False,
        'analysis_environment': {'PATH': '/nonexistent', 'java_executed': False},
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('inputs_dir', type=Path)
    parser.add_argument('--native', type=Path, required=True)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--capture-baseline', action='store_true')
    args = parser.parse_args()
    native = args.native.resolve()
    second.require(native.is_file(), 'native executable must exist')
    with tempfile.TemporaryDirectory(prefix='string-call-') as tmp:
        fixture = second.prepare_fixture(args.inputs_dir, Path(tmp) / 'fixture')
        report = run(native, fixture, args.capture_baseline)
        text = json.dumps(report, indent=2) + '\n'
        if args.output:
            second.reject_symlinks(args.output.absolute())
            with args.output.open('x', encoding='utf-8') as output:
                output.write(text)
        print(text, end='')


if __name__ == '__main__':
    main()
