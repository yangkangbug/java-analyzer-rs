#!/usr/bin/env python3
"""Optional pinned javac mixed strict-invocation oracle; never runs fixture programs."""
import argparse
import importlib.util
from pathlib import Path
import tempfile

_spec = importlib.util.spec_from_file_location('mixed_prior_oracle', Path(__file__).with_name('check-strict-reference-oracle.py'))
prior = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(prior)
o = prior.o
ROOT = o.ORACLE / 'reference-return-v1'
FILES = ('Use.java', 'api/Factory.java', 'api/Sink.java', 'model/Child.java', 'model/Product.java', 'shadow/Product.java')
CASES = frozenset(('direct', 'parens', 'array', 'generic', 'deep', 'unknown_inner', 'argument'))
SELECTABLE = frozenset(('direct', 'parens'))
DECLARATIONS = frozenset(('product_value', 'product_next', 'child_value', 'shadow_value', 'make', 'array', 'generic', 'consume'))
INVALID = {'unknown': ('ReceiverNotBound', '-', 'compiler.err.cant.resolve.location')}


def fixture():
    assert (ROOT / 'files.tsv').read_text().splitlines() == list(FILES)
    ds, qs, req = o.fixture_markers(ROOT / 'source', FILES)
    expected = {}
    for q, status, selected, candidates in o.rows(ROOT / 'expected.tsv', 4):
        assert q not in expected
        expected[q] = status, selected, candidates
    assert set(qs) == set(expected) == {'return_' + case for case in CASES}
    assert set(ds) == DECLARATIONS
    assert {q for q, row in expected.items() if row[0] == 'Resolved'} == {'return_' + x for x in SELECTABLE}
    return ds, qs, req, expected


def invalid_fixtures():
    assert sorted(p.relative_to(ROOT / 'invalid').as_posix() for p in (ROOT / 'invalid').rglob('*.java')) == ['unknown/Use.java']
    ds, qs, req = o.fixture_markers(ROOT / 'invalid/unknown', ('Use.java',))
    assert set(qs) == {'invalid_unknown'} and not ds
    yield 'unknown', 'ReceiverNotBound', '-', 'compiler.err.cant.resolve.location', ds, req


def valid_refusal_fixture():
    assert sorted(p.relative_to(ROOT / 'valid').as_posix() for p in (ROOT / 'valid').rglob('*.java')) == ['concat/Use.java']
    ds, qs, request = o.fixture_markers(ROOT / 'valid/concat', ('Use.java',))
    assert not ds and set(qs) == {'concat_plus', 'concat_assign', 'concat_lhs'}
    return request


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
    return {q: row if q == 'return_argument' else ('Unresolved', row[1], '-')
            for q, row in expected.items()}


def omission_expectations(expected):
    return {q: row if q == 'return_argument' else ('ReceiverNotBound', row[1], '-')
            for q, row in expected.items()}


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
    compare(baseline_expectations(expected) if baseline else o.current_source_invocation_expectations('reference-return-v1', expected, qs, ds), qs, ds, compiler, native_rows, platform)
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
        o.check_reference_candidates(q, 'Unresolved' if baseline else status, candidates, ids, result[q], platform)
    # These compiler-valid String results have binary methods, so independently
    # prove compile validity without asking the source-only helper for a target.
    valid_requests = valid_refusal_fixture()
    request.write_text('\n'.join(line for line in valid_requests.splitlines() if not line.startswith('Q\t')) + '\n')
    valid = o.run([jdk / 'bin/java', '-cp', helper, 'BindingOracle', ROOT / 'valid/concat', request, empty, empty], env)
    assert not valid.stdout and not valid.stderr
    request.write_text(valid_refusal_fixture())
    result = o.records(o.run([native, ROOT / 'valid/concat', request, platform], env).stdout)
    assert set(result) == {'concat_plus', 'concat_assign', 'concat_lhs'}
    for q in result:
        o.check_reference_candidates(q, 'Unresolved' if baseline else 'ReceiverNotBound', '-', {}, result[q], platform)
    print('PASS: compiler-valid String + and both += value-origin receiver refusal controls', flush=True)
    request.write_text(req)
    path = platform / 'java/lang/Object.class'; contents = path.read_bytes(); path.unlink()
    try:
        omitted = o.records(o.run([native, ROOT / 'source', request, platform], env).stdout)
        assert set(omitted) == set(expected)
        for q, row in (baseline_expectations(expected) if baseline else omission_expectations(expected)).items():
            o.check_reference_candidates(q, row[0], row[2], ds, omitted[q], platform)
    finally:
        path.write_bytes(contents)
    print('PASS: reference return oracle; explicit current invocation overlay selects 4, retains 3 valid refusals; historical baseline separate; 1 compiler-rejected case; Object omission exact refusals; baseline=' + str(baseline), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--jdk', type=Path, required=True)
    parser.add_argument('--jdk-archive', type=Path, required=True)
    parser.add_argument('--native', type=Path, required=True)
    parser.add_argument('--baseline', action='store_true')
    args = parser.parse_args()
    print('verified-jdk-archive-sha256=' + prior.verify_jdk(args.jdk.resolve(), args.jdk_archive.resolve()), flush=True)
    with tempfile.TemporaryDirectory(prefix='reference-return-oracle-') as tmp:
        run_suite(args.jdk.resolve(), args.native.resolve(), Path(tmp), args.baseline)


if __name__ == '__main__':
    main()
