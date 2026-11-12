#!/usr/bin/env python3
"""Optional isolated javac declaration oracle for effective binary methods.
Compiles only handwritten dependency/helper files. Never executes fixture programs.
"""
import argparse
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile

spec = importlib.util.spec_from_file_location('effective_prior', Path(__file__).with_name('check-strict-reference-oracle.py'))
prior = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prior)
o = prior.o
ROOT = o.ORACLE / 'effective-binary-v1'
BASELINE_SHA256 = '89280e2a70afb12aed329f9551581e4e63a96711b12ffb1f58fd673e8ab88b10'
FIXTURE_SHA256 = '3d933fd21ffe594678488da11b5ab00ffb1bb2e491b6ccadabda3d7d42d69dbb'
CONTRACT_SHA256 = '982827923c8bacb6b467412ad680ab89cb5529b37b751b6c8c162117008f7175'
ENV = {'PATH': '/nonexistent'}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        assert key not in result, 'duplicate JSON field'
        result[key] = value
    return result


def fixture(capture=False):
    data = (ROOT / 'fixture-manifest.json').read_bytes()
    if not capture:
        assert hashlib.sha256(data).hexdigest() == FIXTURE_SHA256, 'frozen fixture manifest'
    manifest = json.loads(data)
    for name, checksum in manifest['files'].items():
        assert digest(ROOT / name) == checksum, 'fixture bytes: ' + name
    actual = sorted(p.relative_to(ROOT).as_posix() for p in ROOT.rglob('*')
                    if p.is_file() and p.name not in ('README.md', 'fixture-manifest.json', 'contract.json'))
    assert actual == sorted(manifest['files']), 'exact fixture inventory'
    prediction = json.loads((ROOT / 'predictions.json').read_text())
    assert set(prediction['native_positive']).isdisjoint(prediction['native_refusal'])
    assert set(prediction['compiler']) == set(prediction['native_positive'] + prediction['native_refusal'])
    if not capture:
        assert digest(ROOT / 'contract.json') == CONTRACT_SHA256, 'frozen native contract'
    return prediction


def normalize(value, work, platform):
    if isinstance(value, dict):
        return {key: normalize(item, work, platform) for key, item in value.items()}
    if isinstance(value, list):
        return [normalize(item, work, platform) for item in value]
    if isinstance(value, str):
        for path, token in [(work / 'dependency', '@DEPENDENCY@'), (ROOT / 'source', '@SOURCE@'),
                            (platform, '@PLATFORM@')]:
            value = value.replace(str(path), token)
    return value


def target(owner, name, descriptor):
    platform = owner.startswith('java/')
    return {'kind': 'Method', 'name': name, 'owner': owner.replace('/', '.'),
            'origin': 'binary', 'source_available': False,
            'path': ('@PLATFORM@!/classes/' if platform else '@DEPENDENCY@/') + owner + '.class',
            'binary_name': owner, 'loaded_contexts': [{'project': 'fixture', 'source_set': 'main'}],
            'linking_context': {'project': 'fixture', 'source_set': 'main'},
            'identity_precision': 'origin_binary_name_descriptor_linking_context', 'descriptor': descriptor}


def compare(actual, expected):
    # Canonical JSON also rejects bool-as-int coordinates.
    assert json.dumps(actual, sort_keys=True) == json.dumps(expected, sort_keys=True), \
        'complete records differ: ' + json.dumps({'actual': actual, 'expected': expected}, sort_keys=True)


def adversarial_controls(expected):
    mutations = []
    omitted = copy.deepcopy(expected); omitted['queries'].pop(); mutations.append(omitted)
    duplicated = copy.deepcopy(expected); duplicated['queries'][0]['targets'] *= 2; mutations.append(duplicated)
    wrong_owner = copy.deepcopy(expected); wrong_owner['queries'][0]['targets'][0]['binary_name'] = 'dep/Wrong'; mutations.append(wrong_owner)
    wrong_descriptor = copy.deepcopy(expected); wrong_descriptor['queries'][0]['targets'][0]['descriptor'] = '()V'; mutations.append(wrong_descriptor)
    wrong_status = copy.deepcopy(expected); wrong_status['queries'][0]['status'] = 'CallNotResolved'; mutations.append(wrong_status)
    wrong_context = copy.deepcopy(expected); wrong_context['queries'][0]['targets'][0]['linking_context']['project'] = 'other'; mutations.append(wrong_context)
    for mutation in mutations:
        try:
            compare(mutation, expected)
        except AssertionError:
            continue
        raise AssertionError('comparator accepted mutation')
    return len(mutations)


def run_suite(jdk, native, work, baseline=False, capture=False):
    prediction = fixture(capture)
    if baseline or capture:
        assert digest(native) == BASELINE_SHA256, 'exact pre-implementation baseline executable'
    helper, dependency, empty = [work / name for name in ('helper', 'dependency', 'empty')]
    for path in (helper, dependency, empty):
        path.mkdir()
    common = [jdk / 'bin/javac', '--release', '21', '-g:none', '-proc:none', '-implicit:none',
              '-encoding', 'UTF-8', '-classpath', empty, '-sourcepath', empty]
    o.run(common + ['-d', helper, ROOT / 'BinaryDeclarationOracle.java'], ENV)
    dependencies = sorted((ROOT / 'dependency').rglob('*.java'))
    o.run(common + ['-d', dependency, *dependencies], ENV)
    assert sorted(p.relative_to(dependency).as_posix() for p in dependency.rglob('*.class')) == \
        sorted('dep/' + p.stem + '.class' for p in dependencies), 'exact compiled fixture classes'
    _, queries, compiler_requests = o.fixture_markers(ROOT / 'source', ['app/Use.java'])
    assert set(queries) == set(prediction['compiler'])
    request = work / 'compiler-requests.tsv'; request.write_text(compiler_requests)
    compiled = o.records(o.run([jdk / 'bin/java', '-cp', helper, 'BinaryDeclarationOracle',
                                ROOT / 'source', request, dependency, empty], ENV).stdout)
    compare(compiled, prediction['compiler'])
    rejected = {}
    for kind in ('final_override', 'primitive_return'):
        result = o.run(common + ['-XDrawDiagnostics', '-d', work / ('invalid-' + kind),
                                ROOT / 'invalid' / kind / 'Bad.java'], ENV, False)
        assert result.returncode != 0 and 'compiler.err.' in result.stderr, 'javac must reject ' + kind
        rejected[kind] = [line for line in result.stderr.splitlines() if 'compiler.err.' in line]
    platform = jdk / 'jmods/java.base.jmod'
    scope = {'project': 'fixture', 'source_set': 'main'}
    manifest = work / 'project.json'
    manifest.write_text(json.dumps({'schema_version': 1, 'compilations': [
        {'id': scope, 'source_roots': [str(ROOT / 'source')],
         'classpath': [{'kind': 'directory', 'path': str(dependency)}],
         'platform': [{'kind': 'jmod', 'path': str(platform)}], 'target_release': 21}]}))
    native_requests = work / 'requests.json'
    native_requests.write_text(json.dumps({'schema_version': 1, 'queries': [
        {'compilation': scope, 'file': str(ROOT / 'source' / path),
         'position': {'encoding': 'utf8', 'offset': at}} for path, at, _, _ in queries.values()]}))
    output = o.run([native, 'declaration', '--manifest', manifest, '--requests', native_requests, '--target-details'], ENV)
    assert not output.stderr, 'successful native stderr'
    response = json.loads(output.stdout, object_pairs_hook=unique_object)
    assert set(response) == {'schema_version', 'command', 'capabilities', 'snapshot', 'result'}
    assert type(response['schema_version']) is int and response['schema_version'] == 1
    assert response['command'] == 'declaration'
    assert response['capabilities']['runtime'] == 'native_rust_no_jvm'
    assert response['snapshot']['manifest'] == str(manifest)
    actual = normalize(response['result'], work, platform)
    if capture:
        assert not (ROOT / 'contract.json').exists(), 'never overwrite frozen contract'
        wanted = copy.deepcopy(actual)
        for query, row in zip(queries, wanted['queries']):
            if query in prediction['native_positive']:
                row['status'] = 'Resolved'
                row['targets'] = [target(*prediction['compiler'][query])]
            else:
                assert row['status'] != 'Resolved', 'baseline must already refuse ' + query
        contract = {'baseline_sha256': BASELINE_SHA256, 'query_order': list(queries),
                    'compiler': compiled, 'invalid_diagnostics': rejected,
                    'baseline': actual, 'candidate': wanted}
        (ROOT / 'contract.json').write_text(json.dumps(contract, indent=2, sort_keys=True) + '\n')
    else:
        contract = json.loads((ROOT / 'contract.json').read_text())
        assert contract['baseline_sha256'] == BASELINE_SHA256
        compare(list(queries), contract['query_order'])
        compare(compiled, contract['compiler'])
        compare(rejected, contract['invalid_diagnostics'])
        compare(actual, contract['baseline' if baseline else 'candidate'])
    controls = adversarial_controls(contract['candidate'])
    print(json.dumps({'compiler_queries': len(compiled), 'native_positive_predictions': len(prediction['native_positive']),
                      'native_conservative_refusals': len(prediction['native_refusal']),
                      'compiler_rejected_sources': len(rejected), 'comparator_controls': controls,
                      'baseline': baseline, 'capture': capture, 'native_sha256': digest(native)}, sort_keys=True))
    print('PASS: exact binary declaration owner/descriptor, complete native rows, compiler-only optional oracle; no fixture execution')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--jdk', type=Path, required=True)
    parser.add_argument('--jdk-archive', type=Path, required=True)
    parser.add_argument('--native', type=Path, required=True)
    parser.add_argument('--baseline', action='store_true')
    parser.add_argument('--capture-baseline', action='store_true')
    args = parser.parse_args()
    print('verified-jdk-archive-sha256=' + prior.verify_jdk(args.jdk.resolve(), args.jdk_archive.resolve()), flush=True)
    with tempfile.TemporaryDirectory(prefix='effective-binary-oracle-') as temporary:
        run_suite(args.jdk.resolve(), args.native.resolve(), Path(temporary), args.baseline, args.capture_baseline)


if __name__ == '__main__':
    main()

