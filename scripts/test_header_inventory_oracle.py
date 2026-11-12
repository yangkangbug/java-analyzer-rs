"""Offline exact-contract and preservation controls for return inventory oracle."""
import copy
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

_spec = importlib.util.spec_from_file_location('header_oracle', Path(__file__).with_name('check-header-inventory-oracle.py'))
m = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(m)


class HeaderInventoryOracleTests(unittest.TestCase):
    def setUp(self):
        self.predictions, self.contract = m.checked_fixture()

    def test_exact_inventory_and_positive_forecasts(self):
        self.assertEqual(len(self.contract['cases']), 27)
        positives = [n for n, s in self.predictions['cases'].items() if s['kind'] == 'positive']
        self.assertEqual(len(positives), 11)
        for name in positives:
            row = self.contract['cases'][name]
            self.assertNotEqual(row['baseline'], row['current'])
            self.assertEqual(row['current']['probe'][0:2], ['Resolved', '1'])
            self.assertIsNotNone(row['compiler'])

    def test_competitors_and_return_inference_remain_refused(self):
        for name in ('generic_method_competitor', 'generic_parameter_competitor', 'no_return_inference', 'primitive_array_bound_deferred'):
            row = self.contract['cases'][name]
            self.assertEqual(row['baseline'], row['current'])
            self.assertNotEqual(row['current']['probe'][0], 'Resolved')
        for name in ('generic_method_competitor', 'generic_parameter_competitor'):
            self.assertEqual(self.contract['cases'][name]['current']['probe'][1], '2')

    def test_illegal_and_bounded_headers_have_explicit_refusals(self):
        for name, spec in self.predictions['cases'].items():
            if spec['kind'] in ('invalid', 'bounded'):
                row = self.contract['cases'][name]
                self.assertEqual(row['current']['probe'][0:2], ['OverloadNotResolved', '2'])
                self.assertEqual(row['compiler'] is None, spec['kind'] == 'invalid')
        # Retain the historical bug, rather than claiming the old binary refused it.
        self.assertEqual(self.contract['cases']['invalid_top_wildcard']['baseline']['probe'][0], 'Resolved')

    def test_complete_records_reject_status_candidate_identity_and_range_mutations(self):
        expected = self.contract['cases']['wildcard']['current']
        for cell, value in ((0, 'OverloadNotResolved'), (1, '0'), (2, 'Case.java|Method|wrong.Owner|take|1|2')):
            changed = copy.deepcopy(expected); changed['probe'][cell] = value
            with self.assertRaises(AssertionError): m.compare(changed, expected)
        for changed in ({}, dict(expected, extra=expected['probe']), {'probe': expected['probe'] + [expected['probe'][-1]]}):
            with self.assertRaises(AssertionError): m.compare(changed, expected)
        negative = self.contract['cases']['generic_parameter_competitor']['current']
        changed = copy.deepcopy(negative); changed['probe'].pop()
        with self.assertRaises(AssertionError): m.compare(changed, negative)

    def test_compiler_full_record_is_not_ignored(self):
        row = self.contract['cases']['wildcard']['compiler']
        changed = copy.deepcopy(row); changed['probe'][2] = 'inventory.Decoy'
        with self.assertRaises(AssertionError): m.compare(changed, row)

    def test_fixture_mutation_and_extra_file_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / 'fixture'; shutil.copytree(m.ROOT, root)
            with patch.object(m, 'ROOT', root):
                m.checked_fixture()
                source = root / 'wildcard/Case.java'; original = source.read_bytes()
                source.write_bytes(original + b'\n')
                with self.assertRaises(AssertionError): m.checked_fixture()
                source.write_bytes(original)
                (root / 'unexpected').write_text('extra')
                with self.assertRaises(AssertionError): m.checked_fixture()

    def test_capture_cannot_overwrite_contract(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaisesRegex(AssertionError, 'never overwrite'):
                m.run_suite(Path(tmp), Path(tmp), Path(tmp), capture=True)

    def test_baseline_requires_exact_old_binary(self):
        with tempfile.TemporaryDirectory() as tmp:
            native = Path(tmp) / 'wrong'; native.write_text('not frozen executable')
            with self.assertRaisesRegex(AssertionError, 'exact published baseline'):
                m.run_suite(Path(tmp), native, Path(tmp), baseline=True)

    def test_optimized_python_cannot_disable_contract(self):
        result = subprocess.run([sys.executable, '-O', str(Path(m.__file__)), '--help'], capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('requires Python without -O', result.stderr)


if __name__ == '__main__':
    unittest.main()
