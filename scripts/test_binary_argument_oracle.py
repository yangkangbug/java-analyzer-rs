"""Offline contract mutations; no compiler or native executable is launched."""
import copy
import importlib.util
import json
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('binary_argument_test', Path(__file__).with_name('check-binary-argument-oracle.py'))
c = importlib.util.module_from_spec(spec); spec.loader.exec_module(c)


def envelope(rows, manifest):
    return {'schema_version': 1, 'command': 'declaration',
            'capabilities': {'runtime': 'native_rust_no_jvm', 'build_tool_import': False, 'semantic_diagnostics': False},
            'snapshot': {'id': 'unit', 'identity': 'invocation_local', 'manifest': str(manifest)},
            'result': {'index_builds': 1, 'queries': copy.deepcopy(rows)}}


class BinaryArgumentOracleTests(unittest.TestCase):
    def setUp(self):
        self.ds, self.qs, self.req = c.fixture()
        self.source = c.ROOT / 'source'; self.manifest = Path('/fixture/project.json')

    def test_frozen_inventory_and_explicit_case_matrix(self):
        self.assertEqual((len(self.ds), len(self.qs), len(c.CASES)), (3, 3, 8))
        for case in c.CASES:
            self.assertEqual(c.statuses(case, True), ['CallNotResolved'] * 3)
            expected = c.statuses(case)
            self.assertEqual(expected[2], 'CallNotResolved')
            self.assertEqual(expected[1] == 'Resolved', case in ('same_artifact', 'shared_platform'))
            self.assertEqual(expected[0] == 'Resolved', case == 'shared_platform')
        for case, baseline in [('wrong', False), ('same_artifact', 1), ('same_artifact', 'old')]:
            with self.assertRaises(AssertionError): c.statuses(case, baseline)

    def test_revision_rejects_original_string_forecast_not_union(self):
        rows = c.expected_rows(self.source, self.ds, self.qs, 'same_artifact')
        rows[0]['status'] = 'Resolved'
        with self.assertRaises(AssertionError):
            c.compare_native(envelope(rows, self.manifest), self.source, self.manifest, self.ds, self.qs, 'same_artifact')
        rows = c.expected_rows(self.source, self.ds, self.qs, 'shared_platform')
        rows[0]['status'] = 'CallNotResolved'
        with self.assertRaises(AssertionError):
            c.compare_native(envelope(rows, self.manifest), self.source, self.manifest, self.ds, self.qs, 'shared_platform')

    def test_actual_two_compilation_manifest_not_single_root_oracle(self):
        root = Path('/source'); shared = Path('/classes'); platform = Path('/platform')
        manifest = c.manifest_for(root, [shared], [shared], platform, 17, 21)
        self.assertEqual(manifest['schema_version'], 1)
        lib, app = manifest['compilations']
        self.assertEqual(lib['id'], {'project': 'lib', 'source_set': 'main'})
        self.assertEqual(app['id'], {'project': 'app', 'source_set': 'main'})
        self.assertEqual(lib['source_roots'], ['/source/lib'])
        self.assertEqual(app['source_roots'], ['/source/app'])
        self.assertEqual(app['dependencies'], [lib['id']]); self.assertEqual(lib['dependencies'], [])
        self.assertEqual((lib['target_release'], app['target_release']), (17, 21))
        self.assertEqual(lib['classpath'], app['classpath'])
        full = c.manifest_for(root, [Path('/dep.jar')], [Path('/dep.jar')], Path('/jdk/java.base.jmod'))
        for scope in full['compilations']:
            self.assertEqual(scope['platform'], [{'kind': 'jmod', 'path': '/jdk/java.base.jmod'}])
            self.assertEqual(scope['classpath'], [{'kind': 'jar', 'path': '/dep.jar'}])

    def test_complete_native_records_and_old_stage_are_exact(self):
        rows = c.expected_rows(self.source, self.ds, self.qs, 'same_artifact')
        result = envelope(rows, self.manifest)
        c.compare_native(result, self.source, self.manifest, self.ds, self.qs, 'same_artifact')
        mutations = [lambda r: r['result']['queries'].pop(),
                     lambda r: r['result']['queries'].append(copy.deepcopy(r['result']['queries'][0])),
                     lambda r: r['result'].update(index_builds=True),
                     lambda r: r['snapshot'].update(manifest='/wrong'),
                     lambda r: r['capabilities'].update(semantic_diagnostics=True)]
        for i in range(3):
            mutations += [lambda r, i=i: r['result']['queries'][i].update(status='Wrong'),
                          lambda r, i=i: r['result']['queries'][i].update(query_index=False),
                          lambda r, i=i: r['result']['queries'][i].update(source_compilation={'project': 'lib', 'source_set': 'main'}),
                          lambda r, i=i: r['result']['queries'][i]['targets'].append(copy.deepcopy(r['result']['queries'][i]['targets'][0])),
                          lambda r, i=i: r['result']['queries'][i]['targets'][0].update(origin='binary'),
                          lambda r, i=i: r['result']['queries'][i]['targets'][0]['range'].update(start_utf16=0)]
        for mutate in mutations:
            wrong = copy.deepcopy(result); mutate(wrong)
            with self.assertRaises(AssertionError): c.compare_native(wrong, self.source, self.manifest, self.ds, self.qs, 'same_artifact')
        old = envelope(c.expected_rows(self.source, self.ds, self.qs, 'same_artifact', True), self.manifest)
        c.compare_native(old, self.source, self.manifest, self.ds, self.qs, 'same_artifact', True)
        with self.assertRaises(AssertionError): c.compare_native(old, self.source, self.manifest, self.ds, self.qs, 'same_artifact')
        with self.assertRaises(AssertionError): c.compare_native(result, self.source, self.manifest, self.ds, self.qs, 'same_artifact', True)

    def test_independent_compiler_owner_and_whole_declaration_ranges(self):
        result = {}
        for query, marker in zip(c.QUERIES, c.TARGETS):
            path, start, _, name = self.ds[marker]; data = (self.source / path).read_bytes()
            begin = data.rfind(b'\n', 0, start) + 1; end = data.find(b'\n', start)
            begin += len(data[begin:end]) - len(data[begin:end].lstrip())
            result[query] = [path, 'METHOD', 'api.Target', name, str(begin), str(end)]
        c.compare_compiler(result, self.source, self.ds)
        for q in result:
            for i in range(6):
                changed = copy.deepcopy(result); changed[q][i] = 'wrong'
                with self.assertRaises(AssertionError): c.compare_compiler(changed, self.source, self.ds)
        with self.assertRaises(AssertionError): c.compare_compiler({}, self.source, self.ds)

    def test_crossed_specificity_cannot_drop_unknown_widening_competitor(self):
        ds, qs, _ = c.cross_fixture(); source = c.ROOT / 'invalid/cross'
        wanted = c.cross_expected(source, ds, qs); good = envelope(wanted, self.manifest)
        c.compare_rows(good, self.manifest, wanted)
        for mutation in ['select', 'omit', 'duplicate', 'wrong_scope']:
            wrong = copy.deepcopy(good); row = wrong['result']['queries'][0]
            if mutation == 'select': row['status'] = 'Resolved'; row['targets'] = row['targets'][:1]
            elif mutation == 'omit': row['targets'].pop()
            elif mutation == 'duplicate': row['targets'][1] = copy.deepcopy(row['targets'][0])
            else: row['targets'][1]['compilation']['project'] = 'app'
            with self.assertRaises(AssertionError): c.compare_rows(wrong, self.manifest, wanted)

    def test_source_dependency_mr_shadow_and_pin_mutations_are_rejected(self):
        for filename in ['source/app/client/Use.java', 'dependency/dep/Token.java', 'mr/dep/Token.java', 'shadow/dep/Token.java',
                         'invalid/cross/lib/api/Target.java', 'files.tsv', 'fixture-manifest.json', 'contract-v2.json', 'history/forecast-v1.py.txt', 'history/candidate-first.log', 'history/budget-diagnosis.json']:
            with tempfile.TemporaryDirectory() as temp:
                root = Path(temp) / 'fixture'; shutil.copytree(c.ROOT, root)
                path = root / filename; path.write_bytes(path.read_bytes() + b'changed')
                with patch.object(c, 'ROOT', root), self.assertRaises(AssertionError): c.fixture()
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / 'fixture'; shutil.copytree(c.ROOT, root)
            (root / 'source/Extra.java').write_text('class Extra {}')
            with patch.object(c, 'ROOT', root), self.assertRaises(AssertionError): c.fixture()

if __name__ == '__main__': unittest.main()
