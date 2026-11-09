#!/usr/bin/env python3
"""Optional handwritten constructor oracle. No downloads or upstream build scripts."""
import argparse
import importlib.util
from pathlib import Path
import tempfile

SPEC = importlib.util.spec_from_file_location('constructor_semantics', Path(__file__).with_name('check-semantic-oracle.py'))
o = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(o)
ROOT = o.ORACLE / 'constructor-v1'
FILES = ('Abstract.java', 'Annotated.java', 'Default.java', 'Extra.java', 'Generic.java',
         'GenericCtor.java', 'Leaf.java', 'Marker.java', 'Other.java', 'Outer.java',
         'PackageCtor.java', 'ProtectedCtor.java', 'Root.java', 'Target.java',
         'Throwing.java', 'Use.java', 'Vararg.java')
CASES = frozenset(('specificity', 'identity', 'widening', 'interface', 'annotated', 'qualified', 'implicit',
                  'additional', 'throws', 'generic', 'generic_ctor', 'varargs', 'package_ctor',
                  'protected_ctor', 'anonymous', 'abstract_anonymous', 'nested', 'arguments', 'array'))
INVALID = frozenset(('private', 'abstract', 'missing_zero', 'duplicate', 'throws'))


def fixture():
    assert (ROOT / 'files.tsv').read_text().splitlines() == list(FILES)
    ds, qs, req = o.fixture_markers(ROOT / 'source', FILES)
    expected = {}
    for q, status, selected, candidates in o.rows(ROOT / 'expected.tsv', 4):
        assert q not in expected
        expected[q] = (status, selected, candidates)
    assert set(qs) == set(expected) == {'constructor_' + case for case in CASES}
    assert set(ds) == {'pick_root', 'pick_other', 'exact_leaf', 'exact_other',
                       'marker_interface', 'marker_other', 'array_object', 'array_other', 'specific_root', 'specific_leaf', 'specific_other'}
    assert sum(row[0] == 'Resolved' for row in expected.values()) == 6
    return ds, qs, req, expected


def invalid_fixtures():
    cases = o.rows(ROOT / 'invalid.tsv', 5)
    assert len(cases) == len(INVALID) and {row[0] for row in cases} == INVALID
    assert sorted(p.relative_to(ROOT / 'invalid').as_posix() for p in (ROOT / 'invalid').rglob('*.java')) == sorted(case + '/Use.java' for case in INVALID)
    for case, q, status, candidates, files in cases:
        assert files == 'Use.java' and q == 'constructor_invalid_' + case
        assert status == 'OverloadNotResolved'
        ds, qs, req = o.fixture_markers(ROOT / 'invalid' / case, ('Use.java',))
        assert set(qs) == {q} and set(ds) == {'invalid_root', 'invalid_string'}
        yield case, q, status, candidates, ds, req


def run_suite(jdk, native, work, baseline=False):
    env = {'PATH': '/nonexistent'}
    helper, empty = work / 'helper', work / 'empty'
    helper.mkdir(); empty.mkdir()
    platform = o.pinned_object(jdk, work)
    o.run([jdk / 'bin/javac', '--release', '21', '-proc:none', '-implicit:none',
           '-encoding', 'UTF-8', '-classpath', empty, '-sourcepath', empty, '-d', helper,
           o.ORACLE / 'BindingOracle.java'], env)
    ds, qs, req, expected = fixture()
    request = work / 'requests.tsv'; request.write_text(req)
    javac = o.records(o.run([jdk / 'bin/java', '-cp', helper, 'BindingOracle', ROOT / 'source', request, empty, empty], env).stdout)
    actual = o.records(o.run([native, ROOT / 'source', request, platform], env).stdout)
    if baseline:
        expected = {q: ('OverloadNotResolved', selected,
                       selected + ',' + {'pick_root@ctor.Target': 'pick_other@ctor.Target',
                                         'exact_leaf@ctor.Target': 'exact_other@ctor.Target',
                                         'marker_interface@ctor.Target': 'marker_other@ctor.Target',
                                         'specific_leaf@ctor.Target': 'specific_root@ctor.Target,specific_other@ctor.Target'}[selected])
                    if status == 'Resolved' else (status, selected, candidates)
                    for q, (status, selected, candidates) in expected.items()}
    o.compare_reference(expected, qs, ds, javac, actual, platform)
    for q in expected:
        print(q + '\tjavac=' + '|'.join(javac[q]) + '\tnative=' + '|'.join(actual[q]))
    for case, q, status, candidates, ids, contents in invalid_fixtures():
        request.write_text(contents)
        rejected = o.run([jdk / 'bin/java', '-cp', helper, 'BindingOracle', ROOT / 'invalid' / case, request, empty, empty], env, False)
        assert rejected.returncode == 2 and 'compiler.err.' in rejected.stderr and not rejected.stdout
        result = o.records(o.run([native, ROOT / 'invalid' / case, request, platform], env).stdout)
        assert set(result) == {q}
        o.check_reference_candidates(q, status, candidates, ids, result[q], platform)
        print(q + '\tjavac=compiler-error\tnative=' + '|'.join(result[q]))
    request.write_text(req)
    omitted = o.records(o.run([native, ROOT / 'source', request, empty], env).stdout)
    assert set(omitted) == set(expected)
    assert all(omitted[q][0] != 'Resolved' for q in expected)
    print('PASS: handwritten constructor suite; baseline=' + str(baseline) + '; no whole-Gson Java equivalence claimed')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--jdk', type=Path, required=True)
    p.add_argument('--native', type=Path, required=True)
    p.add_argument('--baseline', action='store_true')
    args = p.parse_args()
    with tempfile.TemporaryDirectory(prefix='constructor-oracle-') as tmp:
        run_suite(args.jdk.resolve(), args.native.resolve(), Path(tmp), args.baseline)


if __name__ == '__main__':
    main()
