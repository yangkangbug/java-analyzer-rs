#!/usr/bin/env python3
"""Additive pinned Gson strict-reference-phase gate. Offline; never executes Java."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile

_spec = importlib.util.spec_from_file_location('strict_string', Path(__file__).with_name('check-string-call-acceptance.py'))
string = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(string)
second = string.second
PLAN = second.CORPUS / 'strict-reference-probes.json'
PLAN_SHA256 = '8a5e58f8ac87a206f3c61fa5459f24e3e0f62f25cfe4ac2470945d484d16d0df'

_contract_spec = importlib.util.spec_from_file_location('qualified_contract', Path(__file__).with_name('qualified-primitive-contract.py'))
qualified = importlib.util.module_from_spec(_contract_spec)
_contract_spec.loader.exec_module(qualified)


def load_plan(path=PLAN):
    second.reject_symlinks(path.absolute())
    data = path.read_bytes()
    second.same(hashlib.sha256(data).hexdigest(), PLAN_SHA256, 'frozen complete strict reference plan digest')
    return json.loads(data, object_pairs_hook=second.unique_json_object)


def probes(fixture):
    plan = load_plan()
    second.same(plan['source_archive_sha256'], fixture.metadata['inputs']['sources']['sha256'],
                'strict reference source archive pin')
    for name, checksum in plan['historical_artifact_sha256'].items():
        second.same(second.digest(second.CORPUS / name), checksum, 'strict reference preserved artifact ' + name)
    for name, checksum in plan['source_sha256'].items():
        data = fixture.sources[name]
        second.same(hashlib.sha256(data).hexdigest(), checksum, 'strict reference source digest ' + name)
        second.reject_symlinks(fixture.source_root / name)
        second.same((fixture.source_root / name).read_bytes(), data, 'strict reference extracted bytes ' + name)
    queries = plan['queries']
    second.same(len(queries), 4, 'strict reference complete fixed plan')
    second.same(len({q['id'] for q in queries}), 4, 'strict reference unique query IDs')
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
    # Independent source/formal evidence, frozen before candidate observation.
    for item in plan['source_spellings']:
        data = fixture.sources[item['path']]
        spelling = item['spelling'].encode()
        second.same(data[item['byte']:item['byte'] + len(spelling)], spelling,
                    'independent source/formal spelling ' + item['spelling'])
    return queries


def frozen_qualified_probes(fixture):
    """Exact post-single-primitive stage, not the evolving current contract."""
    return qualified.overlay(second, fixture, probes(fixture))


_mixed_spec = importlib.util.spec_from_file_location('mixed_strict_contract', Path(__file__).with_name('mixed-strict-contract.py'))
mixed = importlib.util.module_from_spec(_mixed_spec)
_mixed_spec.loader.exec_module(mixed)


def frozen_mixed_probes(fixture):
    return mixed.overlay(second, fixture, frozen_qualified_probes(fixture))


_return_spec = importlib.util.spec_from_file_location('reference_return_contract', Path(__file__).with_name('reference-return-contract.py'))
returns = importlib.util.module_from_spec(_return_spec)
_return_spec.loader.exec_module(returns)


def current_probes(fixture):
    return returns.overlay(second, fixture, frozen_mixed_probes(fixture))


def check_response(response, queries, fixture, capture_baseline=False):
    second.require(type(capture_baseline) is bool, 'comparison mode must be boolean')
    second.check_reference_call_response(response, queries, fixture,
                                         'baseline' if capture_baseline else 'expected')


def run(native, fixture, capture_baseline=False):
    proof_stage, parser_stage = second.proof_stages(capture_baseline)
    queries = probes(fixture) if capture_baseline else current_probes(fixture)
    before = second.all_source_hashes(fixture)
    request = second.document(fixture, 'strict-reference-requests.json', {
        'schema_version': 1, 'queries': [
            {'compilation': q['compilation'], 'file': 'sources/' + q['file'],
             'position': {'encoding': 'utf8', 'offset': q['byte']}} for q in queries]})
    response = second.successful(native, fixture, 'declaration', requests=request, strict_json=True)
    check_response(response, queries, fixture, capture_baseline)
    # Always use current string/constructor/reference/local/original expectations.
    preservation = string.run(native, fixture, proof_stage=proof_stage, parser_stage=parser_stage)
    after = second.all_source_hashes(fixture)
    second.same(before, after, 'all source bytes unchanged throughout strict reference gate')
    second.same(after, preservation['source_sha256_after'], 'complete preservation inventory')
    return {
        'validation': 'baseline_verified_not_acceptance' if capture_baseline else 'passed',
        'comparison_stage': 'baseline' if capture_baseline else 'expected',
        'plan_sha256': PLAN_SHA256 if capture_baseline else returns.PLAN_SHA256,
        'historical_strict_plan_sha256': PLAN_SHA256, 'native_sha256': second.digest(native),
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
    with tempfile.TemporaryDirectory(prefix='strict-reference-') as tmp:
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
