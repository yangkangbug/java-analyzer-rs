#!/usr/bin/env python3
"""Verified developer javac targets plus genuinely multi-compilation native gates.

Does not run fixture programs. Compiler positives and native-only conservative
identity refusals are separate evidence, not claims about JVM loader identity.
"""
import argparse
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import tempfile
import zipfile

spec = importlib.util.spec_from_file_location('binary_prior', Path(__file__).with_name('check-strict-reference-oracle.py'))
prior = importlib.util.module_from_spec(spec); spec.loader.exec_module(prior)
o = prior.o
ROOT = o.ORACLE / 'binary-argument-v1'
FIXTURE_SHA256 = 'a806e4776b20dcfe9c5168b24e893f9c59427d7dae7367947c5d2e7437406338'
CONTRACT_SHA256 = 'd1d7a7b428eb8dbd94ca1ce3b1d382e37d84dde09c3221edae4f59a6b8ebae77'
FILES = ('app/client/Use.java', 'lib/api/Target.java')
QUERIES = ('binary_string', 'binary_token', 'binary_widen_refusal')
TARGETS = ('string', 'token', 'widen')
CASES = ('same_artifact', 'different_origin', 'different_ancestor', 'source_shadow',
         'missing_ancestor', 'multi_release', 'missing_platform', 'shared_platform')


def fixture():
    manifest = (ROOT / 'fixture-manifest.json').read_bytes()
    assert hashlib.sha256(manifest).hexdigest() == FIXTURE_SHA256, 'frozen handwritten oracle inputs'
    contract_path = ROOT / 'contract-v2.json'
    assert sha(contract_path) == CONTRACT_SHA256, 'explicit v2 revision'
    contract = json.loads(contract_path.read_text())
    assert contract['supersedes_fixture_sha256'] == FIXTURE_SHA256
    for name, checksum in contract['evidence'].items(): assert sha(ROOT / name) == checksum
    pinned = json.loads(manifest)['files']
    assert sorted(p.relative_to(ROOT).as_posix() for p in ROOT.rglob('*') if p.is_file() and p.name not in ('fixture-manifest.json', 'README.md')) == sorted(list(pinned) + ['contract-v2.json'] + list(contract['evidence']))
    for name, checksum in pinned.items(): assert sha(ROOT / name) == checksum, name + ': source bytes changed'
    assert (ROOT / 'files.tsv').read_text().splitlines() == list(FILES)
    ds, qs, req = o.fixture_markers(ROOT / 'source', FILES)
    assert set(qs) == set(QUERIES) and set(ds) == set(TARGETS)
    cross_fixture()
    assert sorted(p.relative_to(ROOT / 'dependency').as_posix() for p in (ROOT / 'dependency').rglob('*.java')) == ['dep/Base.java', 'dep/Token.java']
    for directory in ('mr', 'shadow'):
        assert sorted(p.relative_to(ROOT / directory).as_posix() for p in (ROOT / directory).rglob('*.java')) == ['dep/Token.java']
    return ds, qs, req


def statuses(case, baseline=False):
    assert case in CASES and type(baseline) is bool
    if baseline or case == 'missing_platform': return ['CallNotResolved'] * 3
    return ['Resolved' if case == 'shared_platform' else 'CallNotResolved',
            'Resolved' if case in ('same_artifact', 'shared_platform') else 'CallNotResolved', 'CallNotResolved']


def utf16(data, offset):
    return len(data[:offset].decode().encode('utf-16-le')) // 2


def expected_rows(source, ds, qs, case, baseline=False):
    result = []
    for i, (query, declaration, status) in enumerate(zip(QUERIES, TARGETS, statuses(case, baseline))):
        file, at, _, _ = qs[query]; target, start, end, name = ds[declaration]
        data = (source / target).read_bytes()
        result.append({'query_index': i, 'compilation': {'project': 'app', 'source_set': 'main'},
                       'source_compilation': {'project': 'app', 'source_set': 'main'},
                       'file': str(source / file), 'position': {'encoding': 'utf8', 'offset': at}, 'byte_offset': at,
                       'status': status, 'targets': [{'compilation': {'project': 'lib', 'source_set': 'main'},
                          'origin': 'source', 'source_available': True, 'path': str(source / target),
                          'kind': 'Method', 'owner': 'api.Target', 'name': name,
                          'range': {'start_byte': start, 'end_byte': end, 'start_utf16': utf16(data, start), 'end_utf16': utf16(data, end)}}]})
    return result


def compare_rows(response, manifest, wanted):
    assert set(response) == {'schema_version', 'command', 'capabilities', 'snapshot', 'result'}
    assert response['schema_version'] == 1 and type(response['schema_version']) is int
    assert response['command'] == 'declaration'
    capabilities = response['capabilities']
    assert capabilities['runtime'] == 'native_rust_no_jvm'
    assert capabilities['build_tool_import'] is False and capabilities['semantic_diagnostics'] is False
    snapshot = response['snapshot']
    assert set(snapshot) == {'id', 'identity', 'manifest'} and isinstance(snapshot['id'], str) and snapshot['id']
    assert snapshot['identity'] == 'invocation_local' and snapshot['manifest'] == str(manifest)
    wanted = {'index_builds': 1, 'queries': wanted}
    # Canonical JSON distinguishes boolean coordinates from integers.
    assert json.dumps(response['result'], sort_keys=True) == json.dumps(wanted, sort_keys=True), (response['result'], wanted)



def compare_native(response, source, manifest, ds, qs, case, baseline=False):
    compare_rows(response, manifest, expected_rows(source, ds, qs, case, baseline))


def cross_fixture():
    root = ROOT / 'invalid/cross'
    ds, qs, req = o.fixture_markers(root, FILES)
    assert set(ds) == {'exact', 'widen'} and set(qs) == {'cross_unknown_competitor'}
    return ds, qs, req


def cross_expected(source, ds, qs):
    file, at, _, _ = qs['cross_unknown_competitor']
    targets = []
    for marker in ('exact', 'widen'):
        path, start, end, name = ds[marker]; data = (source / path).read_bytes()
        targets.append({'compilation': {'project': 'lib', 'source_set': 'main'}, 'origin': 'source',
                        'source_available': True, 'path': str(source / path), 'kind': 'Method', 'owner': 'api.Target', 'name': name,
                        'range': {'start_byte': start, 'end_byte': end, 'start_utf16': utf16(data, start), 'end_utf16': utf16(data, end)}})
    return [{'query_index': 0, 'compilation': {'project': 'app', 'source_set': 'main'},
             'source_compilation': {'project': 'app', 'source_set': 'main'}, 'file': str(source / file),
             'position': {'encoding': 'utf8', 'offset': at}, 'byte_offset': at,
             'status': 'OverloadNotResolved', 'targets': targets}]


def compare_compiler(actual, source, ds):
    assert set(actual) == set(QUERIES)
    for q, marker in zip(QUERIES, TARGETS):
        path, start, _, name = ds[marker]; data = (source / path).read_bytes()
        line = data.rfind(b'\n', 0, start) + 1; end = data.find(b'\n', start)
        begin = line + len(data[line:end]) - len(data[line:end].lstrip())
        assert actual[q] == [path, 'METHOD', 'api.Target', name, str(begin), str(end)], (q, actual[q])


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        assert key not in result, 'duplicate JSON field'
        result[key] = value
    return result


def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()


def manifest_for(source, lib_cp, app_cp, platform, lib_release=21, app_release=21):
    def config(project, cp, release):
        return {'id': {'project': project, 'source_set': 'main'},
                'source_roots': [str(source / project)],
                'dependencies': [{'project': 'lib', 'source_set': 'main'}] if project == 'app' else [],
                'classpath': [{'kind': 'jar' if path.suffix == '.jar' else 'directory', 'path': str(path)} for path in cp + ([] if platform.suffix == '.jmod' else [platform])],
                'platform': [{'kind': 'jmod', 'path': str(platform)}] if platform.suffix == '.jmod' else [],
                'target_release': release}
    return {'schema_version': 1, 'compilations': [config('lib', lib_cp, lib_release), config('app', app_cp, app_release)]}


def run_suite(jdk, native, work, baseline=False):
    env = {'PATH': '/nonexistent'}; ds, qs, compiler_req = fixture()
    helper, empty, dependency = work / 'helper', work / 'empty', work / 'dependency'
    helper.mkdir(); empty.mkdir(); dependency.mkdir()
    platform = prior.pinned_platform(jdk, work)
    o.run([jdk / 'bin/javac', '--release', '21', '-proc:none', '-implicit:none', '-encoding', 'UTF-8', '-classpath', empty, '-sourcepath', empty, '-d', helper, o.ORACLE / 'BindingOracle.java'], env)
    o.run([jdk / 'bin/javac', '--release', '17', '-proc:none', '-implicit:none', '-classpath', empty, '-sourcepath', empty, '-d', dependency,
           ROOT / 'dependency/dep/Base.java', ROOT / 'dependency/dep/Token.java'], env)
    assert sorted(p.relative_to(dependency).as_posix() for p in dependency.rglob('*.class')) == ['dep/Base.class', 'dep/Token.class']
    request = work / 'compiler-requests.tsv'; request.write_text(compiler_req)
    compiler = o.records(o.run([jdk / 'bin/java', '-cp', helper, 'BindingOracle', ROOT / 'source', request, dependency, empty], env).stdout)
    compare_compiler(compiler, ROOT / 'source', ds)
    mr_classes = work / 'mr-classes'; mr_classes.mkdir()
    o.run([jdk / 'bin/javac', '--release', '21', '-proc:none', '-implicit:none', '-classpath', dependency, '-sourcepath', empty, '-d', mr_classes, ROOT / 'mr/dep/Token.java'], env)
    assert sorted(p.relative_to(mr_classes).as_posix() for p in mr_classes.rglob('*.class')) == ['dep/Token.class']
    receipts = []
    for case in CASES:
        stage = work / case; stage.mkdir(); source = stage / 'source'; shutil.copytree(ROOT / 'source', source)
        shared = stage / 'shared'; shutil.copytree(dependency, shared)
        local_platform = stage / 'platform'; shutil.copytree(platform, local_platform)
        lib_cp = app_cp = [shared]; lib_release = app_release = 21
        details = {'case': case}
        if case == 'shared_platform':
            local_platform = jdk / 'jmods/java.base.jmod'
            jar = stage / 'shared.jar'
            with zipfile.ZipFile(jar, 'w') as archive:
                for name in ['Base', 'Token']: archive.writestr('dep/' + name + '.class', (shared / ('dep/' + name + '.class')).read_bytes())
            lib_cp = app_cp = [jar]
            details['bounded_proof_shared_artifacts'] = True
        elif case == 'different_origin':
            other = stage / 'other'; shutil.copytree(shared, other); app_cp = [other]
            assert sha(shared / 'dep/Token.class') == sha(other / 'dep/Token.class')
            assert sha(shared / 'dep/Base.class') == sha(other / 'dep/Base.class')
            details['same_bytes_different_origin'] = True
        elif case == 'different_ancestor':
            other = stage / 'other'; (other / 'dep').mkdir(parents=True)
            shutil.copyfile(shared / 'dep/Base.class', other / 'dep/Base.class'); app_cp = [other, shared]
            assert sha(shared / 'dep/Base.class') == sha(other / 'dep/Base.class')
            details['same_root_origin_different_parent_origin'] = True
        elif case == 'source_shadow':
            (source / 'app/dep').mkdir(); shutil.copyfile(ROOT / 'shadow/dep/Token.java', source / 'app/dep/Token.java')
        elif case == 'missing_ancestor': (shared / 'dep/Base.class').unlink()
        elif case == 'multi_release':
            jar = stage / 'multi.jar'
            with zipfile.ZipFile(jar, 'w') as archive:
                archive.writestr('META-INF/MANIFEST.MF', 'Manifest-Version: 1.0\r\nMulti-Release: true\r\n\r\n')
                for name in ['Base', 'Token']: archive.writestr('dep/' + name + '.class', (shared / ('dep/' + name + '.class')).read_bytes())
                archive.writestr('META-INF/versions/21/dep/Token.class', (mr_classes / 'dep/Token.class').read_bytes())
            lib_cp = app_cp = [jar]; lib_release = 17
            details['same_jar_sha256'] = sha(jar); details['selected_releases'] = [17, 21]
        elif case == 'missing_platform': (local_platform / 'java/lang/Object.class').unlink()
        manifest = stage / 'project.json'; manifest.write_text(json.dumps(manifest_for(source, lib_cp, app_cp, local_platform, lib_release, app_release)))
        if case == 'shared_platform':
            # Full java.base origin enumeration exceeds the CLI result-item cap.
            # Keep default summary plus exact manifest/artifact and query checks.
            inventory = json.loads(o.run([native, 'inspect', '--manifest', manifest], env).stdout, object_pairs_hook=unique_object)['result']
            assert {row['id']['project'] for row in inventory['compilations']} == {'lib', 'app'}
            details['shared_platform_sha256'] = sha(local_platform)
            details['shared_dependency_jar_sha256'] = sha(jar)
            details['inventory_mode'] = 'summary; full java.base exceeds result-item limit'
        else:
            inventory = json.loads(o.run([native, 'inspect', '--manifest', manifest, '--details'], env).stdout, object_pairs_hook=unique_object)['result']
            assert inventory['detail_mode'] == 'full'
            scopes = {row['id']['project']: row for row in inventory['compilations']}
            assert set(scopes) == {'lib', 'app'}
            for project in scopes:
                assert scopes[project]['id'] == {'project': project, 'source_set': 'main'}
                assert scopes[project]['source_paths'] == sorted(str(p) for p in (source / project).rglob('*.java'))
            origins = {project: scopes[project]['class_origins'] for project in scopes}
            if case == 'shared_platform':
                for project in ('lib', 'app'):
                    assert str(local_platform) + '!/classes/java/lang/String.class' in origins[project]
                    assert str(jar) + '!/dep/Token.class' in origins[project]
            elif case == 'multi_release':
                assert str(jar) + '!/dep/Token.class' in origins['lib']
                assert str(jar) + '!/META-INF/versions/21/dep/Token.class' in origins['app']
                assert str(jar) + '!/dep/Token.class' not in origins['app']
            elif case == 'different_origin':
                assert str(shared / 'dep/Token.class') in origins['lib'] and str(other / 'dep/Token.class') in origins['app']
            elif case == 'different_ancestor':
                assert str(shared / 'dep/Token.class') in origins['lib'] and str(shared / 'dep/Token.class') in origins['app']
                assert str(shared / 'dep/Base.class') in origins['lib'] and str(other / 'dep/Base.class') in origins['app']
            details['selected_origin_inventory'] = origins
        req = stage / 'requests.json'; req.write_text(json.dumps({'schema_version': 1, 'queries': [
            {'compilation': {'project': 'app', 'source_set': 'main'}, 'file': str(source / qs[q][0]), 'position': {'encoding': 'utf8', 'offset': qs[q][1]}} for q in QUERIES]}))
        before = {str(p.relative_to(source)): sha(p) for p in source.rglob('*.java')}
        result = json.loads(o.run([native, 'declaration', '--manifest', manifest, '--requests', req], env).stdout, object_pairs_hook=unique_object)
        compare_native(result, source, manifest, ds, qs, case, baseline)
        if baseline and case == 'same_artifact':
            o.must_reject(lambda: compare_native(result, source, manifest, ds, qs, case), 'pre-feature baseline must fail positive current contract')
        assert before == {str(p.relative_to(source)): sha(p) for p in source.rglob('*.java')}
        details['statuses'] = [q['status'] for q in result['result']['queries']]
        details['two_distinct_compilations'] = True
        print(json.dumps(details, sort_keys=True), flush=True); receipts.append(details)
    # Java rejects this crossing specificity. Unsupported cross-scope widening
    # cannot be treated as false applicability to manufacture the exact-root winner.
    cross = work / 'cross'; cross.mkdir(); source = cross / 'source'
    shutil.copytree(ROOT / 'invalid/cross', source)
    cds, cqs, creq = cross_fixture(); request.write_text(creq)
    rejected = o.run([jdk / 'bin/java', '-cp', helper, 'BindingOracle', source, request, dependency, empty], env, False)
    assert rejected.returncode == 2 and 'compiler.err.ref.ambiguous' in rejected.stderr and not rejected.stdout, rejected.stderr
    manifest = cross / 'project.json'; manifest.write_text(json.dumps(manifest_for(source, [dependency], [dependency], platform)))
    req = cross / 'requests.json'; at = cqs['cross_unknown_competitor'][1]
    req.write_text(json.dumps({'schema_version': 1, 'queries': [{'compilation': {'project': 'app', 'source_set': 'main'},
                               'file': str(source / cqs['cross_unknown_competitor'][0]), 'position': {'encoding': 'utf8', 'offset': at}}]}))
    result = json.loads(o.run([native, 'declaration', '--manifest', manifest, '--requests', req], env).stdout, object_pairs_hook=unique_object)
    compare_rows(result, manifest, cross_expected(source, cds, cqs))
    print('PASS: compiler-rejected crossed specificity; native retains both exact candidates', flush=True)
    print('PASS: actual two-compilation binary argument oracle; independent javac source targets; 8 native configurations plus crossed-specificity refusal; baseline=' + str(baseline), flush=True)
    return receipts


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--jdk', type=Path, required=True); p.add_argument('--jdk-archive', type=Path, required=True)
    p.add_argument('--native', type=Path, required=True); p.add_argument('--baseline', action='store_true')
    args = p.parse_args()
    print('verified-jdk-archive-sha256=' + prior.verify_jdk(args.jdk.resolve(), args.jdk_archive.resolve()), flush=True)
    with tempfile.TemporaryDirectory(prefix='binary-argument-oracle-') as temp:
        run_suite(args.jdk.resolve(), args.native.resolve(), Path(temp), args.baseline)

if __name__ == '__main__': main()
