#!/usr/bin/env python3
"""Opt-in handwritten string literal oracle. No downloads or fixture execution."""
import argparse
import importlib.util
from pathlib import Path
import tempfile
import zipfile

SPEC = importlib.util.spec_from_file_location('string_semantics', Path(__file__).with_name('check-semantic-oracle.py'))
o = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(o)
ROOT = o.ORACLE / 'string-v1'
FILES = ('Target.java', 'Ref.java', 'String.java', 'Use.java')
CASES = frozenset(('plain', 'empty', 'unicode', 'escapes', 'octal', 'pair', 'arity_one',
                   'arity_two', 'mixed', 'object', 'choose', 'other', 'null', 'cast',
                   'concat', 'field', 'result', 'unicode_escape', 'textblock'))
DECLARATIONS = frozenset(('exact', 'pair', 'arity_one', 'arity_two', 'mixed', 'object',
                          'choose_string', 'choose_object', 'other_string', 'other_ref'))
PLATFORM = ('java/lang/Object', 'java/lang/String', 'java/io/Serializable',
            'java/lang/Comparable', 'java/lang/CharSequence',
            'java/lang/constant/Constable', 'java/lang/constant/ConstantDesc')
INVALID = {'escape': ('CallNotResolved', 'invalid', 'compiler.err.illegal.esc.char'),
           'arity': ('CallNotResolved', 'invalid', 'compiler.err.cant.apply.symbol'),
           'private': ('CallNotResolved', 'invalid', 'compiler.err.report.access'),
           'ambiguous': ('OverloadNotResolved', 'invalid,invalid_ref', 'compiler.err.ref.ambiguous')}


def fixture():
    assert (ROOT / 'files.tsv').read_text().splitlines() == list(FILES)
    ds, qs, req = o.fixture_markers(ROOT / 'source', FILES)
    expected = {}
    for q, status, selected, candidates in o.rows(ROOT / 'expected.tsv', 4):
        assert q not in expected
        expected[q] = (status, selected, candidates)
    assert set(qs) == set(expected) == {'string_' + case for case in CASES}
    assert set(ds) == DECLARATIONS
    assert sum(row[0] == 'Resolved' for row in expected.values()) == 9
    return ds, qs, req, expected


def invalid_fixtures():
    assert sorted(p.relative_to(ROOT / 'invalid').as_posix() for p in (ROOT / 'invalid').rglob('*.java')) == sorted(case + '/Use.java' for case in INVALID)
    for case, (status, names, diagnostic) in INVALID.items():
        ds, qs, req = o.fixture_markers(ROOT / 'invalid' / case, ('Use.java',))
        assert set(qs) == {'invalid_' + case}
        assert set(ds) == ({'invalid', 'invalid_ref'} if case == 'ambiguous' else {'invalid'})
        candidates = '-' if names == '-' else ','.join(n + '@invalid.Target' for n in names.split(','))
        yield case, status, candidates, diagnostic, ds, req


def pinned_string(jdk, work):
    # Reuse the existing checked SHA-256 gate before reading any archive member.
    platform = o.pinned_object(jdk, work)
    with zipfile.ZipFile(jdk / 'jmods/java.base.jmod') as archive:
        for name in PLATFORM:
            info = archive.getinfo('classes/' + name + '.class')
            assert 0 < info.file_size < 1024 * 1024
            target = platform / (name + '.class')
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(archive.read(info))
    assert sorted(p.relative_to(platform).as_posix() for p in platform.rglob('*.class')) == sorted(n + '.class' for n in PLATFORM)
    return platform


def compiler_range(declarations, marker):
    # Handwritten methods occupy exactly one line, independent of compiler output.
    path, start, _, _ = declarations[marker]
    source = (ROOT / 'source' / path).read_bytes()
    line_start = source.rfind(b'\n', 0, start) + 1
    line_end = source.find(b'\n', start)
    line = source[line_start:line_end]
    return line_start + len(line) - len(line.lstrip()), line_end


def compare(expected, queries, declarations, javac, native, platform):
    o.compare_reference(expected, queries, declarations, javac, native, platform)
    for q, (_, selected, _) in expected.items():
        marker = selected.split('@')[0]
        assert tuple(map(int, javac[q][4:])) == compiler_range(declarations, marker), \
            q + ': exact compiler declaration range differs'


def baseline_expectations(expected):
    result = dict(expected)
    for q, (status, selected, candidates) in expected.items():
        if status != 'Resolved':
            continue
        if q in {'string_arity_one', 'string_arity_two'}:
            result[q] = ('OverloadNotResolved', selected, 'arity_one@literal.Target,arity_two@literal.Target')
        else:
            result[q] = ('CallNotResolved', selected, candidates)
    return result


def run_suite(jdk, native, work, baseline=False):
    env = {'PATH': '/nonexistent'}
    helper, empty = work / 'helper', work / 'empty'
    helper.mkdir(); empty.mkdir()
    platform = pinned_string(jdk, work)
    o.run([jdk / 'bin/javac', '--release', '21', '-proc:none', '-implicit:none',
           '-encoding', 'UTF-8', '-classpath', empty, '-sourcepath', empty, '-d', helper,
           o.ORACLE / 'BindingOracle.java'], env)
    ds, qs, req, expected = fixture()
    request = work / 'requests.tsv'; request.write_text(req)
    javac = o.records(o.run([jdk / 'bin/java', '-cp', helper, 'BindingOracle', ROOT / 'source', request, empty, empty], env).stdout)
    actual = o.records(o.run([native, ROOT / 'source', request, platform], env).stdout)
    for q in sorted(expected):
        print(q + '\tjavac=' + '|'.join(javac[q]) + '\tnative=' + '|'.join(actual[q]), flush=True)
    compare(baseline_expectations(expected) if baseline else o.current_null_expectations('string-v1', expected, qs, ds), qs, ds, javac, actual, platform)
    for case, status, candidates, diagnostic, ids, contents in invalid_fixtures():
        request.write_text(contents)
        rejected = o.run([jdk / 'bin/java', '-cp', helper, 'BindingOracle', ROOT / 'invalid' / case, request, empty, empty], env, False)
        assert rejected.returncode == 2 and diagnostic in rejected.stderr and not rejected.stdout, rejected.stderr
        result = o.records(o.run([native, ROOT / 'invalid' / case, request, platform], env).stdout)
        q = 'invalid_' + case
        print(q + '\tjavac=' + diagnostic + '\tnative=' + '|'.join(result[q]), flush=True)
        assert set(result) == {q}
        o.check_reference_candidates(q, status, candidates, ids, result[q], platform)
    request.write_text(req)
    # Each missing class is an independent control, including all actual ancestors.
    for name in PLATFORM:
        path = platform / (name + '.class')
        contents = path.read_bytes(); path.unlink()
        try:
            omitted = o.records(o.run([native, ROOT / 'source', request, platform], env).stdout)
            assert set(omitted) == set(expected)
            assert all(omitted[q][0] != 'Resolved' for q in expected), 'unsound success without ' + name
        finally:
            path.write_bytes(contents)
        print('missing=' + name + '\tall-refused', flush=True)
    print('PASS: handwritten string suite; baseline=' + str(baseline) + '; no general Java equivalence claimed')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--jdk', type=Path, required=True)
    p.add_argument('--native', type=Path, required=True)
    p.add_argument('--baseline', action='store_true')
    args = p.parse_args()
    with tempfile.TemporaryDirectory(prefix='string-oracle-') as tmp:
        run_suite(args.jdk.resolve(), args.native.resolve(), Path(tmp), args.baseline)


if __name__ == '__main__':
    main()
