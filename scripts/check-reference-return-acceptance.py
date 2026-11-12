#!/usr/bin/env python3
"""Exact reference-return current/4eecefd0 baseline gate; native only, no downloads."""
import argparse
import importlib.util
import json
from pathlib import Path
import tempfile

_spec = importlib.util.spec_from_file_location('return_strict', Path(__file__).with_name('check-strict-reference-acceptance.py'))
strict = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(strict)
second = strict.second


def check_response(response, queries, fixture, capture_baseline=False):
    second.require(type(capture_baseline) is bool, 'comparison mode must be boolean')
    second.check_reference_call_response(response, queries, fixture,
                                         'baseline' if capture_baseline else 'expected')


def run(native, fixture, capture_baseline=False):
    second.require(type(capture_baseline) is bool, 'comparison mode must be boolean')
    queries = strict.frozen_return_probes(fixture) if capture_baseline else strict.current_probes(fixture)
    if capture_baseline:
        second.same(second.digest(native), strict.returns.load_plan(second)['baseline_native_sha256'],
                    'exact frozen 4eecefd0 baseline binary')
    before = second.all_source_hashes(fixture)
    request = second.document(fixture, 'reference-return-requests.json', {
        'schema_version': 1, 'queries': [
            {'compilation': q['compilation'], 'file': 'sources/' + q['file'],
             'position': {'encoding': 'utf8', 'offset': q['byte']}} for q in queries]})
    response = second.successful(native, fixture, 'declaration', requests=request, strict_json=True)
    check_response(response, queries, fixture, capture_baseline)
    preservation = strict.string.run(native, fixture, proof_stage='mixed' if capture_baseline else 'cast_null', parser_stage='current')
    after = second.all_source_hashes(fixture)
    second.same(before, after, 'all source bytes unchanged')
    second.same(after, preservation['source_sha256_after'], 'complete preservation inventory')
    return {
        'validation': 'baseline_verified_not_acceptance' if capture_baseline else 'passed',
        'comparison_stage': 'return_baseline_4eecefd0' if capture_baseline else 'return_current',
        'plan_sha256': strict.returns.PLAN_SHA256 if capture_baseline else strict.casts.PLAN_SHA256, 'native_sha256': second.digest(native),
        'queries': second.portable_queries(response, fixture), 'preservation': preservation,
        'source_sha256_before': before, 'source_sha256_after': after,
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
    native = args.native.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix='reference-return-') as tmp:
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
