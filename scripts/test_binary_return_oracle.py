"""Offline frozen-contract regressions; this test never invokes Java or native tools."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import subprocess
import sys
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('binary_return_oracle', Path(__file__).with_name('check-binary-return-oracle.py'))
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class BinaryReturnOracleTests(unittest.TestCase):
    def setUp(self):
        self.contract = json.loads((m.ROOT / 'contract.json').read_text())

    def test_frozen_fixture_and_contract_are_complete_without_execution(self):
        with patch.object(m.o, 'run', side_effect=AssertionError('offline tests must not execute tools')):
            prediction = m.fixture()
            self.assertEqual(len(prediction['compiler']), 20)
            self.assertEqual(len(prediction['native_positive']), 14)
            self.assertEqual(len(prediction['native_refusal']), 6)
            self.assertEqual(set(prediction['compiler']), set(self.contract['query_order']))
            self.assertEqual(m.adversarial_controls(self.contract['candidate']), 6)

    def test_positive_predictions_are_independent_exact_binary_identities(self):
        prediction = m.fixture()
        baseline = self.contract['baseline']['queries']
        candidate = self.contract['candidate']['queries']
        for query, before, after in zip(self.contract['query_order'], baseline, candidate):
            if query in prediction['native_positive']:
                self.assertNotEqual(before['status'], 'Resolved')
                self.assertEqual(after['status'], 'Resolved')
                self.assertEqual(after['targets'], [m.target(*prediction['compiler'][query])])
            else:
                self.assertEqual(before, after)
                self.assertNotEqual(after['status'], 'Resolved')

    def test_numeric_type_and_inventory_changes_are_rejected(self):
        wanted = self.contract['candidate']
        wrong = copy.deepcopy(wanted)
        wrong['queries'][0]['query_index'] = False
        with self.assertRaises(AssertionError):
            m.compare(wrong, wanted)
        wrong = copy.deepcopy(wanted)
        wrong['queries'].reverse()
        with self.assertRaises(AssertionError):
            m.compare(wrong, wanted)

    def test_optimized_python_refuses_before_argument_parsing_or_capture(self):
        result = subprocess.run([sys.executable, '-O', str(Path(m.__file__)), '--help'],
                                text=True, capture_output=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('requires Python without -O/PYTHONOPTIMIZE', result.stderr)
        self.assertNotIn('usage:', result.stdout)

    def test_duplicate_json_fields_are_rejected(self):
        with self.assertRaises(AssertionError):
            json.loads('{"status":"Resolved","status":"Unresolved"}', object_pairs_hook=m.unique_object)

    def test_normalization_does_not_erase_identity(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            platform = root / 'java.base.jmod'
            record = {'path': str(root / 'dependency/dep/Base.class'),
                      'platform': str(platform) + '!/classes/java/lang/String.class',
                      'binary_name': 'dep/Base', 'descriptor': '(I)I'}
            value = m.normalize(record, root, platform)
            self.assertEqual(value['path'], '@DEPENDENCY@/dep/Base.class')
            self.assertEqual(value['platform'], '@PLATFORM@!/classes/java/lang/String.class')
            self.assertEqual(value['binary_name'], 'dep/Base')
            self.assertEqual(value['descriptor'], '(I)I')


if __name__ == '__main__':
    unittest.main()

