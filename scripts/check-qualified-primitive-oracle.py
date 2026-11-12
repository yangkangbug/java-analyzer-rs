#!/usr/bin/env python3
"""Optional pinned javac primitive-instance oracle; never runs fixture programs."""
import argparse
import importlib.util
from pathlib import Path
import tempfile

_spec = importlib.util.spec_from_file_location('qualified_prior_oracle', Path(__file__).with_name('check-strict-reference-oracle.py'))
prior = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(prior)
o = prior.o
ROOT = o.ORACLE / 'qualified-primitive-v1'
FILES = ('Base.java', 'Derived.java', 'Target.java', 'Use.java')
CASES = frozenset(('literal', 'int', 'byte', 'reverse', 'widen', 'char', 'boolean',
                   'unknown', 'long_suffix', 'boxed', 'varargs', 'generic', 'inherited', 'mixed'))
SELECTABLE = frozenset(('literal', 'int', 'byte', 'reverse', 'widen', 'char', 'boolean'))
DECLARATIONS = frozenset(('pick_int', 'pick_long', 'reverse_long', 'reverse_int', 'widen_int', 'widen_short',
                          'chars_int', 'chars_char', 'bool_int', 'bool_boolean', 'boxed', 'varargs',
                          'generic_ref', 'generic_int', 'mixed_int', 'mixed_long', 'inherited_int', 'inherited_long'))
BASELINE = {'literal': 'pick_int,pick_long', 'int': 'pick_int,pick_long', 'byte': 'pick_int,pick_long',
            'reverse': 'reverse_long,reverse_int', 'widen': 'widen_int,widen_short',
            'char': 'chars_int,chars_char', 'boolean': 'bool_int,bool_boolean'}
INVALID = {'narrow': ('CallNotResolved', 'narrow', 'compiler.err.prob.found.req'),
           'overflow': ('OverloadNotResolved', 'first,second', 'compiler.err.int.number.too.large')}


def fixture():
    assert (ROOT / 'files.tsv').read_text().splitlines() == list(FILES)
    ds, qs, req = o.fixture_markers(ROOT / 'source', FILES)
    expected = {}
    for q, status, selected, candidates in o.rows(ROOT / 'expected.tsv', 4):
        assert q not in expected
        expected[q] = status, selected, candidates
    assert set(qs) == set(expected) == {'primitive_' + case for case in CASES}
    assert set(ds) == DECLARATIONS
    assert {q for q, row in expected.items() if row[0] == 'Resolved'} == {'primitive_' + x for x in SELECTABLE}
    return ds, qs, req, expected


def invalid_fixtures():
    assert sorted(p.relative_to(ROOT / 'invalid').as_posix() for p in (ROOT / 'invalid').rglob('*.java')) == sorted(case + '/Use.java' for case in INVALID)
    for case, (status, names, diagnostic) in INVALID.items():
        ds, qs, req = o.fixture_markers(ROOT / 'invalid' / case, ('Use.java',))
        assert set(qs) == {'invalid_' + case} and set(ds) == set(names.split(','))
        yield case, status, ','.join(n + '@qualified.Target' for n in names.split(',')), diagnostic, ds, req


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
    for case, names in BASELINE.items():
        q = 'primitive_' + case
        result[q] = 'OverloadNotResolved', expected[q][1], ','.join(n + '@qualified.Target' for n in names.split(','))
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
    request.write_text(req)
    path = platform / 'java/lang/Object.class'; contents = path.read_bytes(); path.unlink()
    try:
        omitted = o.records(o.run([native, ROOT / 'source', request, platform], env).stdout)
        assert set(omitted) == set(expected)
        for q, row in baseline_expectations(expected).items():
            o.check_reference_candidates(q, row[0], row[2], ds, omitted[q], platform)
    finally:
        path.write_bytes(contents)
    print('PASS: qualified primitive oracle; 7 selections, 7 compiler-valid native refusals, 2 compiler-rejected cases; Object omission exact refusals; baseline=' + str(baseline), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--jdk', type=Path, required=True)
    parser.add_argument('--jdk-archive', type=Path, required=True)
    parser.add_argument('--native', type=Path, required=True)
    parser.add_argument('--baseline', action='store_true')
    args = parser.parse_args()
    print('verified-jdk-archive-sha256=' + prior.verify_jdk(args.jdk.resolve(), args.jdk_archive.resolve()), flush=True)
    with tempfile.TemporaryDirectory(prefix='qualified-primitive-oracle-') as tmp:
        run_suite(args.jdk.resolve(), args.native.resolve(), Path(tmp), args.baseline)


if __name__ == '__main__':
    main()
