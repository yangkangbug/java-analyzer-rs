#!/usr/bin/env python3
"""Optional pinned javac cast-null oracle; never runs fixture programs."""
import argparse
import importlib.util
from pathlib import Path
import tempfile

_spec = importlib.util.spec_from_file_location('mixed_prior_oracle', Path(__file__).with_name('check-strict-reference-oracle.py'))
prior = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(prior)
o = prior.o
ROOT = o.ORACLE / 'cast-null-v1'
FIXTURE_SHA256 = 'a3df20e34830eaf20898427cbabd62ed81bd6c68a4f462869f851162f8b2a9a8'
FILES = ('Box.java', 'Child.java', 'Parent.java', 'Target.java', 'Use.java')
INVALID = {'primitive': ('CallNotResolved', 'only', 'compiler.err.prob.found.req'),
           'unknown': ('CallNotResolved', 'only', 'compiler.err.cant.resolve.location')}


def fixture():
    import hashlib, json
    data = (ROOT / 'fixture-manifest.json').read_bytes()
    assert hashlib.sha256(data).hexdigest() == FIXTURE_SHA256
    pinned = json.loads(data)['files']
    assert sorted(p.relative_to(ROOT).as_posix() for p in ROOT.rglob('*') if p.is_file() and p.name not in ('fixture-manifest.json', 'README.md')) == sorted(pinned)
    for name, digest in pinned.items(): assert hashlib.sha256((ROOT / name).read_bytes()).hexdigest() == digest
    assert (ROOT / 'files.tsv').read_text().splitlines() == list(FILES)
    ds, qs, req = o.fixture_markers(ROOT / 'source', FILES)
    expected = {}
    for q, status, selected, candidates in o.rows(ROOT / 'expected.tsv', 4):
        assert q not in expected
        expected[q] = status, selected, candidates
    assert set(qs) == set(expected) and len(qs) == 10
    assert set(ds) == {'parent', 'child', 'array', 'generic', 'string', 'comparable'}
    return ds, qs, req, expected


def invalid_fixtures():
    assert sorted(p.relative_to(ROOT / 'invalid').as_posix() for p in (ROOT / 'invalid').rglob('*.java')) == sorted(case + '/Use.java' for case in INVALID)
    for case, (status, names, diagnostic) in INVALID.items():
        ds, qs, req = o.fixture_markers(ROOT / 'invalid' / case, ('Use.java',))
        assert set(qs) == {'invalid_' + case} and set(ds) == set(names.split(','))
        yield case, status, ','.join(n + '@casts.Target' for n in names.split(',')), diagnostic, ds, req


def compiler_range(declarations, marker):
    path, start, _, _ = declarations[marker]
    source = (ROOT / 'source' / path).read_bytes()
    line_start = source.rfind(b'\n', 0, start) + 1
    line_end = source.find(b'\n', start)
    line = source[line_start:line_end]
    return line_start + len(line) - len(line.lstrip()), line_end


def compare(expected, queries, declarations, javac, native, platform):
    o.compare_reference(expected, queries, declarations, javac, native, platform)
    for q, (_, selected, _) in expected.items():
        assert tuple(map(int, javac[q][4:])) == compiler_range(declarations, selected.split('@')[0]), q + ': exact compiler range differs'


def baseline_expectations(expected):
    result = dict(expected)
    for q in ('typed_parent', 'typed_child', 'parenthesized'):
        result[q] = ('OverloadNotResolved', expected[q][1], 'parent@casts.Target,child@casts.Target')
    return result


def run_suite(jdk, native, work, baseline=False):
    env = {'PATH': '/nonexistent'}
    helper, empty = work / 'helper', work / 'empty'
    helper.mkdir(); empty.mkdir()
    platform = prior.pinned_platform(jdk, work)
    o.run([jdk / 'bin/javac', '--release', '21', '-proc:none', '-implicit:none', '-encoding', 'UTF-8', '-classpath', empty, '-sourcepath', empty, '-d', helper, o.ORACLE / 'BindingOracle.java'], env)
    ds, qs, req, expected = fixture()
    request = work / 'requests.tsv'; request.write_text(req)
    compiler = o.records(o.run([jdk / 'bin/java', '-cp', helper, 'BindingOracle', ROOT / 'source', request, empty, empty], env).stdout)
    native_rows = o.records(o.run([native, ROOT / 'source', request, platform], env).stdout)
    for q in sorted(expected):
        print(q + '\tjavac=' + '|'.join(compiler[q]) + '\tnative=' + '|'.join(native_rows[q]), flush=True)
    compare(baseline_expectations(expected) if baseline else expected, qs, ds, compiler, native_rows, platform)
    if baseline:
        o.must_reject(lambda: compare(expected, qs, ds, compiler, native_rows, platform), 'baseline must fail new selected goldens')
    for case, status, candidates, diagnostic, ids, contents in invalid_fixtures():
        request.write_text(contents)
        rejected = o.run([jdk / 'bin/java', '-cp', helper, 'BindingOracle', ROOT / 'invalid' / case, request, empty, empty], env, False)
        print('invalid_' + case + '\tcompiler-rejected=' + rejected.stderr.strip(), flush=True)
        assert rejected.returncode == 2 and diagnostic in rejected.stderr and not rejected.stdout, rejected.stderr
        result = o.records(o.run([native, ROOT / 'invalid' / case, request, platform], env).stdout)
        q = 'invalid_' + case
        assert set(result) == {q}
        o.check_reference_candidates(q, status, candidates, ids, result[q], platform)
    print('PASS: frozen cast-null oracle; exact typed-null selections and unsupported-expression refusals; baseline=' + str(baseline), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--jdk', type=Path, required=True)
    parser.add_argument('--jdk-archive', type=Path, required=True)
    parser.add_argument('--native', type=Path, required=True)
    parser.add_argument('--baseline', action='store_true')
    args = parser.parse_args()
    print('verified-jdk-archive-sha256=' + prior.verify_jdk(args.jdk.resolve(), args.jdk_archive.resolve()), flush=True)
    with tempfile.TemporaryDirectory(prefix='cast-null-oracle-') as tmp:
        run_suite(args.jdk.resolve(), args.native.resolve(), Path(tmp), args.baseline)


if __name__ == '__main__':
    main()
