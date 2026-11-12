#!/usr/bin/env python3
"""Pinned compiler oracle for return-header inventory; never executes fixture programs."""
import argparse
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile

_spec = importlib.util.spec_from_file_location('header_prior', Path(__file__).with_name('check-strict-reference-oracle.py'))
prior = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(prior)
o = prior.o
ROOT = o.ORACLE / 'header-inventory-v1'
BASELINE_SHA256 = 'e03ce5307cce36444382b1be62eb0c23546b8d83ad10035cb642aa96f511ded6'
FIXTURE_SHA256 = '1c204c2609d0436935af2616fccd815ee79c4283e91eb08efc0af613ade6db15'


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def checked_fixture():
    data = (ROOT / 'fixture-manifest.json').read_bytes()
    assert hashlib.sha256(data).hexdigest() == FIXTURE_SHA256, 'frozen fixture manifest'
    files = json.loads(data)['files']
    actual = sorted(p.relative_to(ROOT).as_posix() for p in ROOT.rglob('*') if p.is_file() and p.name not in ('README.md', 'fixture-manifest.json'))
    assert actual == sorted(files), 'exact fixture inventory'
    for name, sha in files.items():
        assert digest(ROOT / name) == sha, name
    return json.loads((ROOT / 'predictions.json').read_text()), json.loads((ROOT / 'contract.json').read_text())


def compare(actual, expected):
    assert actual == expected, 'complete query records differ: ' + json.dumps({'actual': actual, 'expected': expected}, sort_keys=True)


def predicted_selection(ds, selected, compiler):
    path, start, finish, name = ds[selected]
    row = compiler['probe']
    assert row[:4] == [path, 'METHOD', 'inventory.Sink', name], 'independent selected source declaration'
    return ['Resolved', '1', '|'.join([path, 'Method', row[2], name, str(start), str(finish)])]


def verify_compiler_target(case_root, ds, selected, compiler):
    assert set(compiler) == {'probe'}
    path, start, _, name = ds[selected]
    source = (case_root / path).read_bytes()
    line_start = source.rfind(b'\n', 0, start) + 1
    line_end = source.find(b'\n', start)
    line = source[line_start:line_end]
    # Every handwritten selected declaration is public with a single trivial body.
    expected_start = source.rfind(b'public', line_start, start)
    assert expected_start >= line_start
    line_end = source.index(b'}', start) + 1
    expected_owner = 'inventory.Box' if selected == 'target' else 'inventory.Sink'
    assert compiler['probe'] == [path, 'METHOD', expected_owner, name, str(expected_start), str(line_end)], 'exact compiler declaration and range'


def run_suite(jdk, native, work, baseline=False, capture=False):
    if capture:
        assert not (ROOT / 'contract.json').exists(), 'never overwrite frozen predictions'
        predictions = json.loads((ROOT / 'predictions.json').read_text())
        contract = {'baseline_sha256': BASELINE_SHA256, 'cases': {}}
    else:
        predictions, contract = checked_fixture()
    if baseline or capture:
        assert digest(native) == BASELINE_SHA256, 'exact published baseline executable'
    env = {'PATH': '/nonexistent'}
    helper, empty = work / 'helper', work / 'empty'
    helper.mkdir(); empty.mkdir()
    platform = prior.pinned_platform(jdk, work)
    o.run([jdk / 'bin/javac', '--release', '21', '-proc:none', '-implicit:none', '-encoding', 'UTF-8', '-classpath', empty, '-sourcepath', empty, '-d', helper, o.ORACLE / 'BindingOracle.java'], env)
    assert set(predictions['cases']) == {p.name for p in ROOT.iterdir() if p.is_dir()}, 'case inventory'
    for name, spec in sorted(predictions['cases'].items()):
        case_root = ROOT / name
        ds, qs, request_text = o.fixture_markers(case_root, tuple(spec['files']))
        assert set(qs) == {'probe'}
        request = work / 'request.tsv'; request.write_text(request_text)
        compiled = o.run([jdk / 'bin/java', '-cp', helper, 'BindingOracle', case_root, request, empty, empty], env, False)
        if spec['kind'] == 'invalid':
            assert compiled.returncode == 2 and not compiled.stdout and 'compiler.err.' in compiled.stderr, 'compiler must reject malformed return: ' + name
            compiler = None
            diagnostics = compiled.stderr
            assert str(case_root) not in diagnostics and str(work) not in diagnostics, 'diagnostic must not leak environment-specific paths'
        else:
            assert compiled.returncode == 0, name + ': ' + compiled.stderr
            compiler = o.records(compiled.stdout)
            verify_compiler_target(case_root, ds, spec['compiler_selected'], compiler)
            diagnostics = None
        result = o.records(o.run([native, case_root, request, platform], env).stdout)
        assert set(result) == {'probe'}
        if capture:
            expected = copy.deepcopy(result)
            if spec['kind'] == 'positive':
                expected['probe'] = predicted_selection(ds, spec['compiler_selected'], compiler)
                assert result != expected, 'new-selection forecast must fail frozen baseline: ' + name
            elif spec['kind'] in ('invalid', 'bounded'):
                targets = []
                for selected in ('narrow', 'wide'):
                    path, start, finish, method = ds[selected]
                    targets.append('|'.join([path, 'Method', 'inventory.Sink', method, str(start), str(finish)]))
                expected['probe'] = ['OverloadNotResolved', '2'] + targets
            elif spec['kind'] == 'preserve':
                assert result['probe'][0] != 'Resolved', 'refusal prediction unexpectedly already resolved: ' + name
            contract['cases'][name] = {'compiler': compiler, 'compiler_diagnostics': diagnostics, 'baseline': result, 'current': expected}
        else:
            frozen = contract['cases'][name]
            compare(compiler, frozen['compiler'])
            compare(diagnostics, frozen['compiler_diagnostics'])
            compare(result, frozen['baseline' if baseline else 'current'])
        print(name + '\tkind=' + spec['kind'] + '\tnative=' + '|'.join(result['probe']), flush=True)
    assert set(contract['cases']) == set(predictions['cases'])
    if capture:
        (ROOT / 'contract.json').write_text(json.dumps(contract, sort_keys=True, indent=2) + '\n')
    print('PASS: complete fixed return-inventory records; baseline=' + str(baseline) + '; capture=' + str(capture), flush=True)


def main():
    if not __debug__:
        raise SystemExit('oracle requires Python without -O')
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--jdk', type=Path, required=True)
    p.add_argument('--jdk-archive', type=Path, required=True)
    p.add_argument('--native', type=Path, required=True)
    p.add_argument('--baseline', action='store_true')
    p.add_argument('--capture-baseline', action='store_true', help='one-time freeze; exact old binary and absent contract required')
    a = p.parse_args()
    assert not (a.baseline and a.capture_baseline)
    print('verified-jdk=' + prior.verify_jdk(a.jdk.resolve(), a.jdk_archive.resolve()), flush=True)
    with tempfile.TemporaryDirectory(prefix='header-inventory-oracle-') as tmp:
        run_suite(a.jdk.resolve(), a.native.resolve(), Path(tmp), a.baseline, a.capture_baseline)


if __name__ == '__main__':
    main()
