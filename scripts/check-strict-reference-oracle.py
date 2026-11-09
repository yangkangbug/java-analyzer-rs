#!/usr/bin/env python3
"""Optional pinned javac strict-phase oracle; never runs fixture programs or downloads."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import tarfile
import tempfile
import zipfile

SPEC = importlib.util.spec_from_file_location('strict_semantics', Path(__file__).with_name('check-semantic-oracle.py'))
o = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(o)
ROOT = o.ORACLE / 'strict-reference-v1'
FILES = ('Base.java', 'Child.java', 'Derived.java', 'Parent.java', 'Target.java', 'Use.java')
CASES = frozenset(('string', 'child', 'identity', 'specific', 'reverse', 'mixed', 'wrapper', 'loose', 'array', 'unknown', 'static', 'private', 'generic', 'varargs', 'inherited'))
SELECTABLE = frozenset(('string', 'child', 'identity', 'specific', 'reverse', 'mixed'))
DECLARATIONS = frozenset(('string_ref', 'string_scalar', 'parent', 'scalar', 'object', 'specific_scalar', 'specific_parent', 'reverse_parent', 'reverse_scalar', 'reverse_object', 'mixed_ref', 'mixed_scalar', 'mixed_other', 'wrapper_object', 'wrapper_scalar', 'loose_scalar', 'array_parent', 'array_primitive', 'unknown_parent', 'unknown_scalar', 'static_parent', 'static_scalar', 'private_scalar', 'private_parent', 'generic_scalar', 'generic_parent', 'varargs_scalar', 'varargs_parent', 'inherited_scalar', 'inherited_parent'))
PLATFORM = ('java/lang/String', 'java/lang/CharSequence', 'java/lang/Object', 'java/lang/Integer', 'java/lang/Number', 'java/io/Serializable', 'java/lang/Comparable', 'java/lang/constant/Constable', 'java/lang/constant/ConstantDesc')
INVALID = {'duplicate': ('Ambiguous', 'first,second', 'compiler.err.already.defined'),
           'missing': ('OverloadNotResolved', 'first,second', 'compiler.err.cant.resolve.location'),
           'unknown': ('OverloadNotResolved', 'first,second', 'compiler.err.cant.resolve.location'),
           'no_strict': ('CallNotResolved', 'first', 'compiler.err.prob.found.req')}
BASELINE = {'string': 'string_ref,string_scalar', 'child': 'parent,scalar', 'identity': 'parent,scalar',
            'specific': 'object,specific_scalar,specific_parent',
            'reverse': 'reverse_parent,reverse_scalar,reverse_object',
            'mixed': 'mixed_ref,mixed_scalar,mixed_other'}


def fixture():
    assert (ROOT / 'files.tsv').read_text().splitlines() == list(FILES)
    ds, qs, req = o.fixture_markers(ROOT / 'source', FILES)
    expected = {}
    for q, status, selected, candidates in o.rows(ROOT / 'expected.tsv', 4):
        assert q not in expected
        expected[q] = (status, selected, candidates)
    assert set(qs) == set(expected) == {'strict_' + case for case in CASES}
    assert set(ds) == DECLARATIONS
    assert {q for q, row in expected.items() if row[0] == 'Resolved'} == {'strict_' + x for x in SELECTABLE}
    return ds, qs, req, expected


def invalid_fixtures():
    assert sorted(p.relative_to(ROOT / 'invalid').as_posix() for p in (ROOT / 'invalid').rglob('*.java')) == sorted(case + '/Use.java' for case in INVALID)
    for case, (status, names, diagnostic) in INVALID.items():
        ds, qs, req = o.fixture_markers(ROOT / 'invalid' / case, ('Use.java',))
        assert set(qs) == {'invalid_' + case}
        assert set(ds) == set(names.split(','))
        yield case, status, ','.join(n + '@strict.Target' for n in names.split(',')), diagnostic, ds, req


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
        q = 'strict_' + case
        result[q] = ('OverloadNotResolved', expected[q][1], ','.join(n + '@strict.Target' for n in names.split(',')))
    return result


def omitted_expectations(expected):
    result = baseline_expectations(expected)
    # Without Object, these legacy formal inventories report unknown signatures.
    for q in ('strict_specific', 'strict_reverse', 'strict_wrapper'):
        _, selected, candidates = result[q]
        result[q] = ('SignatureNotCompared', selected, candidates)
    return result


def verify_jdk(jdk, archive):
    """Validate the pinned archive AND installed executable/library bytes before execution."""
    pin = json.loads((o.ROOT / 'scripts/acceptance-inputs.json').read_text())['jdk']
    with archive.open('rb') as stream:
        assert hashlib.file_digest(stream, 'sha256').hexdigest() == pin['sha256'], 'JDK archive checksum differs'
    with tarfile.open(archive, 'r:gz') as tar:
        for member in tar:
            if not member.isfile():
                continue
            path = Path(member.name)
            assert path.parts[0] == pin['root'] and '..' not in path.parts
            installed = jdk.joinpath(*path.parts[1:])
            with tar.extractfile(member) as source, installed.open('rb') as target:
                assert hashlib.file_digest(source, 'sha256').digest() == hashlib.file_digest(target, 'sha256').digest(), 'Installed JDK differs: ' + str(path)
    return pin['sha256']


def pinned_platform(jdk, work):
    platform = o.pinned_object(jdk, work)
    with zipfile.ZipFile(jdk / 'jmods/java.base.jmod') as archive:
        for name in PLATFORM:
            info = archive.getinfo('classes/' + name + '.class')
            assert 0 < info.file_size < 1024 * 1024
            path = platform / (name + '.class')
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(archive.read(info))
    assert sorted(p.relative_to(platform).as_posix() for p in platform.rglob('*.class')) == sorted(n + '.class' for n in PLATFORM)
    return platform


def run_suite(jdk, native, work, baseline=False):
    env = {'PATH': '/nonexistent'}
    helper, empty = work / 'helper', work / 'empty'
    helper.mkdir(); empty.mkdir()
    platform = pinned_platform(jdk, work)
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
        print(q + '\tnative=' + '|'.join(result[q]), flush=True)
        assert set(result) == {q}
        o.check_reference_candidates(q, status, candidates, ids, result[q], platform)
    request.write_text(req)
    path = platform / 'java/lang/Object.class'; contents = path.read_bytes(); path.unlink()
    try:
        omitted = o.records(o.run([native, ROOT / 'source', request, platform], env).stdout)
        assert set(omitted) == set(expected)
        for q, row in omitted_expectations(expected).items():
            o.check_reference_candidates(q, row[0], row[2], ds, omitted[q], platform)
    finally:
        path.write_bytes(contents)
    print('PASS: strict-reference suite; javac executed; baseline=' + str(baseline) + '; 6 bounded selections, 9 compiler-valid native limitations, 4 compiler-rejected cases; Object omission preserves exact refusals', flush=True)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--jdk', type=Path, required=True)
    p.add_argument('--jdk-archive', type=Path, required=True)
    p.add_argument('--native', type=Path, required=True)
    p.add_argument('--baseline', action='store_true')
    args = p.parse_args()
    print('verified-jdk-archive-sha256=' + verify_jdk(args.jdk.resolve(), args.jdk_archive.resolve()), flush=True)
    with tempfile.TemporaryDirectory(prefix='strict-reference-oracle-') as tmp:
        run_suite(args.jdk.resolve(), args.native.resolve(), Path(tmp), args.baseline)


if __name__ == '__main__':
    main()
