"""Offline checker controls. No Java or native binaries are invoked."""
import importlib.util
import copy
from pathlib import Path
import unittest


def load(name, file):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(file))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


oracle = load('local_oracle', 'check-semantic-oracle.py')
second = load('local_second', 'check-second-project.py')


class LocalOracleControls(unittest.TestCase):
    def setUp(self):
        self.declarations = {'local': ('Case.java', 20, 25, 'value'),
                             'field': ('Case.java', 60, 65, 'value')}
        self.queries = {'use': ('Case.java', 80, 85, 'value')}
        self.expected = {'use': ('Resolved', 'local')}
        self.javac = {'use': ['Case.java', 'LOCAL_VARIABLE', 'Case', 'value', '16', '30']}
        self.native = {'use': ['Resolved', '1', 'Case.java|Local||value|20|25']}

    def check(self, native):
        oracle.compare(self.expected, self.queries, self.declarations, self.javac, native)

    def test_exact_local(self):
        self.check(self.native)

    def test_missing_wrong_kind_owner_range_and_shadow(self):
        for target in ['Case.java|Field|Case|value|20|25',
                       'Case.java|Local|Case|value|20|25',
                       'Case.java|Local||value|21|25',
                       'Case.java|Local||value|60|65']:
            with self.subTest(target=target), self.assertRaises(AssertionError):
                self.check({'use': ['Resolved', '1', target]})
        with self.assertRaises(AssertionError):
            self.check({})
        with self.assertRaises(AssertionError):
            self.check({**self.native, 'extra': self.native['use']})

    def test_unsupported_cannot_publish_target(self):
        self.expected['use'] = ('Unresolved', 'local')
        self.check({'use': ['Unresolved', '0']})
        with self.assertRaises(AssertionError):
            self.check({'use': ['Unresolved', '1', self.native['use'][2]]})

    def test_fixture_inventory_and_golden_completeness(self):
        root = oracle.ORACLE / 'local-type-v1'
        ds, qs, _ = oracle.fixture_markers(root / 'source', ('Containment.java',))
        rows = oracle.rows(root / 'expected.tsv', 3)
        self.assertEqual(len(rows), 15)
        self.assertEqual({row[0] for row in rows}, set(qs))
        self.assertTrue(all(row[2] in ds for row in rows))


class RealProbeControls(unittest.TestCase):
    def test_ci_preserves_default_and_separate_additive_gate(self):
        workflow = (second.ROOT / '.github/workflows/extended-acceptance.yml').read_text()
        original = workflow.split('name: Query second real project through shipped CLI', 1)[1].split('      - name:', 1)[0]
        additive = workflow.split('name: Additive Gson local-type containment probes', 1)[1].split('      - name:', 1)[0]
        self.assertIn('check-second-project.py', original)
        self.assertNotIn('--local-types', original)
        self.assertIn('check-second-project.py', additive)
        self.assertIn('--local-types', additive)
        self.assertEqual(len(__import__('json').loads((second.CORPUS / 'query-plan.json').read_text())['queries']), 16)

    def test_real_response_mutations(self):
        helper = load('second_target_test', 'test_second_project.py').TargetValidationTests()
        helper.setUp()
        helper.expected['targets'][0]['kind'] = 'Local'
        helper.expected['targets'][0]['owner'] = ''
        helper.row['targets'] = [second.expected_target(helper.expected['targets'][0], helper.fixture)]
        probe = {**helper.query, 'expected': helper.expected}
        second.check_local_type_response({'result': {'queries': [helper.row]}}, [probe], helper.fixture)
        for key, value in [('kind', 'Field'), ('owner', 'example.Alpha')]:
            row = copy.deepcopy(helper.row)
            row['targets'][0][key] = value
            with self.subTest(key=key), self.assertRaises(second.AcceptanceError):
                second.check_local_type_response({'result': {'queries': [row]}}, [probe], helper.fixture)
        row = copy.deepcopy(helper.row)
        row['targets'][0]['range']['start_byte'] += 1
        with self.assertRaises(second.AcceptanceError):
            second.check_local_type_response({'result': {'queries': [row]}}, [probe], helper.fixture)
        probe['expected'] = {'status': 'Unresolved', 'targets': []}
        row = copy.deepcopy(helper.row)
        row['status'] = 'Unresolved'
        with self.assertRaises(second.AcceptanceError):
            second.check_local_type_response({'result': {'queries': [row]}}, [probe], helper.fixture)

    def test_missing_extra_query_records_rejected(self):
        for actual in ([], [{}, {}]):
            with self.assertRaises(second.AcceptanceError):
                second.check_local_type_response({'result': {'queries': actual}}, [{}], None)


if __name__ == '__main__':
    unittest.main()
